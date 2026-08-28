//! Seeding `physics_profiles`, `sim_builds` and `maps` **by running code**.
//!
//! # Nothing in here is a literal
//!
//! Wave contracts §F, and `docs/web/URLS.md` §3 behind it: three digests matter
//! and none of them is ever typed by a human into a file.
//!
//! - `physics_profiles.digest` comes from [`straf3_replay::physics_digest`]
//!   over the profile this build actually simulates with.
//! - `maps.collision_digest` comes from compiling the `.map` text through
//!   `straf3-map` here, at seed time, and asking the `CompiledMap` for it.
//! - `sim_builds.build_hash` comes from running
//!   [`straf3_replay::crosstarget`].
//!
//! A migration containing a digest literal would be a claim about the
//! simulation made by someone who was not running it. It would also stop
//! tracking: change the map text or the compiler and the seeded row would go on
//! naming geometry that no longer exists, and every submitted run would be
//! refused for a reason nobody could see.
//!
//! # Seeding is idempotent and never mutates a physics row
//!
//! `physics_profiles` inserts are `on conflict (digest) do nothing`: re-running
//! the seed after a tuning pass adds the *new* constants as a *new* row and
//! leaves the old one exactly as it was. That is §5.4's rule, and it is what
//! makes `/m/coil/cpm@<digest16>` still mean what it meant. The database
//! enforces it independently with a trigger that raises on `update` and
//! `delete`, so a future code path cannot quietly do the other thing.

use std::path::Path;

use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::catalog::is_valid_slug;
use crate::digest16;
use crate::profiles::{self, PROFILE_LAYOUT_VERSION};
use crate::simbuild::{self, SimBuild};

/// What a seed run did, so the operator can see it rather than infer it.
#[derive(Debug, Default, Clone)]
pub struct SeedReport {
    /// Profiles inserted this run (never updated).
    pub profiles_inserted: Vec<(String, u64)>,
    /// Profiles that were already there.
    pub profiles_present: Vec<(String, u64)>,
    /// Maps inserted or refreshed, with the digest that was derived.
    pub maps: Vec<(String, u64)>,
    /// The `sim_builds` row this process stamps runs with.
    pub sim_build: Option<(i32, u64, bool)>,
    /// Map files that would not compile, named rather than skipped silently.
    pub map_failures: Vec<(String, String)>,
}

impl SeedReport {
    /// A human-readable summary. Digests in hex, as everywhere else.
    #[must_use]
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        if let Some((id, hash, ok)) = self.sim_build {
            let _ = writeln!(
                out,
                "sim_build #{id}: build_hash {} · native_verifier_ok {ok}",
                digest16::format(hash)
            );
        }
        for (kind, digest) in &self.profiles_inserted {
            let _ = writeln!(
                out,
                "physics_profiles + {kind} {}",
                digest16::format(*digest)
            );
        }
        for (kind, digest) in &self.profiles_present {
            let _ = writeln!(
                out,
                "physics_profiles = {kind} {}",
                digest16::format(*digest)
            );
        }
        for (slug, digest) in &self.maps {
            let _ = writeln!(
                out,
                "maps            = {slug} collision {}",
                digest16::format(*digest)
            );
        }
        for (name, why) in &self.map_failures {
            let _ = writeln!(out, "maps            ! {name}: {why}");
        }
        out
    }
}

/// Insert every canon profile that is not already present, and return the
/// `sim_builds` row for this process.
///
/// # Errors
///
/// Any database failure. A profile that already exists is not one.
pub async fn seed_profiles(pool: &PgPool, report: &mut SeedReport) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now();
    for canon in profiles::canon() {
        let digest = canon.digest();
        let inserted = sqlx::query(
            "insert into physics_profiles (kind, label, digest, profile_bits, layout_version) \
             values ($1, $2, $3, $4, $5) on conflict (digest) do nothing returning id",
        )
        .bind(canon.kind)
        .bind(profiles::label_for(canon.kind, now))
        .bind(digest16::to_sql(digest))
        .bind(canon.bits())
        .bind(PROFILE_LAYOUT_VERSION)
        .fetch_optional(pool)
        .await?;

        if inserted.is_some() {
            report
                .profiles_inserted
                .push((canon.kind.to_string(), digest));
        } else {
            report
                .profiles_present
                .push((canon.kind.to_string(), digest));
        }
    }
    Ok(())
}

/// Find or create the `sim_builds` row describing this binary.
///
/// # Errors
///
/// Any database failure.
pub async fn ensure_sim_build(pool: &PgPool, build: &SimBuild) -> Result<i32, sqlx::Error> {
    let hash = digest16::to_sql(build.build_hash);
    let row = sqlx::query(
        "insert into sim_builds (sim_version, git_sha, build_hash, native_verifier_ok) \
         values ($1, $2, $3, $4) \
         on conflict (build_hash) do update set native_verifier_ok = excluded.native_verifier_ok \
         returning id",
    )
    .bind(&build.sim_version)
    .bind(&build.git_sha)
    .bind(hash)
    .bind(build.native_verifier_ok)
    .fetch_one(pool)
    .await?;
    row.try_get("id")
}

/// Compile every `.map` in `dir` and record what came out.
///
/// The slug is the file stem; a file whose stem is not a legal `<map>` (URLS.md
/// §2) is reported rather than silently renamed, because a map nobody can name
/// in a URL is a map nobody can play from a link.
///
/// # Errors
///
/// A database failure, or an unreadable directory. A map that will not compile
/// is recorded in the report and does not fail the run — one broken map should
/// not stop the others being seeded.
pub async fn seed_maps(
    pool: &PgPool,
    dir: &Path,
    url_prefix: &str,
    report: &mut SeedReport,
) -> Result<(), SeedError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| SeedError::Maps(format!("{}: {e}", dir.display())))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "map"))
        .collect();
    entries.sort();

    let compiler = simbuild::map_compiler_version();

    for path in entries {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if !is_valid_slug(&stem) {
            report.map_failures.push((
                stem.clone(),
                "the file stem is not a legal <map> slug (URLS.md §2), so no link could name it"
                    .to_string(),
            ));
            continue;
        }

        let source = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                report.map_failures.push((stem, e.to_string()));
                continue;
            }
        };

        // Derived here, by running the compiler. Never a literal.
        let compiled = match straf3_map::compile(&source) {
            Ok(map) => map,
            Err(e) => {
                report.map_failures.push((stem, e.to_string()));
                continue;
            }
        };

        let collision_digest = compiled.collision_digest();
        let source_sha256 = Sha256::digest(source.as_bytes()).to_vec();
        let has_start = compiled
            .triggers_of(straf3_map::TriggerKind::Start)
            .next()
            .is_some();
        let has_finish = compiled
            .triggers_of(straf3_map::TriggerKind::Finish)
            .next()
            .is_some();
        let name = map_name(&compiled).unwrap_or_else(|| stem.clone());
        let author = map_author(&compiled);
        let source_key = format!("{}/{}.map", url_prefix.trim_end_matches('/'), stem);

        // A map's identity is its compiled hulls, so re-seeding after a
        // recompile updates the row in place: the slug is how a URL names it
        // and must keep working. Every run already recorded against the old
        // digest stops verifying, loudly, which is the truth — the geometry
        // moved (see `straf3-replay`'s identity module).
        sqlx::query(
            "insert into maps (slug, name, author, source_sha256, source_key, collision_digest, \
                               map_compiler_version, has_start_trigger, has_finish_trigger) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             on conflict (slug) do update set \
                 name = excluded.name, \
                 author = excluded.author, \
                 source_sha256 = excluded.source_sha256, \
                 source_key = excluded.source_key, \
                 collision_digest = excluded.collision_digest, \
                 map_compiler_version = excluded.map_compiler_version, \
                 has_start_trigger = excluded.has_start_trigger, \
                 has_finish_trigger = excluded.has_finish_trigger",
        )
        .bind(&stem)
        .bind(&name)
        .bind(author.as_deref())
        .bind(&source_sha256)
        .bind(&source_key)
        .bind(digest16::to_sql(collision_digest))
        .bind(&compiler)
        .bind(has_start)
        .bind(has_finish)
        .execute(pool)
        .await?;

        report.maps.push((stem, collision_digest));
    }
    Ok(())
}

/// The map's own name, when `worldspawn` gives one.
fn map_name(compiled: &straf3_map::CompiledMap) -> Option<String> {
    compiled
        .entities
        .iter()
        .find(|e| e.classname == "worldspawn")
        .and_then(|e| e.get("message").or_else(|| e.get("name")))
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// The map's author, when `worldspawn` gives one.
fn map_author(compiled: &straf3_map::CompiledMap) -> Option<String> {
    compiled
        .entities
        .iter()
        .find(|e| e.classname == "worldspawn")
        .and_then(|e| e.get("author").or_else(|| e.get("_author")))
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// Run every seeding step: profiles, this build, then maps.
///
/// # Errors
///
/// A database failure, or an unreadable maps directory.
pub async fn seed_all(
    pool: &PgPool,
    maps_dir: &Path,
    maps_url_prefix: &str,
) -> Result<SeedReport, SeedError> {
    let mut report = SeedReport::default();

    seed_profiles(pool, &mut report).await?;

    let build = SimBuild::derive();
    let id = ensure_sim_build(pool, &build).await?;
    report.sim_build = Some((id, build.build_hash, build.native_verifier_ok));

    seed_maps(pool, maps_dir, maps_url_prefix, &mut report).await?;

    Ok(report)
}

/// What went wrong seeding.
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    /// The database said no.
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
    /// The maps directory could not be read.
    #[error("maps: {0}")]
    Maps(String),
}

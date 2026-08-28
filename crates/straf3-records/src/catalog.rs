//! Maps, physics profiles, and the category key that joins them.
//!
//! # The whole of requirement r8 lives in [`resolve_category`]
//!
//! `docs/web/ARCHITECTURE.md` §5.2: a leaderboard category is **(map, physics
//! profile)**. §5.4 is the part most leaderboard schemas get wrong — when the
//! constants change, every stored time was produced by a game that no longer
//! exists — and `docs/web/URLS.md` §3 turns that into two different things a
//! person can mean by "the CPM board for coil":
//!
//! | URL | Resolves to | Stability |
//! |---|---|---|
//! | `/m/coil/cpm` | the newest `physics_profiles` row of kind `cpm` | *moves* when cpm is tuned |
//! | `/m/coil/cpm@a1b2…` | exactly that row, by digest | frozen forever |
//!
//! `physics_profiles` rows are immutable — the migration installs a trigger
//! that raises on `update` and `delete`, so tuning *inserts* — which is what
//! makes the second row of that table keep the first one's meaning. A pinned
//! digest with no row is [`ApiError::unknown_physics_digest`]: **not** empty and
//! **not** the current board. Substituting the current profile is the failure
//! §7.2 step 2 forbids the verifier from making, and this resolver does not get
//! to make it either.

use sqlx::{PgPool, Row};
use sqlx::postgres::PgRow;

use crate::digest16;
use crate::error::{ApiError, ApiResult};

/// A `maps` row.
#[derive(Debug, Clone)]
pub struct MapRow {
    /// `maps.id`.
    pub id: i32,
    /// The `<map>` in a URL.
    pub slug: String,
    /// Shown to players.
    pub name: String,
    /// Who made it, when the map says.
    pub author: Option<String>,
    /// SHA-256 of the `.map` text.
    pub source_sha256: Vec<u8>,
    /// Where the one origin serves the `.map` from.
    pub source_key: String,
    /// `straf3_map::CompiledMap::collision_digest` — the compiled hulls.
    pub collision_digest: u64,
    /// The compiler that produced them.
    pub map_compiler_version: String,
    /// Whether the clock can start.
    pub has_start_trigger: bool,
    /// Whether it can stop.
    pub has_finish_trigger: bool,
}

impl MapRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            slug: row.try_get("slug")?,
            name: row.try_get("name")?,
            author: row.try_get("author")?,
            source_sha256: row.try_get("source_sha256")?,
            source_key: row.try_get("source_key")?,
            collision_digest: digest16::from_sql(row.try_get("collision_digest")?),
            map_compiler_version: row.try_get("map_compiler_version")?,
            has_start_trigger: row.try_get("has_start_trigger")?,
            has_finish_trigger: row.try_get("has_finish_trigger")?,
        })
    }

    /// Whether a run on this map can produce a time at all.
    #[must_use]
    pub const fn has_timing(&self) -> bool {
        self.has_start_trigger && self.has_finish_trigger
    }
}

/// A `physics_profiles` row. Immutable, by database trigger.
#[derive(Debug, Clone)]
pub struct ProfileRow {
    /// `physics_profiles.id`.
    pub id: i32,
    /// The `<family>` in a URL: `vq3` or `cpm`.
    pub kind: String,
    /// Shown to players — `CPM (2026-08)`.
    pub label: String,
    /// `PhysicsProfile::digest()`. The `@digest16` in a pinned URL.
    pub digest: u64,
    /// Bumped when `PhysicsProfile` gains a field.
    pub layout_version: i16,
    /// When this set of constants was first seen.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ProfileRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            kind: row.try_get("kind")?,
            label: row.try_get("label")?,
            digest: digest16::from_sql(row.try_get("digest")?),
            layout_version: row.try_get("layout_version")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// A resolved (map, physics profile) pair, and whether the URL pinned it.
#[derive(Debug, Clone)]
pub struct Category {
    /// The map half.
    pub map: MapRow,
    /// The physics half.
    pub profile: ProfileRow,
    /// Whether the request named the digest explicitly. A pinned category is
    /// frozen; an unpinned one moves when the family is tuned.
    pub pinned: bool,
}

impl Category {
    /// The `category` object every board and run body carries.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "map": self.map.slug,
            "family": self.profile.kind,
            "digest": digest16::format(self.profile.digest),
            "label": self.profile.label,
            "pinned": self.pinned,
            // The key a link should use when it means *this* board rather than
            // "whatever cpm is today" (URLS.md §3).
            "key": format!("{}@{}", self.profile.kind, digest16::format(self.profile.digest)),
        })
    }
}

const MAP_COLUMNS: &str = "id, slug, name, author, source_sha256, source_key, collision_digest, \
                           map_compiler_version, has_start_trigger, has_finish_trigger";
const PROFILE_COLUMNS: &str = "id, kind, label, digest, layout_version, created_at";

/// Whether `slug` matches URLS.md §2's `<map>` grammar.
#[must_use]
pub fn is_valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let first_ok = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    first_ok
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// `<family>` per URLS.md §2.
#[must_use]
pub fn is_valid_family(family: &str) -> bool {
    !family.is_empty()
        && family.len() <= 16
        && family
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Split `<family>[@<digest16>]`.
///
/// # Errors
///
/// [`ApiError::invalid_category`] when the spelling is not the grammar — which
/// includes an uppercase digest, because URLS.md §2 makes that a refusal rather
/// than a redirect.
pub fn parse_category_key(spec: &str) -> ApiResult<(String, Option<u64>)> {
    let (family, pinned) = match spec.split_once('@') {
        None => (spec, None),
        Some((family, digest)) => {
            let parsed =
                digest16::parse(digest).ok_or_else(|| ApiError::invalid_category(spec))?;
            (family, Some(parsed))
        }
    };
    if !is_valid_family(family) {
        return Err(ApiError::invalid_category(spec));
    }
    Ok((family.to_string(), pinned))
}

/// Look a map up by slug.
///
/// # Errors
///
/// [`ApiError::unknown_map`] when there is no such row, including when the slug
/// is not even a legal one. A malformed slug cannot name a map, so answering
/// `404` is both true and cheaper than a round trip.
pub async fn resolve_map(pool: &PgPool, slug: &str) -> ApiResult<MapRow> {
    if !is_valid_slug(slug) {
        return Err(ApiError::unknown_map(slug));
    }
    let row = sqlx::query(&format!("select {MAP_COLUMNS} from maps where slug = $1"))
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    row.as_ref()
        .map(MapRow::from_row)
        .transpose()?
        .ok_or_else(|| ApiError::unknown_map(slug))
}

/// Every map, in slug order.
///
/// # Errors
///
/// Only a database failure — a database with no maps in it is an empty `Vec`,
/// which the caller renders as an empty list rather than as an error.
pub async fn all_maps(pool: &PgPool) -> ApiResult<Vec<MapRow>> {
    let rows = sqlx::query(&format!("select {MAP_COLUMNS} from maps order by slug"))
        .fetch_all(pool)
        .await?;
    rows.iter().map(MapRow::from_row).collect::<Result<_, _>>().map_err(Into::into)
}

/// The current profile of a family: the newest row of that kind.
///
/// # Errors
///
/// [`ApiError::unknown_physics_family`] when the family has no rows at all.
pub async fn current_profile(pool: &PgPool, family: &str) -> ApiResult<ProfileRow> {
    if !is_valid_family(family) {
        return Err(ApiError::unknown_physics_family(family));
    }
    let row = sqlx::query(&format!(
        "select {PROFILE_COLUMNS} from physics_profiles \
         where kind = $1 order by created_at desc, id desc limit 1"
    ))
    .bind(family)
    .fetch_optional(pool)
    .await?;
    row.as_ref()
        .map(ProfileRow::from_row)
        .transpose()?
        .ok_or_else(|| ApiError::unknown_physics_family(family))
}

/// Exactly the profile with this digest, and nothing else.
///
/// # Errors
///
/// [`ApiError::unknown_physics_digest`] when no row has it. Never the current
/// row, never an empty board — see the module docs.
pub async fn profile_by_digest(pool: &PgPool, digest: u64) -> ApiResult<ProfileRow> {
    let row = sqlx::query(&format!(
        "select {PROFILE_COLUMNS} from physics_profiles where digest = $1"
    ))
    .bind(digest16::to_sql(digest))
    .fetch_optional(pool)
    .await?;
    row.as_ref()
        .map(ProfileRow::from_row)
        .transpose()?
        .ok_or_else(|| ApiError::unknown_physics_digest(&digest16::format(digest)))
}

/// Every profile, newest first.
///
/// # Errors
///
/// Only a database failure.
pub async fn all_profiles(pool: &PgPool) -> ApiResult<Vec<ProfileRow>> {
    let rows = sqlx::query(&format!(
        "select {PROFILE_COLUMNS} from physics_profiles order by kind, created_at desc, id desc"
    ))
    .fetch_all(pool)
    .await?;
    rows.iter().map(ProfileRow::from_row).collect::<Result<_, _>>().map_err(Into::into)
}

/// Resolve a category key against a map.
///
/// `spec` is `None` for a request that did not name one, which resolves to the
/// map's default family (URLS.md §3).
///
/// # Errors
///
/// [`ApiError::unknown_map`], [`ApiError::invalid_category`],
/// [`ApiError::unknown_physics_family`], [`ApiError::unknown_physics_digest`] —
/// each of which the site renders differently, which is why they are four codes
/// and not one.
pub async fn resolve_category(
    pool: &PgPool,
    slug: &str,
    spec: Option<&str>,
) -> ApiResult<Category> {
    let map = resolve_map(pool, slug).await?;
    let spec = spec.unwrap_or(crate::profiles::DEFAULT_FAMILY);
    let (family, pinned_digest) = parse_category_key(spec)?;

    let profile = match pinned_digest {
        None => current_profile(pool, &family).await?,
        Some(digest) => {
            let row = profile_by_digest(pool, digest).await?;
            if row.kind != family {
                // The digest exists, but not in the family the URL named. Same
                // refusal, and the detail says which — the caller's link is
                // wrong in a specific way and can be fixed.
                return Err(ApiError {
                    detail: format!(
                        "physics digest `{}` belongs to family `{}`, not `{}`.",
                        digest16::format(digest),
                        row.kind,
                        family
                    ),
                    ..ApiError::unknown_physics_digest(&digest16::format(digest))
                });
            }
            row
        }
    };

    Ok(Category {
        map,
        profile,
        pinned: pinned_digest.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slug_grammar_is_urls_md_section_2() {
        assert!(is_valid_slug("coil"));
        assert!(is_valid_slug("training-crouch-slide"));
        assert!(is_valid_slug("9lives"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("Coil"), "identifiers in a URL are lowercase");
        assert!(!is_valid_slug("-leading-dash"));
        assert!(!is_valid_slug("has_underscore"));
        assert!(!is_valid_slug("has/slash"));
        assert!(!is_valid_slug(&"a".repeat(65)));
        assert!(is_valid_slug(&"a".repeat(64)));
    }

    #[test]
    fn a_category_key_is_family_optionally_at_digest() {
        assert_eq!(
            parse_category_key("cpm").unwrap(),
            ("cpm".to_string(), None)
        );
        assert_eq!(
            parse_category_key("cpm@0123456789abcdef").unwrap(),
            ("cpm".to_string(), Some(0x0123_4567_89ab_cdef))
        );
    }

    #[test]
    fn an_uppercase_pin_is_refused_rather_than_normalised() {
        // URLS.md §2 again: two spellings of one board is how a cache ends up
        // holding two copies of the same page.
        let err = parse_category_key("cpm@0123456789ABCDEF").unwrap_err();
        assert_eq!(err.code, "invalid_category");
    }

    #[test]
    fn a_malformed_key_is_a_refusal_and_not_a_family() {
        for spec in ["", "CPM", "cpm@", "cpm@short", "c p m", "cpm@0123456789abcdef@x"] {
            assert!(
                parse_category_key(spec).is_err(),
                "`{spec}` should not parse as a category key"
            );
        }
    }
}

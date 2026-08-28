//! `webcheck` — re-simulate a `.s3d` natively and compare the rolling digest,
//! command by command.
//!
//! # The question this answers
//!
//! Requirement r6: *a run recorded in the browser client re-simulates natively
//! to the same rolling digest.* That is one sentence and it hides two traps,
//! both of which this tool is shaped around.
//!
//! **Trap one: comparing the wrong number.** `docs/web/ARCHITECTURE.md` §0
//! item 4 records a measured run whose *final* state checksum matched across
//! builds while 29 of its 1 200 intermediate checksums did not. An end-state
//! comparison would have called that run identical. So the number compared
//! here is the rolling digest — folded over every command's state checksum, in
//! order, seeded at FNV-1a's offset basis — and where the file carries a
//! checksum trace, every intermediate checksum is compared as well and the
//! *count* of disagreements is reported, not just the first. The tool prints
//! the end-state comparison too, separately and labelled as insufficient, so a
//! reader can see the two verdicts differ when they differ.
//!
//! **Trap two: comparing a run against itself.** A recording carries the
//! digest its producer computed. Re-deriving that digest from the same file
//! with the same code proves the file is internally consistent and nothing
//! more. What makes this a cross-implementation check is that the state
//! checksums come from a *native* x86-64 build stepping `straf3-sim` against a
//! *natively* compiled `assets/maps/<map>.map`, while the digest in the header
//! was folded by a `wasm32-unknown-unknown` build in a browser stepping its
//! own compilation of the same source. Two implementations, one number. The
//! header identifies the map by `collision_digest`, so a native compile that
//! produced different geometry is refused before anything is simulated rather
//! than showing up as a movement divergence.
//!
//! `--expect-digest` closes the remaining gap: the browser reports its run
//! digest to the page through `onRunFinished`, out of band from the file. Pass
//! that value and the tool checks the file's header agrees with what the
//! browser said, so a header written by a code path that never ran the
//! simulation cannot pass.
//!
//! # Commands
//!
//! ```text
//! webcheck resim <run.s3d> [--maps <dir>] [--expect-digest <hex16>]
//! webcheck from-text <fixture.txt> --map <file.map> --out <run.s3d> [--name <slug>]
//! ```
//!
//! `resim` is the r6 check. `from-text` converts a committed text recording
//! into a `.s3d` so the harness has a native subject with a known answer —
//! see `fixture.rs`.
//!
//! Exit status is 0 only when every comparison agreed. Any disagreement, any
//! refused binding and any unreadable input is a non-zero exit, so this is
//! usable as a gate and not only as a report.

mod fixture;
mod lockstep;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use straf3_map::CompiledMap;
use straf3_replay::{Recording, VerifyError, WorldId, physics_digest};
use straf3_sim::{PhysicsProfile, SimState};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("resim") => resim(&args[1..]),
        Some("from-text") => from_text(&args[1..]),
        Some("physics") => physics(),
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("webcheck: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
webcheck — re-simulate a .s3d natively and compare the rolling digest.

    webcheck resim <run.s3d> [options]
        --maps <dir>            where to find <map>.map        [assets/maps]
        --lock <file>           the workspace Cargo.lock        [Cargo.lock]
        --expect-digest <hex>   the run digest the browser reported out of
                                band, as 16 hex digits with or without `0x`

    webcheck from-text <fixture.txt> --map <file.map> --out <run.s3d> [--name <slug>]
        Convert a text recording into a .s3d, carrying the checksum trace.
        --name defaults to the .map file's stem.

    webcheck physics
        Print PhysicsProfile::digest() for every named profile in this tree.
        For r12: run it before and after and diff, rather than trusting a
        number pasted into a document.

Exit status is 0 only when every comparison agreed.";

// ── physics: the r12 digests, derived rather than quoted ────────────────────

/// Every named profile's digest, printed by running the code that defines it.
///
/// r12 asks that the physics digest has not moved. A number transcribed into a
/// document cannot answer that — it can only be compared against, and whoever
/// compares has to trust the transcription. So this prints the digests fresh,
/// and the check is `diff` between two runs of one command.
///
/// The list is written out by name and not derived, because there is no
/// registry of profiles to iterate: a profile added to `straf3-sim` without a
/// line here would go unnoticed, which is the one failure worth guarding, and
/// `crates/straf3-sim/src/profile.rs` is the file to check against.
fn physics() -> Result<(), String> {
    println!("PhysicsProfile::digest(), derived from this tree");
    for (name, profile) in [
        ("cpm", PhysicsProfile::cpm()),
        ("vq3", PhysicsProfile::vq3()),
        ("experimental", PhysicsProfile::experimental()),
        ("default", PhysicsProfile::default()),
    ] {
        println!("  {name:<16}{:#018x}", physics_digest(&profile));
    }
    println!();
    println!(
        "The digest folds the bits of every field of PhysicsProfile with no `..`,\n\
         so a new movement constant is a new input to it. If any line above moved\n\
         during a wave that did not intend to change physics, that is the finding."
    );
    Ok(())
}

// ── resim: the r6 check ─────────────────────────────────────────────────────

fn resim(args: &[String]) -> Result<(), String> {
    let mut path: Option<PathBuf> = None;
    let mut maps = PathBuf::from("assets/maps");
    let mut lock = PathBuf::from("Cargo.lock");
    let mut expect: Option<u64> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--maps" => maps = PathBuf::from(value(args, &mut i, "--maps")?),
            "--lock" => lock = PathBuf::from(value(args, &mut i, "--lock")?),
            "--expect-digest" => {
                expect = Some(hex64(&value(args, &mut i, "--expect-digest")?)?);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option `{other}`\n\n{USAGE}"));
            }
            other => path = Some(PathBuf::from(other)),
        }
        i += 1;
    }
    let path = path.ok_or_else(|| format!("resim needs a .s3d path\n\n{USAGE}"))?;

    let mut out = Report::new();
    out.title("straf3-webcheck — native re-simulation of a recorded run");
    out.blank();

    // ── is this harness verifying the tree it says it is ────────────────────
    let own_lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    let steps = lockstep::compare(&lock, &own_lock)?;
    out.section("harness");
    out.field("tool", "tools/straf3-webcheck (standalone crate)");
    out.field(
        "shared packages",
        &format!("{} resolved in both lock files", steps.shared),
    );
    if steps.agrees() {
        out.field("lockstep", "every shared package is at the same version");
    } else {
        for (name, theirs, ours) in &steps.conflicts {
            out.field(
                "LOCKSTEP BREAK",
                &format!("{name}: workspace {theirs}, harness {ours}"),
            );
        }
        return Err(format!(
            "{out}\n\
             the harness resolved {} package(s) differently from the workspace, so a \
             digest it computes is not a statement about the shipped tree. Re-run \
             `cargo update` in tools/straf3-webcheck against the workspace versions \
             before believing any verdict.",
            steps.conflicts.len()
        ));
    }
    out.blank();

    // ── what the file says ──────────────────────────────────────────────────
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    // `from_bytes` checks the content digest before it believes any length
    // field, so a truncated file fails here rather than parsing into a
    // plausible recording with half its commands missing.
    let rec = Recording::from_bytes(&bytes)
        .map_err(|e| format!("{} is not a loadable .s3d: {e}", path.display()))?;

    let claimed = rec.claimed();
    let trace = rec.trace().map(<[u64]>::to_vec);

    out.section("the recording, as it claims to be");
    out.field("file", &path.display().to_string());
    out.field("bytes", &bytes.len().to_string());
    out.field("content digest", "verified at load");
    out.field(
        "checksum trace",
        &match &trace {
            Some(t) => format!("present, {} entries", t.len()),
            None => "absent — a divergence can be detected but not localised".to_string(),
        },
    );
    out.field("command rate", &format!("{} Hz", rec.start().rate.hz()));
    out.field("commands", &rec.command_count().to_string());
    let (map_name, recorded_collision) = match rec.world() {
        WorldId::Map {
            name,
            collision_digest,
        } => (name.clone(), *collision_digest),
        other => {
            return Err(format!(
                "this recording was made in {other:?}, not in a compiled map. \
                 r6 is about a run in a map: a flat-world recording cannot show \
                 that two builds compiled the same geometry."
            ));
        }
    };
    out.field("map", &map_name);
    out.hex("recorded collision digest", recorded_collision);
    out.field("physics", &rec.physics().name);
    out.hex("recorded physics digest", rec.physics().digest);
    out.field("claimed sim time", &format!("{} ms", claimed.sim_time_ms));
    out.field(
        "claimed run time",
        &match claimed.run_time_ms {
            Some(ms) => format!("{ms} ms"),
            None => "unfinished — no time".to_string(),
        },
    );
    out.hex("claimed rolling digest", claimed.digest);
    out.blank();

    // ── what the browser said out of band ───────────────────────────────────
    if let Some(expect) = expect {
        out.section("cross-check against what the browser reported");
        out.hex("browser onRunFinished digest", expect);
        if expect == claimed.digest {
            out.field("agreement", "the file's header carries the same number");
        } else {
            out.field("AGREEMENT", "NO — the file disagrees with the browser");
            return Err(format!(
                "{out}\n\
                 the recording's header claims {:#018x} but the browser reported \
                 {expect:#018x} for the same run. The file was not written by the \
                 simulation that produced that number.",
                claimed.digest
            ));
        }
        out.blank();
    }

    // ── the native world ────────────────────────────────────────────────────
    let map_path = maps.join(format!("{map_name}.map"));
    let source = std::fs::read_to_string(&map_path)
        .map_err(|e| format!("cannot read {}: {e}", map_path.display()))?;
    let compiled: CompiledMap = straf3_map::compile(&source)
        .map_err(|e| format!("{} does not compile: {e}", map_path.display()))?;
    let native_collision = compiled.collision_digest();
    let world = compiled.collider();
    let world_id = WorldId::map(map_name.clone(), native_collision);

    let profile: PhysicsProfile = fixture::profile_named(&rec.physics().name).ok_or_else(|| {
        format!(
            "this build has no physics profile named `{}`. It cannot re-simulate a \
             run recorded under constants it does not have — that is a refusal, not \
             a divergence.",
            rec.physics().name
        )
    })?;
    let native_physics = physics_digest(&profile);

    out.section("the native world this host compiled");
    out.field("source", &map_path.display().to_string());
    out.field("source bytes", &source.len().to_string());
    out.hex("compiled collision digest", native_collision);
    out.verdict(
        "same geometry",
        native_collision == recorded_collision,
        "the browser and this host compiled the same hulls and triggers",
        "DIFFERENT — the two builds do not agree on the map's geometry",
    );
    out.hex("native physics digest", native_physics);
    out.verdict(
        "same physics",
        native_physics == rec.physics().digest,
        "the same movement constants, field for field",
        "DIFFERENT — the movement constants moved since the run was recorded",
    );
    out.blank();

    if native_collision != recorded_collision || native_physics != rec.physics().digest {
        return Err(format!(
            "{out}\n\
             the binding does not hold, so nothing was simulated. A digest \
             comparison across two different worlds would mean nothing: the run \
             would diverge because the geometry differs, not because the two \
             builds disagree."
        ));
    }

    // ── re-simulate ─────────────────────────────────────────────────────────
    //
    // Two passes over the same command stream, on purpose.
    //
    // `verify` is the crate's own definition of the check and returns the
    // verdict r6 turns on. The `replay` pass below re-derives the same rolling
    // digest from the states it observes, so the number reported here is not
    // simply read back out of the value being tested, and it collects the
    // per-command comparison `verify` reduces to a single index.
    let verdict = rec.verify(&world, &world_id, &profile);

    let mut native_trace = Vec::with_capacity(rec.command_count());
    let replayed = rec
        .replay(&world, &world_id, &profile, |_, state: &SimState| {
            native_trace.push(state.checksum());
        })
        .map_err(|e| format!("replay refused: {e}"))?;

    out.section("re-simulation, natively, on this host");
    out.field("commands stepped", &native_trace.len().to_string());
    out.hex("native rolling digest", replayed.digest);
    out.field("native sim time", &format!("{} ms", replayed.sim_time_ms));
    out.field(
        "native run time",
        &match replayed.run_time_ms {
            Some(ms) => format!("{ms} ms"),
            None => "unfinished — no time".to_string(),
        },
    );
    out.blank();

    out.section("the comparison r6 is about");
    let rolling_agrees = replayed.digest == claimed.digest;
    out.verdict(
        "rolling digest",
        rolling_agrees,
        "AGREE — folded over every command, in order",
        "DISAGREE — the two builds did not simulate the same run",
    );

    match &trace {
        Some(recorded) => {
            let compared = recorded.len().min(native_trace.len());
            let disagreements = recorded
                .iter()
                .zip(&native_trace)
                .filter(|(a, b)| a != b)
                .count();
            let first = recorded
                .iter()
                .zip(&native_trace)
                .position(|(a, b)| a != b);
            out.field(
                "intermediate checksums",
                &format!("{compared} compared, {disagreements} disagree"),
            );
            out.field(
                "first divergence",
                &match first {
                    Some(i) => format!("command {i}"),
                    None => "none".to_string(),
                },
            );
            // The comparison ARCHITECTURE §0 item 4 shows to be insufficient,
            // printed beside the one that is not, so the difference is visible
            // rather than argued.
            let end_agrees = recorded.last() == native_trace.last();
            out.field(
                "end-state checksum only",
                &format!(
                    "{} — insufficient on its own (ARCHITECTURE §0 item 4)",
                    if end_agrees { "agrees" } else { "disagrees" }
                ),
            );
            if end_agrees && disagreements > 0 {
                out.field(
                    "NOTE",
                    "the end state agrees while intermediate states do not. This is \
                     exactly the failure an end-state comparison cannot see.",
                );
            }
        }
        None => {
            out.field(
                "intermediate checksums",
                "not compared — this file carries no trace",
            );
            out.field(
                "note",
                "the rolling fold is sticky, so digest equality still implies every \
                 command agreed; only the diagnosis of a disagreement is lost. Record \
                 evidence runs with `to_bytes_with_checksums`.",
            );
        }
    }
    out.blank();

    match verdict {
        Ok(outcome) => {
            out.section("verdict");
            out.field(
                "result",
                "AGREE — the recorded run re-simulates natively to the same rolling digest",
            );
            out.hex("digest", outcome.digest);
            out.field(
                "over",
                &format!("{} commands, {} ms of simulation", rec.command_count(), outcome.sim_time_ms),
            );
            println!("{out}");
            Ok(())
        }
        Err(VerifyError::Diverged {
            claimed,
            actual,
            first_diverging_command,
        }) => {
            out.section("verdict");
            out.field("result", "DIVERGE");
            out.hex("claimed digest", claimed.digest);
            out.hex("native digest", actual.digest);
            out.field(
                "first diverging command",
                &match first_diverging_command {
                    Some(i) => i.to_string(),
                    None => "unknown — the file carries no checksum trace".to_string(),
                },
            );
            out.field(
                "claimed times",
                &format!("sim {} ms, run {:?} ms", claimed.sim_time_ms, claimed.run_time_ms),
            );
            out.field(
                "native times",
                &format!("sim {} ms, run {:?} ms", actual.sim_time_ms, actual.run_time_ms),
            );
            Err(format!(
                "{out}\n\
                 the diverging command index above is the finding. Report it rather \
                 than adjusting anything to make it go away."
            ))
        }
        Err(VerifyError::Mismatch(m)) => Err(format!("{out}\nbinding refused: {m}")),
    }
}

// ── from-text: give the harness a native subject ────────────────────────────

fn from_text(args: &[String]) -> Result<(), String> {
    let mut input: Option<PathBuf> = None;
    let mut map: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut name: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--map" => map = Some(PathBuf::from(value(args, &mut i, "--map")?)),
            "--out" => out_path = Some(PathBuf::from(value(args, &mut i, "--out")?)),
            "--name" => name = Some(value(args, &mut i, "--name")?),
            other if other.starts_with('-') => {
                return Err(format!("unknown option `{other}`\n\n{USAGE}"));
            }
            other => input = Some(PathBuf::from(other)),
        }
        i += 1;
    }
    let input = input.ok_or("from-text needs a fixture path")?;
    let map = map.ok_or("from-text needs --map")?;
    let out_path = out_path.ok_or("from-text needs --out")?;
    let name = name.unwrap_or_else(|| {
        map.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unnamed".to_string())
    });

    let text = std::fs::read_to_string(&input)
        .map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let f = fixture::parse(&text).map_err(|e| format!("{}: {e}", input.display()))?;
    if f.world != fixture::World::Map {
        return Err(format!(
            "{} declares {:?}, and this command only converts map recordings",
            input.display(),
            f.world
        ));
    }

    let source = std::fs::read_to_string(&map)
        .map_err(|e| format!("cannot read {}: {e}", map.display()))?;
    let compiled = straf3_map::compile(&source)
        .map_err(|e| format!("{} does not compile: {e}", map.display()))?;

    let rec = Recording::record(
        straf3_replay::RunStart {
            rate: f.rate,
            spawn: f.spawn,
            yaw: f.yaw,
        },
        f.commands,
        &compiled.collider(),
        WorldId::map(name.clone(), compiled.collision_digest()),
        &f.profile,
        f.profile_name.clone(),
    );

    // With the trace, not without it. This file exists to be evidence, and
    // `to_bytes` would drop exactly the per-command detail that lets a
    // disagreement be localised rather than merely noticed.
    let bytes = rec
        .to_bytes_with_checksums()
        .ok_or("the recording carries no checksum trace")?;
    if let Some(dir) = out_path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(&out_path, &bytes)
        .map_err(|e| format!("cannot write {}: {e}", out_path.display()))?;

    let claimed = rec.claimed();
    println!("wrote {} ({} bytes)", out_path.display(), bytes.len());
    println!("  map                {name}");
    println!("  collision digest   {:#018x}", compiled.collision_digest());
    println!("  physics            {}", f.profile_name);
    println!("  commands           {}", rec.command_count());
    println!("  sim time           {} ms", claimed.sim_time_ms);
    println!(
        "  run time           {}",
        match claimed.run_time_ms {
            Some(ms) => format!("{ms} ms"),
            None => "unfinished".to_string(),
        }
    );
    println!("  rolling digest     {:#018x}", claimed.digest);
    Ok(())
}

// ── argument plumbing ───────────────────────────────────────────────────────

fn value(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Parse 16 hex digits, with or without a `0x` prefix.
fn hex64(text: &str) -> Result<u64, String> {
    let t = text.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(t, 16).map_err(|_| format!("`{text}` is not a 64-bit hex digest"))
}

// ── the report ──────────────────────────────────────────────────────────────

/// The artefact this tool exists to produce.
///
/// Accumulated rather than printed as it goes, because a failure has to be
/// able to print everything gathered so far *and* exit non-zero — a check that
/// prints its findings on success and a bare error on failure hides the case
/// worth reading.
struct Report(String);

impl Report {
    fn new() -> Self {
        Self(String::new())
    }
    fn title(&mut self, t: &str) {
        self.0.push_str(t);
        self.0.push('\n');
        self.0.push_str(&"=".repeat(t.len()));
        self.0.push('\n');
    }
    fn section(&mut self, t: &str) {
        self.0.push_str(&format!("[{t}]\n"));
    }
    fn field(&mut self, k: &str, v: &str) {
        self.0.push_str(&format!("  {k:<30}{v}\n"));
    }
    fn hex(&mut self, k: &str, v: u64) {
        self.field(k, &format!("{v:#018x}"));
    }
    fn verdict(&mut self, k: &str, ok: bool, yes: &str, no: &str) {
        self.field(k, if ok { yes } else { no });
    }
    fn blank(&mut self) {
        self.0.push('\n');
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_is_read_with_or_without_the_prefix() {
        assert_eq!(hex64("0x0000000000000001").unwrap(), 1);
        assert_eq!(hex64("0000000000000001").unwrap(), 1);
        assert_eq!(hex64(" 0XFF ").unwrap(), 255);
        assert!(hex64("not a digest").is_err());
    }
}

//! Criterion 7's cross-target check: the same saved run, on all four targets.
//!
//! # What this proves that criterion 2 does not
//!
//! `cargo xtask determinism` proves that a *reference command stream compiled
//! into the binary* produces the same bits on `x86_64-unknown-linux-gnu`,
//! `x86_64-unknown-linux-musl`, `x86_64-pc-windows-gnu` and
//! `wasm32-unknown-unknown`. That is Proof A and it is closed.
//!
//! It says nothing about a run that arrived as *bytes*. Between a run and its
//! replay on another machine sits an encoder, a decoder, a length field, a
//! string, twenty-two `f32` bit patterns and a `usize` that is 64 bits on
//! three of those targets and 32 on the fourth. Every one of them is a place
//! where the same run can become a different file, or the same file a
//! different run. So this module runs the whole path — record, encode, decode,
//! re-simulate — inside each target and publishes every number that must
//! agree:
//!
//! - the encoded file's **length** and **content digest**, in both the compact
//!   and the with-checksums form, which is byte-identity of the format itself;
//! - the **run digest** and the **final time** from re-simulating the *decoded*
//!   recording, which is byte-identity of the simulation the format carries;
//! - every **per-command checksum**, so a disagreement names the first command
//!   it happened on rather than only that it happened.
//!
//! # There is no golden value here either
//!
//! Same rule as `straf3-det-runner`: the check is *relative*. Nothing in this
//! file stores an expected digest, so C3's angles, a movement fix, or a
//! deliberate change to the layout re-numbers everything and requires no
//! fixture to be re-recorded. What is compared is four targets against each
//! other.
//!
//! Run it with `crosstarget/verify.sh`.

use core::fmt::Write as _;
use std::sync::OnceLock;

use straf3_sim::num::{s, vec3};
use straf3_sim::world::FlatGround;
use straf3_sim::{Buttons, PhysicsProfile, TickRate, UserCmd, angle_to_short};

use crate::codec::FORMAT_VERSION;
use crate::digest::{FNV_OFFSET, fold};
use crate::identity::WorldId;
use crate::recording::{Recording, RunStart};

/// Bumped when the report text changes in a way a reader must notice.
pub const REPORT_VERSION: u32 = 1;

/// Commands per case.
///
/// Shorter than `straf3-det-runner`'s 1 200 on purpose: this check is not
/// hunting for a divergence in the *physics* — criterion 2 already does that,
/// over a longer stream, and would find one first. It is checking that the
/// format carries a run intact, and 400 commands is 3.2 s of movement, long
/// enough that the trajectory is well away from the spawn and every field in a
/// command has taken many different values.
pub const STEPS: u32 = 400;

/// The triple this was compiled for, as the report states it. Reported so the
/// driver can verify the artefact it just ran is the one it asked for.
pub const TARGET: &str = target_triple();

const fn target_triple() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "wasm32-unknown-unknown"
    } else if cfg!(all(target_os = "linux", target_env = "musl")) {
        "x86_64-unknown-linux-musl"
    } else if cfg!(all(target_os = "linux", target_env = "gnu")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "windows", target_env = "gnu")) {
        "x86_64-pc-windows-gnu"
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else {
        "unknown"
    }
}

/// One case: a physics profile, a rate, an input program, and the world
/// identity the recording declares.
struct Case {
    name: &'static str,
    hz: u32,
    vq3: bool,
    /// Degrees of yaw added per command. A different angle every command is
    /// what makes a 1-ULP disagreement in `sin_cos` reachable at all.
    yaw_step: f32,
    /// How the recording identifies its world.
    ///
    /// Every case simulates against [`FlatGround`], because this crate depends
    /// on `straf3-sim` alone and has no compiled map to sweep. The `Map` case
    /// is therefore declaring an identity for a world it is not really running
    /// in, which would be a bug anywhere but here — its job is to put a
    /// variable-length UTF-8 name and a 64-bit collision digest through the
    /// header on a 32-bit target and a 64-bit one, which is the part of the
    /// map binding that can differ between them. Whether a `.map` compiles to
    /// the same hulls everywhere is `straf3-map`'s own already-verified
    /// property, not this file's to re-prove.
    world: fn() -> WorldId,
}

const CASES: &[Case] = &[
    Case {
        name: "cpm-flat-125hz",
        hz: 125,
        vq3: false,
        yaw_step: 0.37,
        world: || WorldId::flat(s(0.0)),
    },
    Case {
        name: "vq3-flat-76hz",
        hz: 76,
        vq3: true,
        yaw_step: 0.013,
        world: || WorldId::flat(s(0.0)),
    },
    Case {
        // Exercises the variable-length header: a UTF-8 map name and a 64-bit
        // collision digest. The name is deliberately not ASCII-only — a
        // length field counted in `char`s instead of bytes would pass every
        // ASCII test and truncate here.
        name: "map-binding-250hz",
        hz: 250,
        vq3: false,
        yaw_step: 1.5,
        world: || WorldId::map("coil-π-ø", 0x0123_4567_89ab_cdef),
    },
];

fn spawn() -> RunStart {
    RunStart {
        rate: TickRate::HZ_125,
        spawn: vec3(s(0.0), s(0.0), s(64.0)),
        yaw: s(0.0),
    }
}

/// Build command `i` of a case: a pure function of the index and the ground
/// state, so no target can produce a different command stream to run.
fn cmd_for(case: &Case, i: u32, grounded: bool) -> UserCmd {
    let rate = TickRate::from_hz(case.hz).expect("case rate is in 1..=1000");
    let mut cmd = UserCmd::still_at(rate);

    // Quantised at the command boundary, exactly where a real producer
    // quantises (C3). The wrap in `angle_to_short` is what keeps a stepping
    // yaw inside one turn.
    cmd.view.yaw = angle_to_short(s(i as f32) * s(case.yaw_step));
    // A saw-tooth pitch, so the first of `angle_vectors`' three `sin_cos`
    // calls sees a varying argument too.
    cmd.view.pitch = angle_to_short(s(-45.0) + s((i % 180) as f32) * s(0.5));

    cmd.forward_move = 127;
    cmd.right_move = if (i / 12).is_multiple_of(2) {
        127
    } else {
        -127
    };
    if grounded {
        cmd.buttons = Buttons::JUMP;
    }
    // Crouch for a stretch in the middle, so the crouched hull and the
    // duck-scale branch are on the recorded path and `up_move` is not
    // always zero.
    if (100..140).contains(&i) {
        cmd.buttons = cmd.buttons.with(Buttons::CROUCH);
        cmd.up_move = -127;
    }
    cmd
}

fn profile_of(case: &Case) -> (PhysicsProfile, &'static str) {
    if case.vq3 {
        (PhysicsProfile::vq3(), "vq3")
    } else {
        (PhysicsProfile::cpm(), "cpm")
    }
}

/// Everything one case produced. Every field is a number four targets must
/// agree on.
pub struct CaseResult {
    /// Case name, so a report cannot be compared against a different case.
    pub name: &'static str,
    /// How many commands were recorded.
    pub commands: u32,
    /// The recorded command rate, in hertz.
    pub hz: u32,
    /// Length in bytes of the compact encoding.
    pub compact_len: u64,
    /// Content digest of the compact encoding.
    pub compact_content: u64,
    /// Length in bytes of the encoding that carries the checksum trace.
    pub traced_len: u64,
    /// Content digest of that encoding.
    pub traced_content: u64,
    /// Whether decoding the bytes and re-encoding them produced the same
    /// bytes, in both forms — the round trip, checked on the target rather
    /// than asserted at home.
    pub round_trips: bool,
    /// Whether the decoded recording, re-simulated, reproduced what it claims.
    pub verifies: bool,
    /// Whether a deliberately altered world identity was refused.
    pub refuses_stale_geometry: bool,
    /// The run digest, from re-simulating the **decoded** recording.
    pub digest: u64,
    /// Simulation time after the last command, in milliseconds.
    pub sim_time_ms: u32,
    /// The run's time, if it finished. `None` in every case here: `FlatGround`
    /// has no trigger volumes, so no clock starts. Published anyway, because
    /// the day a case runs on a real course this is the number criterion 7
    /// says must match.
    pub run_time_ms: Option<u32>,
    /// Every per-command state checksum, in order. What names the first
    /// diverging command.
    pub checksums: Vec<u64>,
}

fn run_case(case: &Case) -> CaseResult {
    let world = FlatGround::at(s(0.0));
    let (profile, profile_name) = profile_of(case);
    let world_id = (case.world)();

    let start = spawn();
    let mut state = start.state();
    let mut commands = Vec::with_capacity(STEPS as usize);
    for i in 0..STEPS {
        let grounded = state.player.ground.is_grounded();
        let cmd = cmd_for(case, i, grounded);
        straf3_sim::step_in_place(&mut state, &cmd, &world, &profile);
        commands.push(cmd);
    }

    let recorded = Recording::record(
        start,
        commands,
        &world,
        world_id.clone(),
        &profile,
        profile_name,
    );

    let compact = recorded.to_bytes();
    let traced = recorded
        .to_bytes_with_checksums()
        .expect("a freshly recorded run always carries its trace");

    // Decode both, and re-encode: the round trip is checked on the target, in
    // the direction that matters. Bytes -> Recording -> bytes must be the
    // identity, or two machines can hold "the same" recording and write
    // different files.
    let from_compact = Recording::from_bytes(&compact);
    let from_traced = Recording::from_bytes(&traced);
    let round_trips = match (&from_compact, &from_traced) {
        (Ok(a), Ok(b)) => {
            a.to_bytes() == compact
                && b.to_bytes_with_checksums().as_deref() == Some(traced.as_slice())
                // A recording loaded from the traced form must also write the
                // compact form byte-identically to one recorded fresh.
                && b.to_bytes() == compact
                && a.commands_unchecked() == recorded.commands_unchecked()
                && b.trace() == recorded.trace()
        }
        _ => false,
    };

    // Re-simulate the *decoded* recording, not the one still in memory. The
    // number this report publishes has to have gone through the file.
    let decoded = from_traced.expect("the traced encoding decodes");
    let verified = decoded.verify(&world, &world_id, &profile);
    let outcome = match &verified {
        Ok(o) => *o,
        Err(_) => decoded.claimed(),
    };

    // And check the refusal, on the target, rather than only in a unit test at
    // home: a recompiled map must not replay.
    let stale = match &world_id {
        WorldId::Map {
            name,
            collision_digest,
        } => WorldId::map(name.clone(), collision_digest ^ 1),
        WorldId::Flat { height_bits } => WorldId::Flat {
            height_bits: height_bits ^ 1,
        },
        WorldId::Empty => WorldId::flat(s(0.0)),
    };
    let refuses_stale_geometry = decoded.commands_for(&stale, &profile).is_err();

    CaseResult {
        name: case.name,
        commands: STEPS,
        hz: case.hz,
        compact_len: compact.len() as u64,
        compact_content: content_digest(&compact),
        traced_len: traced.len() as u64,
        traced_content: content_digest(&traced),
        round_trips,
        verifies: verified.is_ok(),
        refuses_stale_geometry,
        digest: outcome.digest,
        sim_time_ms: outcome.sim_time_ms,
        run_time_ms: outcome.run_time_ms,
        checksums: decoded.trace().unwrap_or_default().to_vec(),
    }
}

/// The trailing eight bytes of an encoded file: the content digest the encoder
/// wrote. Read back out rather than recomputed, so the report publishes what
/// is actually in the bytes.
fn content_digest(bytes: &[u8]) -> u64 {
    let tail = &bytes[bytes.len() - 8..];
    u64::from_le_bytes([
        tail[0], tail[1], tail[2], tail[3], tail[4], tail[5], tail[6], tail[7],
    ])
}

/// Every case, computed once.
pub fn results() -> &'static Vec<CaseResult> {
    static CACHE: OnceLock<Vec<CaseResult>> = OnceLock::new();
    CACHE.get_or_init(|| CASES.iter().map(run_case).collect())
}

/// One number folded over every case's numbers — a log line, never compared on
/// its own.
#[must_use]
pub fn grand_digest() -> u64 {
    results().iter().fold(FNV_OFFSET, |acc, c| {
        let acc = fold(acc, c.digest);
        let acc = fold(acc, c.compact_content);
        let acc = fold(acc, c.traced_content);
        let acc = fold(acc, c.compact_len);
        let acc = fold(acc, c.traced_len);
        fold(acc, u64::from(c.sim_time_ms))
    })
}

/// Whether every case passed its own on-target assertions.
#[must_use]
pub fn all_ok() -> bool {
    results()
        .iter()
        .all(|c| c.round_trips && c.verifies && c.refuses_stale_geometry)
}

/// The report text. Line-oriented on purpose: diffing two of these by hand is
/// a supported way to debug a failure.
#[must_use]
pub fn render(platform: &str) -> String {
    let cases = results();
    let mut o = String::with_capacity(CASES.len() * STEPS as usize * 17 + 4096);

    let _ = writeln!(o, "straf3-s3d-report {REPORT_VERSION}");
    let _ = writeln!(o, "target {TARGET}");
    let _ = writeln!(o, "platform {platform}");
    let _ = writeln!(o, "format-version {FORMAT_VERSION}");
    let _ = writeln!(o, "cases {}", CASES.len());
    let _ = writeln!(o, "steps {STEPS}");
    let _ = writeln!(o, "grand {:016x}", grand_digest());
    let _ = writeln!(o, "all-ok {}", all_ok());

    for (i, c) in cases.iter().enumerate() {
        let _ = writeln!(
            o,
            "case {i} {} hz {} commands {} digest {:016x} sim_time_ms {} run_time_ms {} \
             compact_len {} compact_content {:016x} traced_len {} traced_content {:016x} \
             round_trips {} verifies {} refuses_stale {}",
            c.name,
            c.hz,
            c.commands,
            c.digest,
            c.sim_time_ms,
            match c.run_time_ms {
                Some(ms) => ms.to_string(),
                None => "-".to_owned(),
            },
            c.compact_len,
            c.compact_content,
            c.traced_len,
            c.traced_content,
            c.round_trips,
            c.verifies,
            c.refuses_stale_geometry,
        );
    }
    for (i, c) in cases.iter().enumerate() {
        let _ = write!(o, "checksums {i} ");
        for (n, checksum) in c.checksums.iter().enumerate() {
            if n > 0 {
                o.push(',');
            }
            let _ = write!(o, "{checksum:016x}");
        }
        o.push('\n');
    }
    o
}

/// The rendered report, computed once and kept alive for the wasm exports.
#[cfg(target_arch = "wasm32")]
fn cached_report() -> &'static String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| render(TARGET))
}

// ── the wasm surface ────────────────────────────────────────────────────────
//
// Three exports through the raw C ABI, no wasm-bindgen: the host reads the
// *already rendered* report straight out of linear memory. Modelled on
// `straf3-det-runner`'s, for its reason — doing it this way means the
// JavaScript runner cannot format the report differently from the native
// binary, because it does no formatting at all.

/// Byte offset of the rendered report in linear memory.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn s3d_report_ptr() -> u32 {
    cached_report().as_ptr() as u32
}

/// Length of the rendered report in bytes.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn s3d_report_len() -> u32 {
    cached_report().len() as u32
}

/// The report format version, so the runner can refuse a stale `.wasm`.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn s3d_report_version() -> u32 {
    REPORT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::Scalar;

    #[test]
    fn every_case_passes_its_own_assertions() {
        for c in results() {
            assert!(c.round_trips, "{} did not round-trip", c.name);
            assert!(c.verifies, "{} did not re-simulate to its claim", c.name);
            assert!(
                c.refuses_stale_geometry,
                "{} accepted a world it was not recorded in",
                c.name
            );
        }
        assert!(all_ok());
    }

    #[test]
    fn the_stream_actually_moves_the_player() {
        // Guards against every target agreeing on the same nothing.
        let c = &results()[0];
        let distinct: std::collections::BTreeSet<_> = c.checksums.iter().collect();
        assert_eq!(c.checksums.len(), STEPS as usize);
        assert!(
            distinct.len() > STEPS as usize / 2,
            "only {} distinct states in {STEPS} commands",
            distinct.len()
        );
    }

    #[test]
    fn the_cases_are_not_the_same_run_three_times() {
        let digests: std::collections::BTreeSet<u64> = results().iter().map(|c| c.digest).collect();
        assert_eq!(digests.len(), CASES.len());
    }

    #[test]
    fn the_report_has_the_shape_the_driver_parses() {
        let text = render("test");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], format!("straf3-s3d-report {REPORT_VERSION}"));
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("case ")).count(),
            CASES.len()
        );
        assert!(lines.contains(&"all-ok true"));
        for line in lines.iter().filter(|l| l.starts_with("checksums ")) {
            let list = line.split(' ').nth(2).expect("checksum list");
            assert_eq!(list.split(',').count(), STEPS as usize);
        }
    }

    #[test]
    fn a_case_name_travels_with_its_numbers() {
        // So a driver cannot compare case 0 of one target against a
        // differently ordered case 0 of another.
        for (i, c) in results().iter().enumerate() {
            assert!(render("test").contains(&format!("case {i} {}", c.name)));
        }
    }

    #[test]
    fn the_scalar_type_is_what_the_format_assumes() {
        // Every `f32` in the file travels as four bytes. If `Scalar` widens,
        // the layout changes and the version must be bumped.
        assert_eq!(core::mem::size_of::<Scalar>(), 4);
    }
}

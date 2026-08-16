//! The cross-target determinism reference stream (architecture item C2).
//!
//! This crate is compiled once for each target the project ships or verifies
//! on — `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
//! `x86_64-pc-windows-gnu` and `wasm32-unknown-unknown` — runs one fixed set
//! of command streams through `straf3-sim`, and prints a report. `cargo xtask
//! determinism` builds it, runs it, and fails if the reports disagree.
//!
//! # Why a rolling digest and not a final checksum
//!
//! The `probes/wasm-determinism` probe ran six cases of 1 200 commands across
//! four builds. In the `cpm-fine-turn` case, **29 of the 1 200 intermediate
//! per-command checksums differed while the final checksum matched** — the
//! trajectories diverged and happened to re-converge on the last command. A
//! check that compared end states would have certified that run as identical.
//!
//! So the number this crate publishes per case is a digest **folded over
//! every command's state checksum in order**. It is sticky: once two targets
//! disagree on any command, their digests differ for every command after it,
//! and can never re-converge. The per-command checksums are published too, so
//! `xtask` can name the *first* diverging command rather than only reporting
//! that two targets disagree.
//!
//! # Why these cases
//!
//! Taken from the probe, where they were measured to localise a divergence
//! rather than merely detect one: if `cpm-strafe-turn` differs but
//! `cpm-yaw-0` does not, the culprit is angle-dependent; if `cpm-yaw-0`
//! differs too, it is something more basic. `cpm-fine-turn` is the case that
//! re-converged. Three cases were added on top of the probe's six: a pitch
//! sweep (the probe held pitch level, so it never exercised the first of the
//! three `sin_cos` calls in `angle_vectors` with a varying argument) and two
//! rate variants, because the command duration feeds every integration in the
//! step and 76 Hz truncates 1000/76 to 13 ms.
//!
//! # What it must not do
//!
//! Store a golden digest. The check is *relative*: all targets must agree
//! with each other. That is what keeps it from needing to be re-recorded
//! every time the physics legitimately changes — C1's owned `sin_cos` and
//! C3's 16-bit angles both change every number in here, and neither should
//! require touching this file.

use std::sync::OnceLock;

use straf3_sim::num::{s, vec3};
use straf3_sim::world::FlatGround;
use straf3_sim::{
    Buttons, PhysicsProfile, SimState, TickRate, UserCmd, angle_to_short, step_in_place,
};

/// Bumped when the report text format changes in a way a reader must notice.
/// `xtask` refuses to compare reports that do not agree on it, which is what
/// stops a stale binary in a target directory from being compared against a
/// fresh one.
pub const REPORT_VERSION: u32 = 1;

/// Commands per case. 1 200 at 125 Hz is 9.6 s of simulated movement — long
/// enough for the known glibc/musl divergence (first seen around command 393)
/// to appear and grow.
pub const STEPS: u32 = 1_200;

/// The triple this binary was compiled for, as the report states it.
///
/// Reported so `xtask` can verify that the artefact it just ran is the one it
/// asked for — a stale binary left in `target/<triple>/release` by an earlier
/// build is otherwise indistinguishable from a fresh one.
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

// ── the reference cases ─────────────────────────────────────────────────────

/// How a case aims the view horizontally.
#[derive(Clone, Copy)]
enum Yaw {
    /// A constant yaw. `Fixed(0.0)` makes `sin`/`cos` exactly 0 and 1, so a
    /// divergence here cannot be a transcendental.
    Fixed(f32),
    /// `i * step` degrees — a different angle on every command, which is what
    /// makes a 1-ULP `sin_cos` disagreement show up at all.
    Step(f32),
}

/// How a case aims the view vertically.
#[derive(Clone, Copy)]
enum Pitch {
    Level,
    /// A saw-tooth from -45° to +45° in half-degree steps, repeating every
    /// 180 commands.
    Sweep,
}

/// One reference case: a physics profile, a command rate, and an input
/// program that is a pure function of the command index and the ground state.
#[derive(Clone, Copy)]
pub struct Case {
    pub name: &'static str,
    pub hz: u32,
    vq3: bool,
    yaw: Yaw,
    pitch: Pitch,
    /// Press nothing at all: fall, land, rest.
    still: bool,
    /// Jump whenever grounded. Off for the fine-turn case so friction and
    /// acceleration dominate instead of ballistics.
    jump: bool,
}

/// The reference stream. Order is part of the format: `xtask` compares case
/// `n` against case `n`, and reports disagree if their names differ.
pub const CASES: &[Case] = &[
    Case {
        name: "cpm-strafe-turn",
        hz: 125,
        vq3: false,
        yaw: Yaw::Step(0.37),
        pitch: Pitch::Level,
        still: false,
        jump: true,
    },
    Case {
        name: "vq3-strafe-turn",
        hz: 125,
        vq3: true,
        yaw: Yaw::Step(0.37),
        pitch: Pitch::Level,
        still: false,
        jump: true,
    },
    Case {
        name: "cpm-yaw-0",
        hz: 125,
        vq3: false,
        yaw: Yaw::Fixed(0.0),
        pitch: Pitch::Level,
        still: false,
        jump: true,
    },
    Case {
        name: "cpm-yaw-90",
        hz: 125,
        vq3: false,
        yaw: Yaw::Fixed(90.0),
        pitch: Pitch::Level,
        still: false,
        jump: true,
    },
    Case {
        name: "cpm-still",
        hz: 125,
        vq3: false,
        yaw: Yaw::Fixed(0.0),
        pitch: Pitch::Level,
        still: true,
        jump: false,
    },
    Case {
        name: "cpm-fine-turn",
        hz: 125,
        vq3: false,
        yaw: Yaw::Step(0.013),
        pitch: Pitch::Level,
        still: false,
        jump: false,
    },
    Case {
        name: "cpm-pitch-sweep",
        hz: 125,
        vq3: false,
        yaw: Yaw::Step(0.37),
        pitch: Pitch::Sweep,
        still: false,
        jump: true,
    },
    Case {
        name: "cpm-strafe-turn-250hz",
        hz: 250,
        vq3: false,
        yaw: Yaw::Step(0.37),
        pitch: Pitch::Level,
        still: false,
        jump: true,
    },
    Case {
        name: "vq3-strafe-turn-76hz",
        hz: 76,
        vq3: true,
        yaw: Yaw::Step(0.37),
        pitch: Pitch::Level,
        still: false,
        jump: true,
    },
];

fn spawn_state() -> SimState {
    SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0))
}

/// Build command `i` of a case.
///
/// The one place in this crate that writes a view angle. When C3 moves
/// `ViewAngles` to 16-bit shorts this function changes and nothing else does;
/// the digests change with it, and no fixture needs re-recording because the
/// check compares targets against each other rather than against a baseline.
fn cmd_for(case: &Case, i: u32, grounded: bool) -> UserCmd {
    let rate = TickRate::from_hz(case.hz).expect("reference case rate is in 1..=1000");
    let mut cmd = UserCmd::still_at(rate);

    // C3: view angles are 16-bit at the command boundary. The cases are
    // authored in degrees because that is what makes them readable as
    // movement, and quantised here — at the boundary, exactly where a real
    // producer quantises — rather than being written as short literals. The
    // wrap in `angle_to_short` is load-bearing for the stepping cases:
    // `Yaw::Step(0.37)` passes 360° around command 973, and the mask folds it
    // back into one turn instead of handing a growing angle to `sin_cos`.
    cmd.view.yaw = angle_to_short(match case.yaw {
        Yaw::Fixed(deg) => s(deg),
        Yaw::Step(deg) => s(i as f32) * s(deg),
    });
    cmd.view.pitch = angle_to_short(match case.pitch {
        Pitch::Level => s(0.0),
        Pitch::Sweep => s(-45.0) + s((i % 180) as f32) * s(0.5),
    });

    if case.still {
        return cmd;
    }

    cmd.forward_move = 127;
    // Flip the strafe key every 12 commands, the way a player half-beats a
    // strafe jump; the sign flip is what makes the turn productive.
    cmd.right_move = if (i / 12).is_multiple_of(2) {
        127
    } else {
        -127
    };
    if grounded && case.jump {
        cmd.buttons = Buttons::JUMP;
    }
    cmd
}

// ── the digest ──────────────────────────────────────────────────────────────

// FNV-1a, the same function and constants `SimState::checksum` uses. Chosen
// for the same reasons: order-dependent, no allocation, no dependency, and
// identical on every platform for the same input bits. It is an equality
// check, not a security primitive.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Fold one 64-bit value into a rolling digest.
#[must_use]
pub fn fold(mut digest: u64, value: u64) -> u64 {
    for b in value.to_le_bytes() {
        digest ^= u64::from(b);
        digest = digest.wrapping_mul(FNV_PRIME);
    }
    digest
}

/// What one case produced.
pub struct CaseResult {
    /// `SimState::checksum()` after each command, in order. Diagnostic: this
    /// is what names the *first* diverging command, and what shows whether a
    /// divergence later re-converged.
    pub steps: Vec<u64>,
    /// The digest folded over every entry of `steps`. This is the number the
    /// check compares.
    pub rolling: u64,
    /// The checksum after the last command — i.e. what an end-state
    /// comparison would have looked at. Published only so a failure can say
    /// out loud when end-state comparison would have missed the divergence.
    pub final_checksum: u64,
}

fn run_case(case: &Case) -> CaseResult {
    let world = FlatGround::at(s(0.0));
    let profile = if case.vq3 {
        PhysicsProfile::vq3()
    } else {
        PhysicsProfile::cpm()
    };

    let mut state = spawn_state();
    let mut steps = Vec::with_capacity(STEPS as usize);
    let mut rolling = FNV_OFFSET;
    for i in 0..STEPS {
        let grounded = state.player.ground.is_grounded();
        let cmd = cmd_for(case, i, grounded);
        step_in_place(&mut state, &cmd, &world, &profile);
        let checksum = state.checksum();
        rolling = fold(rolling, checksum);
        steps.push(checksum);
    }

    CaseResult {
        final_checksum: *steps.last().expect("STEPS > 0"),
        steps,
        rolling,
    }
}

/// Every case, computed once.
pub fn results() -> &'static Vec<CaseResult> {
    static CACHE: OnceLock<Vec<CaseResult>> = OnceLock::new();
    CACHE.get_or_init(|| CASES.iter().map(run_case).collect())
}

/// The digest folded over every case's rolling digest — one number for the
/// whole run, useful in a log line. Never compared on its own.
#[must_use]
pub fn grand_digest() -> u64 {
    results()
        .iter()
        .fold(FNV_OFFSET, |acc, case| fold(acc, case.rolling))
}

// ── the report ──────────────────────────────────────────────────────────────

/// The report text, in the format `xtask::determinism` parses.
///
/// Deliberately line-oriented and readable: a human diffing two of these by
/// hand is a supported way to debug a failure.
///
/// ```text
/// straf3-determinism-report 1
/// target x86_64-unknown-linux-gnu
/// platform native-x86_64-linux
/// cases 9
/// steps 1200
/// grand 0123456789abcdef
/// case 0 cpm-strafe-turn hz 125 rolling <hex> final <hex>
/// checksums 0 <hex>,<hex>,...
/// ```
#[must_use]
pub fn render(platform: &str) -> String {
    let cases = results();
    // 9 cases x 1200 commands x 17 bytes of hex, plus headers.
    let mut o = String::with_capacity(CASES.len() * STEPS as usize * 17 + 4096);

    push_line(
        &mut o,
        &format!("straf3-determinism-report {REPORT_VERSION}"),
    );
    push_line(&mut o, &format!("target {TARGET}"));
    push_line(&mut o, &format!("platform {platform}"));
    push_line(&mut o, &format!("cases {}", CASES.len()));
    push_line(&mut o, &format!("steps {STEPS}"));
    push_line(&mut o, &format!("grand {}", hex(grand_digest())));

    for (i, (case, result)) in CASES.iter().zip(cases).enumerate() {
        push_line(
            &mut o,
            &format!(
                "case {i} {} hz {} rolling {} final {}",
                case.name,
                case.hz,
                hex(result.rolling),
                hex(result.final_checksum)
            ),
        );
    }
    for (i, result) in cases.iter().enumerate() {
        o.push_str("checksums ");
        o.push_str(&i.to_string());
        o.push(' ');
        for (n, checksum) in result.steps.iter().enumerate() {
            if n > 0 {
                o.push(',');
            }
            o.push_str(&hex(*checksum));
        }
        o.push('\n');
    }
    o
}

fn push_line(o: &mut String, line: &str) {
    o.push_str(line);
    o.push('\n');
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

/// The rendered report, computed once and kept alive for the wasm exports.
#[cfg(target_arch = "wasm32")]
fn cached_report() -> &'static String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| render(TARGET))
}

// ── the wasm surface ────────────────────────────────────────────────────────
//
// Three exports, called through the raw C ABI with no wasm-bindgen: the host
// reads the *already rendered* report straight out of linear memory. Doing it
// this way rather than exporting one accessor per number means the JavaScript
// runner cannot format the report differently from the native binary, because
// it does no formatting at all.

/// Byte offset of the rendered report in linear memory.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn d_report_ptr() -> u32 {
    cached_report().as_ptr() as u32
}

/// Length of the rendered report in bytes (UTF-8, in fact ASCII).
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn d_report_len() -> u32 {
    cached_report().len() as u32
}

/// The report format version, so the runner can refuse a stale `.wasm`.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn d_report_version() -> u32 {
    REPORT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::Scalar;

    #[test]
    fn the_digest_is_sticky_once_it_diverges() {
        // The property the whole check rests on: a rolling digest cannot
        // re-converge after a disagreement, so comparing the final digest is
        // equivalent to comparing every intermediate one.
        let a = [1u64, 2, 3, 4, 5];
        let b = [1u64, 2, 99, 4, 5]; // differs at index 2, agrees after
        let fold_all = |xs: &[u64]| xs.iter().fold(FNV_OFFSET, |h, v| fold(h, *v));
        assert_ne!(fold_all(&a), fold_all(&b));
        // ...and the end state alone would have said they were identical.
        assert_eq!(a.last(), b.last());
    }

    #[test]
    fn every_case_is_named_once() {
        for (i, case) in CASES.iter().enumerate() {
            assert!(
                CASES
                    .iter()
                    .skip(i + 1)
                    .all(|other| other.name != case.name),
                "duplicate case name {}",
                case.name
            );
            assert!(
                TickRate::from_hz(case.hz).is_some(),
                "{} has an unusable rate",
                case.name
            );
        }
    }

    #[test]
    fn the_reference_stream_actually_moves_the_player() {
        // Guards against the whole check passing for the boring reason that
        // every target computed the same nothing.
        let result = run_case(&CASES[0]);
        assert_eq!(result.steps.len(), STEPS as usize);
        let distinct = result
            .steps
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert!(
            distinct > STEPS as usize / 2,
            "only {distinct} distinct states in {STEPS} commands — the stream is not exercising the sim"
        );
    }

    #[test]
    fn a_still_player_and_a_strafing_player_do_not_agree() {
        assert_ne!(run_case(&CASES[0]).rolling, run_case(&CASES[4]).rolling);
    }

    #[test]
    fn the_report_round_trips_its_own_shape() {
        let text = render("test");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            format!("straf3-determinism-report {REPORT_VERSION}")
        );
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("case ")).count(),
            CASES.len()
        );
        for line in lines.iter().filter(|l| l.starts_with("checksums ")) {
            let commas = line.split(' ').nth(2).expect("checksum list").split(',');
            assert_eq!(commas.count(), STEPS as usize);
        }
    }

    #[test]
    fn a_run_is_reproducible_within_this_process() {
        assert_eq!(run_case(&CASES[1]).rolling, run_case(&CASES[1]).rolling);
    }

    #[test]
    fn the_scalar_type_is_what_the_digest_assumes() {
        // If `Scalar` ever widens, every digest in this crate changes; that is
        // fine, but it must be a deliberate act.
        assert_eq!(std::mem::size_of::<Scalar>(), 4);
    }
}

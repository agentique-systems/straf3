//! Shared machinery for the Wave 3 seam evidence (spec rev 6, criteria 3–5).
//!
//! # What this is for
//!
//! Criterion 4 says a recorded input sequence must replay to the same checksum
//! through the windowed build as through `straf3-headless`. That is a
//! three-cornered claim, and this module owns the two corners that do not
//! depend on the windowed build existing yet:
//!
//! - **The recording.** A run is defined here as Rust data ([`Run`]), and the
//!   checked-in fixture text is *rendered* from it ([`Run::render`]). There is
//!   deliberately no second parser in this crate: a hand-written parser here
//!   could drift away from `bin/headless.rs`'s, and the symptom of that drift
//!   would be a checksum mismatch that looks exactly like a physics bug.
//!   Rendering instead of parsing means the fixture on disk and the in-process
//!   command list cannot disagree without a test saying so.
//!
//! - **The reference stream.** [`Run::reference_digests`] runs the same
//!   commands through `straf3_sim` in this process, producing one checksum per
//!   tick.
//!
//! # Per-tick, never end-state
//!
//! Every comparison in these tests is over the whole digest stream. The
//! determinism probe (spec rev 6 §R) found a case whose *final* checksum
//! matched across builds while 29 of its 1200 intermediate per-command
//! checksums did not — an end-state-only check would have certified a run that
//! had actually diverged. [`assert_digests_match`] is the only comparison
//! helper here, and it compares every tick.
//!
//! # No golden constants
//!
//! Nothing in this module or its callers asserts a checksum against a literal.
//! Spec rev 6 Q1 replaces the three `f32::sin_cos` calls at
//! `crates/straf3-sim/src/step.rs:955-957` with a Cody–Waite implementation
//! once Wave 2 closes, which changes every checksum in the repository. A
//! literal would break that day; a computed-both-sides comparison survives it
//! untouched.

#![allow(dead_code)] // Each test binary links only the parts it uses.

pub mod pacing;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::{EmptyWorld, FlatGround};
use straf3_sim::{Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, step_in_place};

/// Which of the two worlds `straf3-headless` can build a run uses.
///
/// Deliberately only the worlds the fixture format can name. The arena with
/// ramps that `straf3-render` owns is not among them — see the note on
/// [`Run`] about what that costs criterion 4.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorldChoice {
    /// No geometry at all: every sweep completes.
    Empty,
    /// An infinite horizontal plane at this height.
    Flat(Scalar),
    /// The hardcoded arena with ramps, which lives in `straf3-render`.
    ///
    /// **Not comparable against `straf3-headless`.** Headless's fixture format
    /// has no spelling for this world, and `crates/straf3-sim` is off-limits
    /// this wave, so a run in the arena can only ever be compared against
    /// *itself* — see [`arena_runs`].
    Arena,
}

impl WorldChoice {
    /// Whether `straf3-headless` can build this world.
    ///
    /// The whole reason criterion 4 splits into a flat-ground half and an arena
    /// half.
    pub fn is_comparable_to_headless(self) -> bool {
        !matches!(self, Self::Arena)
    }

    fn render(self) -> String {
        match self {
            Self::Empty => "world empty".to_string(),
            Self::Flat(h) => format!("world flat {}", render_scalar(h)),
            Self::Arena => "world arena".to_string(),
        }
    }
}

/// Which movement profile a run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Challenge ProMode: the spec D1 default.
    Cpm,
    /// Vanilla Quake 3.
    Vq3,
}

impl Profile {
    fn name(self) -> &'static str {
        match self {
            Self::Cpm => "cpm",
            Self::Vq3 => "vq3",
        }
    }

    fn physics(self) -> PhysicsProfile {
        match self {
            Self::Cpm => PhysicsProfile::cpm(),
            Self::Vq3 => PhysicsProfile::vq3(),
        }
    }
}

/// A recorded input sequence: everything needed to reproduce a run exactly.
///
/// # The world is flat, and that is a real limit on criterion 4
///
/// `straf3-headless`'s fixture format can name `empty` or `flat <z>` and
/// nothing else. The playable build runs on the hardcoded arena with ramps
/// (spec rev 6 §T, option (a)), and ramps are precisely where CPM and VQ3
/// diverge most. So a replay-equivalence proof built on these fixtures shows
/// the platform layer changes nothing below the seam *on flat ground*; it does
/// not exercise the arena's ramp geometry. Closing that gap needs the arena
/// `World` reachable from both sides of the comparison. Reported as an
/// uncertainty rather than papered over.
#[derive(Debug, Clone)]
pub struct Run {
    /// File-stem of the checked-in fixture, and the test's name for the run.
    pub name: &'static str,
    /// One line of prose saying what the run is supposed to exercise.
    pub intent: &'static str,
    /// The command rate. Part of the physics (spec D2), so it is recorded.
    pub rate: TickRate,
    /// Which constants the run uses.
    pub profile: Profile,
    /// Which world the run happens in.
    pub world: WorldChoice,
    /// Spawn origin.
    pub spawn: Vec3,
    /// Spawn view yaw, in degrees.
    pub yaw: Scalar,
    /// The commands, in order. Every one lasts `rate.command_millis()`.
    pub cmds: Vec<UserCmd>,
}

impl Run {
    /// Path of this run's checked-in fixture.
    pub fn fixture_path(&self) -> PathBuf {
        fixtures_dir().join(format!("{}.txt", self.name))
    }

    /// Render the run as `straf3-headless` fixture text.
    ///
    /// This is the only direction that exists: text is produced from the
    /// commands, never parsed back into them. See the module docs.
    ///
    /// # Panics
    ///
    /// If any command's duration disagrees with the run's rate. The fixture
    /// format does not write a per-command duration — it derives it from
    /// `rate` — so a run whose commands disagree with its rate could not be
    /// rendered faithfully, and silently rendering it would produce a fixture
    /// that replays as something other than what the run says it is.
    pub fn render(&self) -> String {
        let expected = self.rate.command_millis();
        for (i, cmd) in self.cmds.iter().enumerate() {
            assert_eq!(
                cmd.duration_ms,
                expected,
                "{}: command {i} lasts {} ms but the run's rate is {} Hz ({} ms). \
                 The fixture format derives duration from `rate`, so this run \
                 cannot be written down without changing it.",
                self.name,
                cmd.duration_ms,
                self.rate.hz(),
                expected,
            );
        }

        let mut out = String::new();
        let _ = writeln!(out, "# {} — {}", self.name, self.intent);
        let _ = writeln!(
            out,
            "#\n# GENERATED from crates/straf3-platform/tests/support/mod.rs (`runs()`).\n\
             # Do not hand-edit: `fixtures_match_their_definitions` will fail.\n\
             # Regenerate with STRAF3_BLESS_FIXTURES=1 cargo test -p straf3-platform.\n"
        );
        let _ = writeln!(out, "rate {}", self.rate.hz());
        let _ = writeln!(out, "profile {}", self.profile.name());
        let _ = writeln!(out, "{}", self.world.render());
        let _ = writeln!(
            out,
            "spawn {} {} {}",
            render_scalar(self.spawn.x),
            render_scalar(self.spawn.y),
            render_scalar(self.spawn.z)
        );
        let _ = writeln!(out, "yaw {}", render_scalar(self.yaw));
        let _ = writeln!(
            out,
            "\n# cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> <roll>"
        );

        // Run-length encode identical consecutive commands, which is what the
        // `<repeat>` field is for and what keeps a 2000-command fixture
        // readable.
        let mut i = 0;
        while i < self.cmds.len() {
            let cmd = self.cmds[i];
            let mut repeat = 1;
            while i + repeat < self.cmds.len() && self.cmds[i + repeat] == cmd {
                repeat += 1;
            }
            let _ = writeln!(
                out,
                "cmd {repeat} {} {} {} {} {} {} {}",
                cmd.forward_move,
                cmd.right_move,
                cmd.up_move,
                render_buttons(cmd.buttons),
                render_scalar(cmd.view.pitch),
                render_scalar(cmd.view.yaw),
                render_scalar(cmd.view.roll),
            );
            i += repeat;
        }
        out
    }

    /// One checksum per tick, computed in this process through `straf3_sim`.
    ///
    /// Index 0 is the spawn state before any command runs, matching
    /// `straf3-headless --trace`, which emits the initial state first.
    pub fn reference_digests(&self) -> Vec<u64> {
        let profile = self.profile.physics();
        let mut state = SimState::spawned_at(self.spawn, self.yaw);
        let mut out = Vec::with_capacity(self.cmds.len() + 1);
        out.push(state.checksum());

        assert!(
            self.world.is_comparable_to_headless(),
            "{}: runs in the arena, which lives in straf3-render and cannot be \
             built here. An arena run has no in-process reference and no \
             headless reference — it is only ever compared against itself \
             under different frame schedules. Use arena_runs(), not runs().",
            self.name,
        );

        // The two arms differ only in the world's concrete type, which `World`
        // being a trait with a generic `step_in_place` forces to be two calls.
        match self.world {
            WorldChoice::Arena => unreachable!("refused above"),
            WorldChoice::Empty => {
                let world = EmptyWorld;
                for cmd in &self.cmds {
                    step_in_place(&mut state, cmd, &world, &profile);
                    out.push(state.checksum());
                }
            }
            WorldChoice::Flat(height) => {
                let world = FlatGround::at(height);
                for cmd in &self.cmds {
                    step_in_place(&mut state, cmd, &world, &profile);
                    out.push(state.checksum());
                }
            }
        }
        out
    }

    /// Run this run's checked-in fixture through the `straf3-headless` binary
    /// and return one checksum per tick.
    pub fn headless_digests(&self) -> Vec<u64> {
        assert!(
            self.world.is_comparable_to_headless(),
            "{}: straf3-headless cannot build the arena world — its fixture \
             format has no spelling for it, and crates/straf3-sim is off-limits \
             this wave. Comparing an arena run against headless is not a test \
             that can be made to pass; it is a category error.",
            self.name,
        );
        let path = self.fixture_path();
        let bin = headless_binary();
        let out = Command::new(&bin)
            .arg(&path)
            .arg("--trace")
            .arg("--csv")
            .output()
            .unwrap_or_else(|e| panic!("could not execute {}: {e}", bin.display()));

        assert!(
            out.status.success(),
            "{} {} --trace --csv exited {}\nstderr:\n{}",
            bin.display(),
            path.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr),
        );

        parse_trace_csv(&String::from_utf8_lossy(&out.stdout))
    }
}

/// Pull the checksum column out of `straf3-headless --trace --csv` output.
///
/// Strict on purpose: an unparseable line is a panic, not a skipped row. A
/// lenient parser here would silently shorten the digest stream, and a
/// comparison of two short streams is the "passes unconditionally" failure
/// this whole exercise exists to avoid.
pub fn parse_trace_csv(stdout: &str) -> Vec<u64> {
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());

    let header = lines
        .next()
        .expect("straf3-headless --trace --csv produced no output at all");
    assert_eq!(
        header, "tick,time_ms,x,y,z,vx,vy,vz,speed,grounded,checksum",
        "straf3-headless's CSV header changed; this parser assumes the \
         checksum is the last column",
    );

    let digests: Vec<u64> = lines
        .map(|line| {
            let field = line
                .rsplit(',')
                .next()
                .unwrap_or_else(|| panic!("no columns in trace line {line:?}"));
            let hex = field.strip_prefix("0x").unwrap_or_else(|| {
                panic!("checksum field {field:?} in line {line:?} is not 0x-prefixed")
            });
            u64::from_str_radix(hex, 16)
                .unwrap_or_else(|e| panic!("checksum {field:?} is not hex: {e}"))
        })
        .collect();

    assert!(
        !digests.is_empty(),
        "straf3-headless emitted a header but no tick rows",
    );
    digests
}

/// Compare two per-tick digest streams and fail with the *first* divergence.
///
/// This is the only comparison the seam tests use, and it exists as a named
/// function so the per-tick property is stated once rather than re-derived at
/// each call site. Comparing final states only would have passed the probe
/// case where 29 of 1200 intermediate checksums differed while the last one
/// matched (spec rev 6 §R).
///
/// # Panics
///
/// If the streams differ in length or at any tick.
pub fn assert_digests_match(left_label: &str, left: &[u64], right_label: &str, right: &[u64]) {
    assert!(
        !left.is_empty() && !right.is_empty(),
        "refusing to compare an empty digest stream: {left_label} has {} ticks, \
         {right_label} has {}. Two empty streams are equal, which would make \
         this assertion pass without proving anything.",
        left.len(),
        right.len(),
    );

    if let Some((tick, (l, r))) = left
        .iter()
        .zip(right.iter())
        .enumerate()
        .find(|(_, (l, r))| l != r)
    {
        panic!(
            "{left_label} and {right_label} diverge at tick {tick} of {}:\n  \
             {left_label:>24}: {l:#018x}\n  {right_label:>24}: {r:#018x}\n\n\
             The final checksums are {}. Note that a matching final checksum \
             would not have made this run equivalent — this is exactly the \
             case spec rev 6 §R records.",
            left.len().min(right.len()),
            if left.last() == right.last() {
                "IDENTICAL, and the divergence above is mid-run"
            } else {
                "also different"
            },
        );
    }

    assert_eq!(
        left.len(),
        right.len(),
        "{left_label} ran for {} ticks and {right_label} for {} — every tick \
         they share agrees, but one stream is truncated",
        left.len(),
        right.len(),
    );
}

/// The runs the seam evidence is built on.
///
/// Each one exists to exercise something the platform layer could plausibly
/// break, not to be a pretty demo.
pub fn runs() -> Vec<Run> {
    vec![
        still_on_ground(),
        strafe_jump_cpm(),
        strafe_jump_vq3(),
        airborne_turn_empty(),
    ]
}

/// Runs that happen in the **arena**, and therefore only the windowed build can
/// execute.
///
/// # Why these are separate from [`runs`]
///
/// Criterion 4 names `straf3-headless` as its reference, and headless's fixture
/// format can spell `empty` and `flat <z>` and nothing else. The playable build
/// runs the hardcoded arena with ramps — and ramps are, in the spec's own
/// words, *"where CPM and VQ3 diverge most, and the discrete branches the
/// 1-ULP finding warns about."* So the geometry that matters most is the
/// geometry the named reference structurally cannot run.
///
/// Rather than pretend otherwise, criterion 4's evidence splits in two
/// (coordinator-approved):
///
/// 1. **Flat ground, three corners.** [`runs`] — the windowed build against
///    `straf3-headless` against the in-process simulation, per tick.
/// 2. **Arena, self-consistency.** These runs — the same recorded input through
///    the windowed build under wildly different *frame schedules*, required to
///    produce byte-identical per-tick digests. `straf3-headless` has no frame
///    loop at all, so the flat-ground half already pins the absolute answer;
///    this half pins that ramp geometry is not perturbed by frame pacing, which
///    is what "the renderer changes nothing below the seam" means on the
///    geometry it is hardest to be sure about.
///
/// Neither half alone is criterion 4. Together they are.
pub fn arena_runs() -> Vec<Run> {
    let rate = TickRate::HZ_125;
    let ms = rate.command_millis();
    let mut cmds = Vec::new();

    // Settle onto whatever is underfoot.
    for _ in 0..30 {
        cmds.push(UserCmd::still_at(rate));
    }
    // Run forward into the arena, so the run meets geometry rather than air.
    for _ in 0..80 {
        cmds.push(UserCmd {
            duration_ms: ms,
            forward_move: 127,
            ..UserCmd::still_at(rate)
        });
    }
    // Jump and strafe with a sweeping view: ramps, walls and edges get hit
    // while airborne, which is where the multi-plane slide solver and step-up
    // actually branch.
    cmds.push(UserCmd {
        duration_ms: ms,
        forward_move: 127,
        buttons: Buttons::JUMP,
        ..UserCmd::still_at(rate)
    });
    for i in 0..260 {
        let t = i as f32;
        cmds.push(UserCmd {
            duration_ms: ms,
            forward_move: 127,
            right_move: if i % 64 < 32 { 127 } else { -127 },
            up_move: 0,
            buttons: if i % 53 == 0 {
                Buttons::JUMP
            } else {
                Buttons::NONE
            },
            view: ViewAngles {
                pitch: s(0.0),
                yaw: s(0.9) * t,
                roll: s(0.0),
            },
        });
    }

    vec![Run {
        name: "arena_ramp_run",
        intent: "run, jump and strafe across the arena's ramps — geometry straf3-headless cannot build",
        rate,
        profile: Profile::Cpm,
        world: WorldChoice::Arena,
        spawn: vec3(s(0.0), s(0.0), s(64.0)),
        yaw: s(0.0),
        cmds,
    }]
}

/// One run by name.
///
/// # Panics
///
/// If no run has that name.
pub fn run_named(name: &str) -> Run {
    all_runs()
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no run named {name:?}"))
}

/// Every run that has a checked-in fixture, comparable to headless or not.
pub fn all_runs() -> Vec<Run> {
    let mut all = runs();
    all.extend(arena_runs());
    all
}

/// The degenerate run: no input at all. If the platform layer injects a
/// spurious command, drops one, or gets the spawn state wrong, this is the
/// smallest run that shows it.
fn still_on_ground() -> Run {
    let rate = TickRate::HZ_125;
    Run {
        name: "still_on_ground",
        intent: "no input for 2 s: catches an injected, dropped or misordered command",
        rate,
        profile: Profile::Cpm,
        world: WorldChoice::Flat(s(0.0)),
        spawn: vec3(s(0.0), s(0.0), s(64.0)),
        yaw: s(0.0),
        cmds: vec![UserCmd::still_at(rate); 250],
    }
}

/// A real strafe-jump: land, build speed, jump, then hold strafe while the
/// view sweeps. This is the run the whole project is about, and the one where
/// a 1-ULP difference in `sin_cos` would show up first.
fn strafe_jump_cpm() -> Run {
    strafe_jump("strafe_jump_cpm", Profile::Cpm)
}

/// The same input under VQ3 constants. Included because the two profiles take
/// different branches through air control, so a run that is equivalent under
/// one is not automatically equivalent under the other.
fn strafe_jump_vq3() -> Run {
    strafe_jump("strafe_jump_vq3", Profile::Vq3)
}

fn strafe_jump(name: &'static str, profile: Profile) -> Run {
    let rate = TickRate::HZ_125;
    let ms = rate.command_millis();
    let mut cmds = Vec::new();

    // Fall to the floor and settle.
    for _ in 0..40 {
        cmds.push(UserCmd::still_at(rate));
    }
    // Run forward on the ground to build ground speed.
    for _ in 0..60 {
        cmds.push(UserCmd {
            duration_ms: ms,
            forward_move: 127,
            view: ViewAngles {
                pitch: s(0.0),
                yaw: s(0.0),
                roll: s(0.0),
            },
            ..UserCmd::still_at(rate)
        });
    }
    // Jump.
    cmds.push(UserCmd {
        duration_ms: ms,
        forward_move: 127,
        buttons: Buttons::JUMP,
        ..UserCmd::still_at(rate)
    });
    // The strafe itself: hold right, sweep the view, exactly as a player does.
    // Yaw advances by a fixed amount per command so the fixture is a pure
    // function of the loop and reproduces byte-for-byte.
    for i in 0..220 {
        let yaw = s(0.55) * (i as f32);
        cmds.push(UserCmd {
            duration_ms: ms,
            forward_move: 0,
            right_move: 127,
            buttons: Buttons::NONE,
            view: ViewAngles {
                pitch: s(0.0),
                yaw,
                roll: s(0.0),
            },
            ..UserCmd::still_at(rate)
        });
    }

    Run {
        name,
        intent: "run, jump, then strafe with a sweeping view — the movement the project exists for",
        rate,
        profile,
        world: WorldChoice::Flat(s(0.0)),
        spawn: vec3(s(0.0), s(0.0), s(64.0)),
        yaw: s(0.0),
        cmds,
    }
}

/// Pitch, yaw and roll all moving, in a world with no geometry, at a rate
/// whose command duration does not divide 1000 evenly (76 Hz → 13 ms).
///
/// The odd rate is the point: it is the case where an accumulator that thinks
/// in whole frames rather than whole milliseconds drifts.
fn airborne_turn_empty() -> Run {
    let rate = TickRate::HZ_76;
    let ms = rate.command_millis();
    let cmds = (0..300)
        .map(|i| {
            let t = i as f32;
            UserCmd {
                duration_ms: ms,
                forward_move: 96,
                right_move: -64,
                up_move: 0,
                buttons: if i % 37 == 0 {
                    Buttons::JUMP
                } else {
                    Buttons::NONE
                },
                view: ViewAngles {
                    pitch: s(-20.0) + s(0.13) * t,
                    yaw: s(1.7) * t,
                    roll: s(0.05) * t,
                },
            }
        })
        .collect();

    Run {
        name: "airborne_turn_empty",
        intent: "13 ms commands (76 Hz) in an empty world, all three view axes moving",
        rate,
        profile: Profile::Cpm,
        world: WorldChoice::Empty,
        spawn: vec3(s(0.0), s(0.0), s(1024.0)),
        yaw: s(-20.0),
        cmds,
    }
}

// ---------------------------------------------------------------------------
// Paths and rendering details
// ---------------------------------------------------------------------------

/// `crates/straf3-platform/tests/fixtures`.
///
/// Resolved through [`workspace_root`] rather than `CARGO_MANIFEST_DIR` so this
/// module can be included from `crates/straf3-game/tests/` too — corner C of
/// criterion 4 must compare the windowed build against *the same* recorded
/// inputs, and a second copy of the fixtures would be a second thing to drift.
pub fn fixtures_dir() -> PathBuf {
    workspace_root()
        .join("crates")
        .join("straf3-platform")
        .join("tests")
        .join("fixtures")
}

/// The workspace root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root should exist relative to CARGO_MANIFEST_DIR")
}

/// Locate the `straf3-headless` binary, building it if it is not there yet.
///
/// # Why this is not `env!("CARGO_BIN_EXE_straf3-headless")`
///
/// That macro is only defined for integration tests *of the package that
/// declares the binary*. `straf3-headless` belongs to `straf3-sim`; these
/// tests belong to `straf3-platform`, so the macro is unavailable and the
/// binary has to be found on disk.
///
/// `cargo test --workspace` builds it as a matter of course. A bare
/// `cargo test -p straf3-platform` does not, because a sibling package's
/// binary is not a dependency of this package's tests — so this falls back to
/// building it into its own target directory, which keeps it clear of the
/// outer `cargo test`'s lock on `target/`.
///
/// # Panics
///
/// If the binary can be neither found nor built. It deliberately does not
/// skip: a test that quietly passes when its subject is missing is worth less
/// than no test.
pub fn headless_binary() -> PathBuf {
    // Resolved once per test binary: the fallback shells out to cargo, and
    // without this every run in `runs()` would pay for that separately.
    static RESOLVED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(resolve_headless_binary).clone()
}

fn resolve_headless_binary() -> PathBuf {
    const EXE: &str = if cfg!(windows) {
        "straf3-headless.exe"
    } else {
        "straf3-headless"
    };

    // Walk up from this test binary: target/<profile>/deps/<test>-<hash>.
    let mut dir = std::env::current_exe().expect("test binary should have a path");
    for _ in 0..4 {
        if !dir.pop() {
            break;
        }
        let candidate = dir.join(EXE);
        if candidate.is_file() {
            return candidate;
        }
    }

    let target = workspace_root().join("target").join("harness-oracle");
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(workspace_root())
        .args(["build", "-p", "straf3-sim", "--bin", "straf3-headless"])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .unwrap_or_else(|e| panic!("could not run cargo to build straf3-headless: {e}"));
    assert!(
        status.success(),
        "building straf3-headless failed ({status}). Run \
         `cargo build -p straf3-sim --bin straf3-headless` and try again."
    );

    let built = target.join("debug").join(EXE);
    assert!(
        built.is_file(),
        "cargo reported success but {} does not exist",
        built.display(),
    );
    built
}

/// Format a scalar so `straf3-headless`'s `str::parse::<f32>` reads back the
/// identical bit pattern.
///
/// `{}` on `f32` in Rust prints the shortest decimal that round-trips, so this
/// is exact rather than merely close — asserted by
/// `rendered_scalars_round_trip_exactly`, which calls *this function* rather
/// than re-inlining the format string. That distinction is not pedantry: an
/// earlier version of that test inlined `format!("{v}")`, so it went on
/// passing when this function was mutated to `{v:.3}` — it was testing the
/// standard library instead of the code the fixtures actually go through.
pub fn render_scalar(v: Scalar) -> String {
    format!("{v}")
}

fn render_buttons(b: Buttons) -> String {
    let mut parts = Vec::new();
    for (bit, name) in [
        (Buttons::JUMP, "jump"),
        (Buttons::CROUCH, "crouch"),
        (Buttons::ATTACK, "attack"),
        (Buttons::WALK, "walk"),
    ] {
        if b.contains(bit) {
            parts.push(name);
        }
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("+")
    }
}

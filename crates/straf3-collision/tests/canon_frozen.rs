//! The canon freeze: proof that `vq3` and `cpm` movement did not move.
//!
//! Spec criterion 4, read as spec decision D9. **This is the first gate of its
//! kind in the repository**, so this file explains itself at length rather than
//! assuming a reader knows why it exists.
//!
//! # What was and was not already protected
//!
//! Before this wave the tree had two things that look like they pin movement
//! and are not:
//!
//! - **`cargo xtask determinism`** (`tools/det-runner`) compares the four build
//!   targets *against each other*. Its own module docs list "store a golden
//!   digest" under **what it must not do**, and that is the correct design for
//!   what it checks: it asks "do all targets agree", not "does today agree with
//!   yesterday". Change a movement constant and every target changes together,
//!   and the check stays green.
//! - **`crates/straf3-sim/tests/movement.rs`** pins individual techniques with
//!   two-sided assertions, which is genuinely strong — but each one covers the
//!   behaviour it was written for. Nothing asked "did *anything* about a run
//!   change", and nothing covered brush geometry at all, because that file's
//!   fixtures are analytic half-spaces rather than the tracer the game uses.
//!
//! So a change to `step.rs` that altered every number in the simulation could
//! have landed with a green suite, as long as it did not happen to break one of
//! the named techniques. This file closes that.
//!
//! # Why a *movement* digest and not `SimState::checksum()`
//!
//! The literal reading of criterion 4 — "replay a reference recording to an
//! unchanged checksum" — cannot be satisfied by any correct implementation of
//! this wave, and the reason is worth stating precisely because it looks like a
//! dodge and is not.
//!
//! `SimState::checksum()` folds every field of `PlayerState`, and `state.rs`
//! carries `the_checksum_covers_the_state_a_technique_depends_on`, which asserts
//! that every field the movement code branches on *is* folded. That invariant is
//! right: a replay that diverges while reporting a matching checksum is worse
//! than one that diverges visibly.
//!
//! This wave adds an `experimental` profile with slide and dash timers. Those
//! timers are state the movement code branches on, so the invariant *requires*
//! them to be folded — and the moment they are, `SimState::checksum()` changes
//! for `vq3` and `cpm` too, because the struct they fold grew a field. Canon
//! movement would not have moved a millimetre and the literal gate would be red.
//! Not folding them keeps the gate green and breaks the invariant, which is the
//! worse of the two.
//!
//! The question criterion 4 is actually protecting is **"did the player go
//! anywhere different?"** So that is what is folded here, per command:
//!
//! ```text
//! origin.x/y/z bits · velocity.x/y/z bits ·
//! ground discriminant (0 airborne / 1 grounded / 2 sliding) and its normal bits ·
//! run discriminant and its ms fields · tick · time_ms
//! ```
//!
//! and deliberately **not** `timers`, `crouched`, `jump_held`,
//! `left_ground_by_jumping` or `view`. Those are internal bookkeeping whose
//! *representation* may grow this wave while its canon *behaviour* may not. The
//! set that is folded is exactly what a player, a spectator or a leaderboard can
//! observe: where you were, how fast, what you were standing on, and what the
//! clock said.
//!
//! Note what this costs, honestly: a change that alters a timer's value without
//! altering any position, velocity, ground state or time is invisible to this
//! gate. That is not a loophole, it is the definition — such a change did not
//! change the run. If it later *does* change a run, it changes an origin on the
//! command where it does, and this catches it there.
//!
//! # Why it lives in `straf3-collision` and not in `straf3-sim`
//!
//! Because it has to cover brush geometry. Ramp, step, corner and ledge
//! behaviour are artefacts of the brush tracer's bevels and epsilons, and a
//! freeze that only covered `FlatGround` would leave every geometric behaviour
//! this wave is hardening unprotected. `straf3-collision` already depends on
//! `straf3-sim`, so a geometry-covering freeze needs no new dependency edge and
//! no dev-dependency cycle. `FlatGround` is included as well, because it is the
//! world the existing technique tests are written in.
//!
//! # Where the goldens came from
//!
//! Captured on this branch's base commit, **102894d**, which is pre-wave HEAD.
//! Nothing in `crates/straf3-sim/src/**` had been touched when they were taken;
//! the only change in the tree at capture time was the addition of
//! `straf3_collision::testbed`, which adds geometry and no movement code.
//!
//! **A golden here may not be regenerated to make a test pass.** If this file
//! goes red, either canon moved — which this wave forbids outside
//! `experimental` — or the command program below changed, which is the same
//! thing wearing a different hat. There is deliberately no regeneration mode:
//! the only way to update these numbers is to type them, which is exactly the
//! amount of friction a freeze should have.
//!
//! # Why the goldens are a ladder rather than one number per command
//!
//! A single rolling digest per case says *that* something moved. It cannot say
//! *when*. A golden for every command could, at a cost of seven thousand inline
//! literals for the fourteen cases here, which is more numbers than a reviewer
//! will ever read and therefore a worse gate in practice: unreviewable goldens
//! get regenerated.
//!
//! So each case stores the rolling digest plus a checkpoint ladder that is dense
//! where divergences actually start — every command through 16, then widening to
//! 500. A constant or an order-of-operations change diverges at command 1 and is
//! named exactly. A change that only fires in a specific situation — a new
//! step-up rule, a different ramp clip — diverges when the run reaches that
//! situation, and is bracketed to within a factor of two, *and* the failure
//! prints the player's full observable state at that checkpoint, which is what a
//! reader actually needs to recognise what changed. This is a deliberate
//! deviation from "name the exact command index", made for legibility, and the
//! bracket plus state dump is strictly more information per byte than an index
//! alone.
//!
//! # A freeze has to be shown to bite
//!
//! `a_minimal_change_to_canon_is_caught_by_the_frozen_digest` runs every case
//! again under profiles perturbed by the smallest edit anyone could make — one
//! ulp of `overclip`, `step_height` 18 → 17.9, `min_walk_normal` 0.7 → 0.71 —
//! and requires the frozen digests to notice. It is not decoration: writing it
//! found **two holes in this fixture that a passing gate had hidden**, and both
//! are recorded where they were fixed. The first draft spawned the player 512
//! units from each feature and never reached any of them, so three worlds
//! produced one digest under three names. The second draft bunny-hopped clean
//! over its own 18-unit step, so `step_height` was uncovered. A freeze that has
//! never been shown to fail is a freeze nobody has checked.

use straf3_collision::testbed;
use straf3_sim::num::{Scalar, Vec3, s, to_bits, vec3};
use straf3_sim::state::{GroundState, RunState};
use straf3_sim::world::{FlatGround, World};
use straf3_sim::{Buttons, PhysicsProfile, SimState, UserCmd, ViewAngles, step};

// ═══ the digest ════════════════════════════════════════════════════════════

// FNV-1a, the same function and constants `SimState::checksum` and
// `det-runner` use: order-dependent, no allocation, no dependency, identical on
// every platform for the same input bits. An equality check, not a security
// primitive.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fold_u32(digest: &mut u64, value: u32) {
    for b in value.to_le_bytes() {
        *digest ^= u64::from(b);
        *digest = digest.wrapping_mul(FNV_PRIME);
    }
}

fn fold_scalar(digest: &mut u64, value: Scalar) {
    fold_u32(digest, to_bits(value));
}

fn fold_vec(digest: &mut u64, value: Vec3) {
    fold_scalar(digest, value.x);
    fold_scalar(digest, value.y);
    fold_scalar(digest, value.z);
}

/// Everything about a state a player, a spectator or a leaderboard can observe.
///
/// The order is fixed by spec D9 and must not be rearranged: FNV is
/// order-dependent, so a reordering changes every golden in this file without
/// changing a single thing about the simulation.
fn movement_digest(state: &SimState) -> u64 {
    let mut h = FNV_OFFSET;
    fold_vec(&mut h, state.player.origin);
    fold_vec(&mut h, state.player.velocity);
    match state.player.ground {
        GroundState::Airborne => fold_u32(&mut h, 0),
        GroundState::Grounded { normal } => {
            fold_u32(&mut h, 1);
            fold_vec(&mut h, normal);
        }
        GroundState::Sliding { normal } => {
            fold_u32(&mut h, 2);
            fold_vec(&mut h, normal);
        }
    }
    match state.run {
        RunState::NotStarted => fold_u32(&mut h, 0),
        RunState::Running { started_at_ms } => {
            fold_u32(&mut h, 1);
            fold_u32(&mut h, started_at_ms);
        }
        RunState::Finished {
            started_at_ms,
            finished_at_ms,
        } => {
            fold_u32(&mut h, 2);
            fold_u32(&mut h, started_at_ms);
            fold_u32(&mut h, finished_at_ms);
        }
    }
    fold_u32(&mut h, state.tick);
    fold_u32(&mut h, state.time_ms);
    h
}

// ═══ the command program ═══════════════════════════════════════════════════

/// Command duration: 125 Hz, the rate spec D2 defaults to and the one the
/// client runs at.
const MS: u16 = 8;

/// How many commands each case runs. 1.92 seconds of play — long enough for
/// several jumps, a full crossing of every feature in every world, and twenty
/// strafe half-beats.
const COMMANDS: u32 = 500;

/// Commands at which a golden digest is stored. Dense where divergences start.
const CHECKPOINTS: &[u32] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 24, 32, 48, 64, 96, 128, 192, 256, 360,
    500,
];

/// Half-width of the yaw sweep, in degrees.
const YAW_SWING: i32 = 30;

/// Commands in one strafe beat: the yaw sweeps one way for this many commands
/// and back for the next, and the strafe key flips at each turning point.
const BEAT: u32 = 12;

/// Build command `i`, given whether the player is on walkable ground.
///
/// A strafe-turn program in the shape of `tools/det-runner`'s, with one change:
/// its yaw *steps* monotonically (`Yaw::Step(0.37)`), which is right for a
/// determinism reference that only needs the arithmetic exercised, and wrong
/// here — a monotonic yaw turns the player away from the feature the world was
/// built around, and a freeze over a player wandering off the ramp is a freeze
/// over nothing.
///
/// So the yaw is a triangle wave: −30° to +30° and back over 24 commands, with
/// the strafe key flipping at each turning point. That is a player half-beating
/// a strafe jump, it keeps the net heading along +X, and it exercises the same
/// arithmetic.
///
/// `jump` is per case, and it is not a decoration. A bunny-hopping player is
/// airborne on almost every command, which means a jumping case barely
/// exercises `walk_move`, ground friction, the ground-plane rescale or
/// `step_slide_move`'s climb — the first draft of the step case jumped clean
/// over its own 18-unit step, and the perturbation test below caught it by
/// showing that changing `step_height` altered nothing about the run. A walking
/// case is a different regime, not a slower one.
fn cmd_for(i: u32, grounded: bool, yaw_offset: Scalar, jump: bool) -> UserCmd {
    let phase = i % (BEAT * 2);
    // 0 → BEAT → 0, so the yaw ramps up and back down.
    let up = if phase < BEAT {
        phase as i32
    } else {
        (BEAT * 2 - phase) as i32
    };
    let yaw = -YAW_SWING + (2 * YAW_SWING * up) / BEAT as i32;

    UserCmd {
        duration_ms: MS,
        forward_move: 127,
        right_move: if phase < BEAT { 127 } else { -127 },
        up_move: 0,
        buttons: if grounded && jump {
            Buttons::JUMP
        } else {
            Buttons::NONE
        },
        view: ViewAngles::from_degrees(s(0.0), s(yaw as f32) + yaw_offset, s(0.0)),
    }
}

// ═══ the cases ═════════════════════════════════════════════════════════════

/// Which world a case runs in. An enum rather than a boxed world in the table
/// so the case list stays a `const` a reviewer can read.
#[derive(Clone, Copy)]
enum Arena {
    /// `straf3_sim::world::FlatGround` — the analytic plane the existing
    /// technique tests are written against.
    Flat,
    /// `testbed::ramp`, at an angle the player can walk up.
    RampWalkable,
    /// `testbed::ramp`, at an angle too steep to stand on.
    RampSliding,
    /// `testbed::ramp`, at the walk/slide boundary itself.
    ///
    /// Here because the perturbation test below found this gate blind without
    /// it: with only a 26-degree and a 50-degree ramp, moving
    /// `min_walk_normal` from 0.7 to 0.71 changed nothing in any of the twelve
    /// original cases, because neither ramp is anywhere near the boundary. The
    /// constant governs *where the boundary is*, and only a ramp beside it can
    /// see the boundary move.
    RampBoundary,
    /// `testbed::step(18)` — exactly `STEPSIZE`.
    Step18,
    /// `testbed::corner()` — two walls and a crease.
    Corner,
    /// `testbed::drop_from(256)` — run off a platform and land.
    Drop256,
}

/// A walkable ramp: `cos(26°) = 0.899`, comfortably above `min_walk_normal`.
const WALKABLE_DEGREES: Scalar = s(26.0);
/// A sliding ramp: `cos(50°) = 0.643`, below `min_walk_normal` of 0.7.
const SLIDING_DEGREES: Scalar = s(50.0);
/// The knife edge: `cos(45°) = 0.7071`, barely above `min_walk_normal`.
///
/// `min_walk_normal` of 0.7 is 45.57 degrees, so 45 is walkable by four tenths
/// of a degree. Nudge the constant to 0.71 and this ramp becomes a slide, which
/// is exactly the sensitivity a freeze over the ramp rule needs to have.
const BOUNDARY_DEGREES: Scalar = s(45.0);

struct Case {
    name: &'static str,
    vq3: bool,
    arena: Arena,
    /// Where the player starts. Chosen so the run meets the world's feature
    /// rather than starting on top of it.
    spawn: Vec3,
    /// Added to every command's yaw, so a case can be aimed somewhere other
    /// than +X without changing the program.
    yaw_offset: Scalar,
    /// Whether the player bunny-hops. `false` puts the case in the walking
    /// regime, where `walk_move`, ground friction and step-up actually run.
    jump: bool,
    /// Rolling digest folded over the per-command movement digests.
    rolling: u64,
    /// Movement digest after each command in [`CHECKPOINTS`], in that order.
    checkpoints: &'static [u64],
}

fn arena_world(arena: Arena) -> Box<dyn World> {
    match arena {
        Arena::Flat => Box::new(FlatGround::at(s(0.0))),
        Arena::RampWalkable => Box::new(testbed::ramp(WALKABLE_DEGREES)),
        Arena::RampSliding => Box::new(testbed::ramp(SLIDING_DEGREES)),
        Arena::RampBoundary => Box::new(testbed::ramp(BOUNDARY_DEGREES)),
        Arena::Step18 => Box::new(testbed::step(s(18.0))),
        Arena::Corner => Box::new(testbed::corner()),
        Arena::Drop256 => Box::new(testbed::drop_from(s(256.0))),
    }
}

/// What one case produced.
struct Ran {
    /// Movement digest after every command, indexed from command 1.
    per_command: Vec<u64>,
    rolling: u64,
    end: SimState,
    /// Highest horizontal speed seen at any command boundary.
    peak_speed: Scalar,
    /// Whether the player was ever on a plane too steep to stand on.
    saw_sliding: bool,
    /// Lowest origin height seen at any command boundary.
    min_z: Scalar,
}

/// The profile a case is frozen under.
fn profile_for(case: &Case) -> PhysicsProfile {
    if case.vq3 {
        PhysicsProfile::vq3()
    } else {
        PhysicsProfile::cpm()
    }
}

fn run_case(case: &Case) -> Ran {
    run_under(case, &profile_for(case))
}

/// Run a case under an arbitrary profile.
///
/// The override exists for one test — the sensitivity check below — and is
/// worth the parameter: it is how this file demonstrates that it would *catch*
/// a canon change, without any test in the suite ever mutating canon.
fn run_under(case: &Case, profile: &PhysicsProfile) -> Ran {
    let world = arena_world(case.arena);

    let mut state = SimState::spawned_at(case.spawn, s(0.0));
    let mut per_command = Vec::with_capacity(COMMANDS as usize);
    let mut rolling = FNV_OFFSET;
    let mut peak_speed = s(0.0);
    let mut saw_sliding = false;
    let mut min_z = state.player.origin.z;

    for i in 0..COMMANDS {
        let grounded = state.player.ground.is_grounded();
        let cmd = cmd_for(i, grounded, case.yaw_offset, case.jump);
        state = step(&state, &cmd, world.as_ref(), profile);

        let digest = movement_digest(&state);
        per_command.push(digest);
        for b in digest.to_le_bytes() {
            rolling ^= u64::from(b);
            rolling = rolling.wrapping_mul(FNV_PRIME);
        }

        let v = state.player.velocity;
        peak_speed = peak_speed.max((v.x * v.x + v.y * v.y).sqrt());
        saw_sliding |= matches!(state.player.ground, GroundState::Sliding { .. });
        min_z = min_z.min(state.player.origin.z);
    }

    Ran {
        per_command,
        rolling,
        end: state,
        peak_speed,
        saw_sliding,
        min_z,
    }
}

// ═══ the goldens ═══════════════════════════════════════════════════════════
//
// Captured at commit 102894d, pre-wave HEAD. Do not regenerate to make a test
// pass; see the module docs.

const CASES: &[Case] = &[
    Case {
        name: "vq3-flat",
        vq3: true,
        arena: Arena::Flat,
        spawn: vec3(s(0.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0xdf0640946d0b88bf,
        checkpoints: &[
            0x9d83dd6092781600,
            0x6ffc67dc26822b95,
            0xa92b8d93ff11f3c0,
            0x284dd0c9ba574eca,
            0xe9bfdbed0930a0ed,
            0x30a57b65a17c1fd3,
            0x260d2573b19a39c2,
            0x73d6aed28e55b4c1,
            0xc6ef4e8cf365e8bb,
            0xd69073981772be11,
            0x0c5912201fd09e2f,
            0xe1130aece365a496,
            0x006ed5952c1347e0,
            0x7b6bb52f11212561,
            0x4d9c0b1d89e9bd9d,
            0xa8bf03da3068c03b,
            0xd3744829d26e9b4a,
            0x535beed652f7fc6e,
            0x0bdf7c3be9e14490,
            0x23c19ec92253e29d,
            0x4ad7fbaa7fd4619f,
            0x82e81b7019303ca3,
            0x67004ceff3b76f50,
            0x9882a4b7322b8fa7,
            0xd4b9c7ec56f3321b,
            0x404b06f782fd5b03,
        ],
    },
    Case {
        name: "cpm-flat",
        vq3: false,
        arena: Arena::Flat,
        spawn: vec3(s(0.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0x8a3b1d80226be3b4,
        checkpoints: &[
            0x9d83dd6092781600,
            0x6ffc67dc26822b95,
            0xa92b8d93ff11f3c0,
            0x284dd0c9ba574eca,
            0xe9bfdbed0930a0ed,
            0x30a57b65a17c1fd3,
            0x260d2573b19a39c2,
            0x73d6aed28e55b4c1,
            0xc6ef4e8cf365e8bb,
            0xd69073981772be11,
            0x0c5912201fd09e2f,
            0xe1130aece365a496,
            0xa05f9f3ade68a2b9,
            0x89294b4b2d106596,
            0x1462a55362ba125a,
            0x04860c8060261539,
            0xa25e6aa18f115350,
            0xcbaf370e9e2751c9,
            0xfd6bacc5ec91ae65,
            0xe18205d44783a657,
            0x03d9f74a9adddcbe,
            0x0493292eed5842b2,
            0x7465385d3e5ea89b,
            0xd1b6c61beaf3e8e3,
            0x6ea1397ed3ed1fcf,
            0xb4a3eb334765cf94,
        ],
    },
    Case {
        name: "vq3-ramp-26",
        vq3: true,
        arena: Arena::RampWalkable,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0x53fb447fa9c2aaf2,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4dc03b19d7c41b86,
            0xaf06b29c99aaa4c1,
            0x288a1267b1b82751,
            0xd82e67db3d9491c3,
            0x6df695ecbc8b5d7b,
            0x10f7a9d0a04717db,
            0x66463ff23c7e42a3,
            0x0b3ad7a3a3ff257c,
            0x808f50ab9d62fe53,
            0x3f37df9b2bf90d84,
            0x7a61c4567b4951ca,
            0xe54cf6da06bf1435,
            0x1835ba972a60d8e1,
            0xca59af5337ef91ae,
        ],
    },
    Case {
        name: "cpm-ramp-26",
        vq3: false,
        arena: Arena::RampWalkable,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0xf41bc4be6995287e,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4e86cf1380cf72c3,
            0xd01dd7cfda8c2dcb,
            0xc643ca117998d8e2,
            0x6418c00646290662,
            0xf12a1da74941aca5,
            0x5f1b8cf5a3b86779,
            0xe951885b908c3006,
            0xbf5e4e8d081426ea,
            0x2abee95ccec12578,
            0x4a37d2dd5787e790,
            0x78dde82555eed7cb,
            0xc106d86f73679cc9,
            0x1b2af2e855c26648,
            0x67dcd15650587eab,
        ],
    },
    Case {
        name: "vq3-ramp-50",
        vq3: true,
        arena: Arena::RampSliding,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0x2c475b50a8154840,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4dc03b19d7c41b86,
            0xaf06b29c99aaa4c1,
            0x288a1267b1b82751,
            0xd82e67db3d9491c3,
            0x6df695ecbc8b5d7b,
            0x10f7a9d0a04717db,
            0x66463ff23c7e42a3,
            0x0b3ad7a3a3ff257c,
            0x808f50ab9d62fe53,
            0x3f37df9b2bf90d84,
            0x7a61c4567b4951ca,
            0x134870ea9cdeebb1,
            0xaaac455515590ef2,
            0x2e259df6ca334991,
        ],
    },
    Case {
        name: "cpm-ramp-50",
        vq3: false,
        arena: Arena::RampSliding,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0xeab2a007164901cf,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4e86cf1380cf72c3,
            0xd01dd7cfda8c2dcb,
            0xc643ca117998d8e2,
            0x6418c00646290662,
            0xf12a1da74941aca5,
            0x5f1b8cf5a3b86779,
            0xe951885b908c3006,
            0xbf5e4e8d081426ea,
            0x2abee95ccec12578,
            0x4a37d2dd5787e790,
            0x78dde82555eed7cb,
            0x6963c584b1c1881e,
            0x1ff41b62ce677cd7,
            0xdf4b4e36f311fcd0,
        ],
    },
    Case {
        name: "vq3-ramp-45",
        vq3: true,
        arena: Arena::RampBoundary,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0xf8701d08e7109aad,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4dc03b19d7c41b86,
            0xaf06b29c99aaa4c1,
            0x288a1267b1b82751,
            0xd82e67db3d9491c3,
            0x6df695ecbc8b5d7b,
            0x10f7a9d0a04717db,
            0x66463ff23c7e42a3,
            0x0b3ad7a3a3ff257c,
            0x808f50ab9d62fe53,
            0x3f37df9b2bf90d84,
            0x7a61c4567b4951ca,
            0xa98fad99613f23fb,
            0xc5e15433a44f5db2,
            0xe07ac5cbb282b9c5,
        ],
    },
    Case {
        name: "cpm-ramp-45",
        vq3: false,
        arena: Arena::RampBoundary,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0x7717551322f63675,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4e86cf1380cf72c3,
            0xd01dd7cfda8c2dcb,
            0xc643ca117998d8e2,
            0x6418c00646290662,
            0xf12a1da74941aca5,
            0x5f1b8cf5a3b86779,
            0xe951885b908c3006,
            0xbf5e4e8d081426ea,
            0x2abee95ccec12578,
            0x4a37d2dd5787e790,
            0x78dde82555eed7cb,
            0x059e245845afd387,
            0xcfc5f4440b8b707e,
            0x44526751228d00d7,
        ],
    },
    Case {
        name: "vq3-step-18",
        vq3: true,
        arena: Arena::Step18,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: false,
        rolling: 0xe2766b391b4c3726,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4dc03b19d7c41b86,
            0xaf06b29c99aaa4c1,
            0x288a1267b1b82751,
            0xd82e67db3d9491c3,
            0x6df695ecbc8b5d7b,
            0x10f7a9d0a04717db,
            0x9dc3348133536f63,
            0xb0b858a110b342c4,
            0xc609d99b96506066,
            0x99dbdf33eb0006fe,
            0x374bbd36314ae54e,
            0xd1c9902bf83387a5,
            0x9e1f7514690c1af1,
            0xdfd0eeb8a372c533,
        ],
    },
    Case {
        name: "cpm-step-18",
        vq3: false,
        arena: Arena::Step18,
        spawn: vec3(s(-256.0), s(0.0), s(64.0)),
        yaw_offset: s(0.0),
        jump: false,
        rolling: 0x79f60e892346f8d8,
        checkpoints: &[
            0x48fb1b014c87652a,
            0x9297466cb86b0413,
            0xa5128e62c4b72c90,
            0x56dd14ca6f43add3,
            0xca4118877860fc68,
            0xc0950d50d62697fc,
            0x3e1d903788936a74,
            0x4c25613cc93e9762,
            0xa00aa67dbedb726f,
            0xcd1ec81213bc0a04,
            0x392423d6fb09cebb,
            0x844bcfefd6a495b4,
            0x4e86cf1380cf72c3,
            0xd01dd7cfda8c2dcb,
            0xc643ca117998d8e2,
            0x6418c00646290662,
            0xf12a1da74941aca5,
            0x5f1b8cf5a3b86779,
            0xfc8ed9dc0869b9ee,
            0x1aa6115f9b62d849,
            0x4748d716af4cc409,
            0x0ef9d663d1cd4c5e,
            0x72c8d956122ac785,
            0xdf7742374dbb79b3,
            0x46cbbdc6eac49109,
            0x8b93685aa4af8c5e,
        ],
    },
    Case {
        name: "vq3-corner",
        vq3: true,
        arena: Arena::Corner,
        spawn: vec3(s(-256.0), s(-256.0), s(64.0)),
        yaw_offset: s(45.0),
        jump: true,
        rolling: 0x5a2ace07137879bc,
        checkpoints: &[
            0x51b6f83c468bf120,
            0x18497f1e70b999cc,
            0x1825b75e6b866f83,
            0x00468ceb01e2862b,
            0xedadb60d56a71ab1,
            0x5343a3c8614cce60,
            0x0cefd090b565d731,
            0x9b7e2b0b2a8c37ed,
            0xf3d438d95a228057,
            0x8c07b498fa974ad9,
            0xf24e22a2cd1ee8e3,
            0x311bed79d363ea37,
            0x89e7f78bbe61d398,
            0x6feda0246ba010e2,
            0xea6ca1120ba2a627,
            0x629cdd0988d421e3,
            0x98618758613a9c68,
            0x25961dff15e4903d,
            0x7df35a1f00aab864,
            0x6fc2553a7bd87fce,
            0x058c3af0b6c396a5,
            0x7001239ad2ddac9a,
            0x8b4afa73060e6d6a,
            0x08af4c4b8300d3f2,
            0x6ed5a37708a0aaa2,
            0x1a9d15594745501b,
        ],
    },
    Case {
        name: "cpm-corner",
        vq3: false,
        arena: Arena::Corner,
        spawn: vec3(s(-256.0), s(-256.0), s(64.0)),
        yaw_offset: s(45.0),
        jump: true,
        rolling: 0xb47805ce963d7e50,
        checkpoints: &[
            0x51b6f83c468bf120,
            0x18497f1e70b999cc,
            0x1825b75e6b866f83,
            0x00468ceb01e2862b,
            0xedadb60d56a71ab1,
            0x5343a3c8614cce60,
            0x0cefd090b565d731,
            0x9b7e2b0b2a8c37ed,
            0xf3d438d95a228057,
            0x8c07b498fa974ad9,
            0xf24e22a2cd1ee8e3,
            0x311bed79d363ea37,
            0xc8db15aeba644abf,
            0x69a13c794912d84d,
            0xf9bcbe2ab983e2d5,
            0xf22471f433f5cba4,
            0xc4df940b3fc7d0b3,
            0xf2d7d22d226ea245,
            0x2a14a5f5413f1564,
            0x602dc8a364bb82c2,
            0x2d1b880451189095,
            0x3651df0b1681a272,
            0xb7150bff5dea52c8,
            0xc81cb607e15b1f0c,
            0xdea7e0ee4b9aea48,
            0x019ee8918dbd2413,
        ],
    },
    Case {
        name: "vq3-drop-256",
        vq3: true,
        arena: Arena::Drop256,
        spawn: vec3(s(-256.0), s(0.0), s(320.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0x62f973b7662fbab4,
        checkpoints: &[
            0x779e5271e063be6a,
            0x052850fa8646cd27,
            0x63653a917191acad,
            0x75abe3f580af13e0,
            0x901a06539692cd53,
            0x319a27a9e2e7c683,
            0x81d10fd09cf1aee8,
            0x1fed0223e74be90d,
            0x4b92fbd6f21f38e5,
            0xf152e7c88d26a93b,
            0x1684b859c8ae0de8,
            0x26c71c1cb44d599b,
            0x5beccf88f78bc4cb,
            0x668f7d74d8e36e45,
            0x23353d2377fb32e4,
            0x5878340b3381be02,
            0x849c6446fcce6b39,
            0x7834eec3c76ebbd7,
            0xc11c95d59f60cae5,
            0x3c1d5526e895d4ed,
            0x81fec574234267e6,
            0x8cb720d311fc1f8a,
            0x038f4da9b98fd88c,
            0xf56ccbc293bc4f85,
            0xa61dfc7b3a4284f7,
            0x64becaa427d4cffd,
        ],
    },
    Case {
        name: "cpm-drop-256",
        vq3: false,
        arena: Arena::Drop256,
        spawn: vec3(s(-256.0), s(0.0), s(320.0)),
        yaw_offset: s(0.0),
        jump: true,
        rolling: 0x39686ae85ac41162,
        checkpoints: &[
            0x779e5271e063be6a,
            0x052850fa8646cd27,
            0x63653a917191acad,
            0x75abe3f580af13e0,
            0x901a06539692cd53,
            0x319a27a9e2e7c683,
            0x81d10fd09cf1aee8,
            0x1fed0223e74be90d,
            0x4b92fbd6f21f38e5,
            0xf152e7c88d26a93b,
            0x1684b859c8ae0de8,
            0x26c71c1cb44d599b,
            0x4f640441904d64aa,
            0x2be67f1765e43023,
            0xac74b5be67e6df57,
            0x27e53efffdcaf073,
            0x0f1b66dd96bc1fcb,
            0xe3711b9b8742f131,
            0x00b22f7da7f11c7c,
            0xbfa12d5191b6e2ff,
            0x0d6f8a0a890e4671,
            0x05257beb764f7119,
            0xdbe36cb1c396fab5,
            0xeea38a59ccff0b86,
            0x98ecb95fda606069,
            0xe61dbc94b833b45f,
        ],
    },
];

// ═══ the gate ══════════════════════════════════════════════════════════════

/// Re-run a case for `commands` commands and return the state it reached.
///
/// Only used to describe a failure. Re-running is cheaper than carrying five
/// hundred `SimState`s per case through the happy path, and a freeze that is
/// slow to pass is a freeze that gets `#[ignore]`d.
fn state_after(case: &Case, commands: u32) -> SimState {
    let world = arena_world(case.arena);
    let profile = profile_for(case);
    let mut state = SimState::spawned_at(case.spawn, s(0.0));
    for i in 0..commands {
        let grounded = state.player.ground.is_grounded();
        state = step(
            &state,
            &cmd_for(i, grounded, case.yaw_offset, case.jump),
            world.as_ref(),
            &profile,
        );
    }
    state
}

/// Say, in as much detail as the ladder allows, *what* moved.
fn describe_divergence(case: &Case, ran: &Ran) -> String {
    let mut last_match: Option<u32> = None;
    for (slot, &at) in CHECKPOINTS.iter().enumerate() {
        let actual = ran.per_command[(at - 1) as usize];
        let golden = case.checkpoints[slot];
        if actual == golden {
            last_match = Some(at);
            continue;
        }
        let st = state_after(case, at);
        return format!(
            "case `{}` diverged from frozen canon.\n\
             \x20 first differing checkpoint: command {at}\n\
             \x20   expected digest 0x{golden:016x}\n\
             \x20   actual   digest 0x{actual:016x}\n\
             \x20 last matching checkpoint:   {}\n\
             \x20 observable state at command {at}:\n\
             \x20   origin   {:?}\n\
             \x20   velocity {:?}\n\
             \x20   ground   {:?}\n\
             \x20   time_ms  {}\n\
             The divergence began somewhere in commands {}..={at}.",
            case.name,
            match last_match {
                Some(c) => format!("command {c}"),
                None => "none — the very first command already differs".to_string(),
            },
            st.player.origin,
            st.player.velocity,
            st.player.ground,
            st.time_ms,
            last_match.map_or(1, |c| c + 1),
        );
    }
    format!(
        "case `{}` has a rolling digest of 0x{:016x} where 0x{:016x} was frozen, \
         yet every checkpoint on the ladder matches. The divergence is at a command \
         the ladder does not sample — see CHECKPOINTS.",
        case.name, ran.rolling, case.rolling,
    )
}

/// **The gate.** Spec criterion 4, as decision D9.
///
/// `vq3` and `cpm` produce, command for command, exactly the run they produced
/// at commit 102894d, in six worlds including four made of brushes.
#[test]
fn canon_movement_is_frozen() {
    for case in CASES {
        let ran = run_case(case);
        assert_eq!(
            ran.rolling,
            case.rolling,
            "\n{}\n",
            describe_divergence(case, &ran)
        );
        // The ladder is checked too, not merely used for diagnosis: the rolling
        // digest could in principle collide, and a checkpoint that has silently
        // gone stale is a checkpoint that cannot describe the next failure.
        for (slot, &at) in CHECKPOINTS.iter().enumerate() {
            assert_eq!(
                ran.per_command[(at - 1) as usize],
                case.checkpoints[slot],
                "case `{}`, checkpoint at command {at}",
                case.name
            );
        }
    }
}

/// A freeze over a player standing still passes forever and proves nothing.
///
/// `det-runner` makes the same check for the same reason. Here it has to be
/// stronger than "did they move": the first version of this fixture spawned the
/// player 512 units from each feature and ran for 240 commands, and the player
/// never got there — `ramp(26)`, `ramp(50)` and `step(18)` all produced *the
/// same digest*, because all three runs happened entirely on flat approach
/// floor. It passed. It froze nothing but `FlatGround` under three names.
///
/// So each case asserts it reached the thing its world was built around.
#[test]
fn every_case_reaches_the_feature_its_world_was_built_around() {
    for case in CASES {
        let ran = run_case(case);
        let end = ran.end.player.origin;
        let travelled = (end - case.spawn).truncate().length();

        assert!(
            ran.peak_speed > s(300.0),
            "case `{}` never got above {} ups; the strafe program is not strafing",
            case.name,
            ran.peak_speed
        );

        match case.arena {
            Arena::Flat => assert!(
                travelled > s(1000.0),
                "case `{}` covered only {travelled} units of open ground",
                case.name
            ),
            Arena::RampWalkable => {
                assert!(
                    end.x > s(0.0) && end.z > s(200.0),
                    "case `{}` ended at {end:?}; it never climbed the walkable ramp",
                    case.name
                );
                assert!(
                    !ran.saw_sliding,
                    "case `{}` slid on a ramp whose normal.z is {} — above min_walk_normal",
                    case.name,
                    testbed::ramp_normal(WALKABLE_DEGREES).z
                );
            }
            Arena::RampSliding => assert!(
                ran.saw_sliding,
                "case `{}` never entered GroundState::Sliding, so the {}-degree ramp \
                 was never actually met",
                case.name, SLIDING_DEGREES
            ),
            Arena::RampBoundary => {
                assert!(
                    end.x > s(0.0),
                    "case `{}` ended at {end:?}; it never reached the boundary ramp",
                    case.name
                );
                assert!(
                    !ran.saw_sliding,
                    "case `{}` slid on a 45-degree ramp, whose normal.z is {} — above                      min_walk_normal of 0.7. That is the boundary this case exists to                      pin, and it has moved.",
                    case.name,
                    testbed::ramp_normal(BOUNDARY_DEGREES).z
                );
            }
            Arena::Step18 => assert!(
                end.x > testbed::STEP_RISER_X && end.z > s(40.0),
                "case `{}` ended at {end:?}; it never got up the 18-unit step",
                case.name
            ),
            Arena::Corner => assert!(
                end.x > s(40.0) && end.y > s(40.0),
                "case `{}` ended at {end:?}; it never reached the inside corner at \
                 ({inner}, {inner})",
                case.name,
                inner = testbed::CORNER_INNER
            ),
            Arena::Drop256 => assert!(
                end.x > testbed::PLATFORM_EDGE_X && ran.min_z < s(120.0),
                "case `{}` ended at {end:?} having got no lower than {}; it never ran \
                 off the platform",
                case.name,
                ran.min_z
            ),
        }
    }
}

/// Twelve cases, twelve runs — no two of them the same simulation.
///
/// This is the check that would have caught the mis-spawned first draft on its
/// own, without anyone reading a position. A freeze whose cases collide is a
/// freeze with fewer cases than it claims.
#[test]
fn no_two_cases_are_the_same_run() {
    for (i, a) in CASES.iter().enumerate() {
        for b in &CASES[i + 1..] {
            assert_ne!(
                a.rolling, b.rolling,
                "cases `{}` and `{}` are the same run",
                a.name, b.name
            );
        }
    }
}

/// The other half of the canon gate: the *profiles* must not have grown a
/// mechanic that is on by default.
///
/// The freeze above proves canon movement is unchanged **given the profiles as
/// they are today**. It cannot prove that a field added to `PhysicsProfile` for
/// `experimental` was left at a disabling value in `vq3` and `cpm` — a field
/// that is on in canon would simply be part of the new canon, and the goldens
/// would be regenerated to suit.
///
/// So the destructure below is **exhaustive on purpose**: no `..` rest pattern.
/// Adding a field to `PhysicsProfile` fails to compile *here*, and the failure
/// lands on a comment telling the author what to do about it. This is the same
/// device `straf3-replay`'s `physics_digest` uses, chosen for the same reason:
/// a compile error is the only reminder that cannot be forgotten.
#[test]
fn a_new_profile_field_must_declare_its_canon_value() {
    for (label, p) in [
        ("vq3", PhysicsProfile::vq3()),
        ("cpm", PhysicsProfile::cpm()),
    ] {
        let PhysicsProfile {
            // ── canon, as of the pre-wave tree. `profile.rs` pins these
            //    numerically; the freeze above pins what they do. Nothing to
            //    assert here.
            accelerate: _,
            friction: _,
            stop_speed: _,
            max_speed: _,
            duck_scale: _,
            air_accelerate: _,
            gravity: _,
            jump_velocity: _,
            step_height: _,
            overclip: _,
            max_clip_planes: _,
            ground_trace_probe: _,
            min_walk_normal: _,
            hull_mins: _,
            hull_maxs: _,
            crouched_height: _,
            air_control,
            air_stop_accelerate,
            strafe_accelerate,
            strafe_wish_speed_cap,
            double_jump_window_ms,
            double_jump_boost,
            // ── CANDIDATE MECHANICS LAND HERE ──────────────────────────────
            //
            // If you are reading this because the compiler sent you: you added
            // a field to `PhysicsProfile`. Bind it above and add it to
            // `candidate_fields` below, so this test asserts it is switched off
            // in both canon profiles. A candidate mechanic that is non-zero in
            // `vq3` or `cpm` is not a candidate, it is a change to canon, and
            // this wave does not permit one.
            slide_entry_speed,
            slide_friction,
            slide_duration_ms,
            dash_speed,
            dash_window_ms,
            wall_jump_velocity,
            wall_contact_window_ms,
            wall_normal_max,
        } = p;

        // Every field a candidate mechanic adds, paired with the value that
        // means "off". The integer windows are widened to `Scalar` rather than
        // given a second list: they are milliseconds, exactly representable,
        // and one list a reader can scan beats two that must be kept in step.
        let candidate_fields: &[(&str, Scalar)] = &[
            // ── crouch slide ──────────────────────────────────────────────
            ("slide_entry_speed", slide_entry_speed),
            ("slide_friction", slide_friction),
            ("slide_duration_ms", s(slide_duration_ms as f32)),
            // ── dash ──────────────────────────────────────────────────────
            ("dash_speed", dash_speed),
            ("dash_window_ms", s(dash_window_ms as f32)),
            // ── wall interaction ──────────────────────────────────────────
            ("wall_jump_velocity", wall_jump_velocity),
            ("wall_contact_window_ms", s(wall_contact_window_ms as f32)),
            // A threshold, not an on/off number — zero is a meaningful value
            // for it ("only vertical or overhanging planes count as walls"),
            // so the mechanic is gated on the two effect constants above and
            // this is read only when they are non-zero. It is listed anyway
            // because canon does set it to zero and a future edit that did not
            // should have to argue for itself here. See the field's own doc
            // comment for why this is consistent with the data-not-branches
            // doctrine rather than an exception to it — `strafe_wish_speed_cap`
            // is the same shape and predates it.
            ("wall_normal_max", wall_normal_max),
        ];
        for (name, value) in candidate_fields {
            assert_eq!(
                *value,
                s(0.0),
                "profile `{label}` has candidate mechanic `{name}` switched on; \
                 canon may not carry one"
            );
        }

        // Not vacuous in the meantime: VQ3 is CPM with the pre-wave extensions
        // zeroed, and that identity is what makes "a zero disables it" a real
        // rule rather than a habit. If a future extension cannot be expressed
        // that way, it needs an argument before it needs a field.
        if label == "vq3" {
            assert_eq!(air_control, s(0.0));
            assert_eq!(air_stop_accelerate, s(0.0));
            assert_eq!(strafe_accelerate, s(0.0));
            assert_eq!(strafe_wish_speed_cap, s(0.0));
            assert_eq!(double_jump_window_ms, 0);
            assert_eq!(double_jump_boost, s(0.0));
        }
    }
}

/// The digest itself, checked against the two ways it could be wrong: blind to
/// a change it must see, or sensitive to one it must not.
#[test]
fn the_movement_digest_sees_movement_and_nothing_else() {
    let base = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));

    // Sees: one bit of origin.
    let mut moved = base;
    moved.player.origin.x = Scalar::from_bits(to_bits(base.player.origin.x) + 1);
    assert_ne!(movement_digest(&base), movement_digest(&moved));

    // Sees: signed zero, the divergence that is invisible to `==`.
    let mut negative = base;
    negative.player.velocity.x = s(-0.0);
    assert_eq!(base.player.velocity.x, negative.player.velocity.x);
    assert_ne!(movement_digest(&base), movement_digest(&negative));

    // Sees: standing on a plane versus sliding down the same plane.
    let n = vec3(s(0.0), s(0.6), s(0.8));
    let mut grounded = base;
    grounded.player.ground = GroundState::Grounded { normal: n };
    let mut sliding = base;
    sliding.player.ground = GroundState::Sliding { normal: n };
    assert_ne!(movement_digest(&grounded), movement_digest(&sliding));

    // Sees: the clock.
    let mut running = base;
    running.run = RunState::Running { started_at_ms: 40 };
    assert_ne!(movement_digest(&base), movement_digest(&running));

    // Does not see: bookkeeping whose representation may grow this wave. This
    // is the whole reason the gate is not `SimState::checksum()`, so it is
    // asserted rather than left as an accident of which fields got folded.
    let mut timers = base;
    timers.player.timers.double_jump_ms = 400;
    timers.player.timers.since_landed_ms = 123;
    timers.player.jump_held = true;
    timers.player.left_ground_by_jumping = true;
    timers.player.crouched = true;
    timers.player.view = ViewAngles::from_degrees(s(0.0), s(90.0), s(0.0));
    assert_eq!(
        movement_digest(&base),
        movement_digest(&timers),
        "the movement digest is reading state a player cannot observe"
    );
    // …and `SimState::checksum` does see all of it, which is why the two exist.
    assert_ne!(base.checksum(), timers.checksum());
}

/// One named edit to a canon constant: a label and the profile it produces.
type Perturbation = (&'static str, fn(PhysicsProfile) -> PhysicsProfile);

/// Would this gate actually catch a canon change?
///
/// The question a freeze cannot leave unanswered, and it is not answerable by
/// the gate passing. So: run the same cases under profiles that differ from
/// canon by the smallest edit anyone could plausibly make, and require the
/// frozen digests to notice.
///
/// No test here mutates canon to find this out. `PhysicsProfile` is data — that
/// is the whole argument for it being a struct rather than a match arm — so a
/// perturbed profile is a value, not an edit to `profile.rs`.
///
/// The perturbations probe different parts of the pipeline:
///
/// - **one ulp of `overclip`** is the smallest representable change to the
///   constant behind ramp boost and overbounce. If a single-ulp edit slipped
///   past this gate, no larger one would be reliably caught either.
/// - **`step_height` 18 → 17.9** is invisible on flat ground and decisive on
///   `testbed::step(18)`, which is how this file proves the geometric cases
///   carry weight rather than merely existing.
/// - **`min_walk_normal` 0.7 → 0.71** moves the walk/slide boundary by a third
///   of a degree, the finest change to the ramp rule a tuning pass would make.
#[test]
fn a_minimal_change_to_canon_is_caught_by_the_frozen_digest() {
    let perturbations: &[Perturbation] = &[
        ("overclip +1ulp", |p| PhysicsProfile {
            overclip: Scalar::from_bits(to_bits(p.overclip) + 1),
            ..p
        }),
        ("step_height 17.9", |p| PhysicsProfile {
            step_height: s(17.9),
            ..p
        }),
        ("min_walk_normal 0.71", |p| PhysicsProfile {
            min_walk_normal: s(0.71),
            ..p
        }),
    ];

    for (label, perturb) in perturbations {
        let caught: Vec<&str> = CASES
            .iter()
            .filter(|case| run_under(case, &perturb(profile_for(case))).rolling != case.rolling)
            .map(|case| case.name)
            .collect();
        assert!(
            !caught.is_empty(),
            "`{label}` changed canon and not one of the {} frozen cases noticed",
            CASES.len()
        );
    }

    // And specifically: the geometric cases are what catch a geometric change.
    // A `step_height` edit is invisible on open ground, so a gate made of
    // `FlatGround` alone — which is what every pre-wave movement fixture
    // effectively was — would sail straight through it.
    let flat = CASES.iter().find(|c| c.name == "vq3-flat").expect("flat");
    let stepped = CASES
        .iter()
        .find(|c| c.name == "vq3-step-18")
        .expect("step");
    let shorter = |p: PhysicsProfile| PhysicsProfile {
        step_height: s(17.9),
        ..p
    };
    assert_eq!(
        run_under(flat, &shorter(profile_for(flat))).rolling,
        flat.rolling,
        "a step_height change should be invisible on flat ground"
    );
    assert_ne!(
        run_under(stepped, &shorter(profile_for(stepped))).rolling,
        stepped.rolling,
        "the step case did not notice a step_height change, so it is not covering \
         the step"
    );
}

//! Movement techniques: spec rev 3, acceptance criteria 1 and 2.
//!
//! # What these tests are for
//!
//! Criterion 2 asks that each documented technique have a test that **fails if
//! the behaviour regresses**. That is a stronger requirement than "a test
//! exists", and it shapes how these are written. Almost every technique test
//! below runs the same input twice — once under the profile that enables the
//! technique, once under a profile with exactly one constant zeroed — and
//! asserts the two disagree, *and* pins the absolute number the enabled run
//! produces.
//!
//! That pair catches both ways the behaviour can be lost:
//!
//! - delete the code path, and the two runs become equal, so the difference
//!   assertion fails;
//! - keep the path but get the arithmetic wrong, and the absolute assertion
//!   fails.
//!
//! A test that only checked "CPM is faster than VQ3" would survive a rewrite
//! that made CPM faster for the wrong reason. These do not.
//!
//! # The worlds
//!
//! `straf3-sim` ships `FlatGround` and `EmptyWorld`. Ramps, stairs and corners
//! are needed to exercise the slide solver, so they are defined here rather
//! than in the crate — a test world is a test fixture, and the collision seam
//! exists precisely so one can be written in a few dozen lines.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::state::GroundState;
use straf3_sim::world::{SurfaceFlags, Sweep, Trace, World};
use straf3_sim::{Buttons, PhysicsProfile, SimState, UserCmd, ViewAngles, run, step};

/// The command duration used throughout: 125 Hz, spec D2's default.
const MS: u16 = 8;
const DT: Scalar = 0.008;

// ═══ test worlds ═══════════════════════════════════════════════════════════

/// An infinite plane at any angle: the half-space `dot(p, normal) >= 0` is
/// free, everything below it is solid.
///
/// A ramp, in other words, with no edges to fall off — which is what a test of
/// slope behaviour wants, because it removes every question except the slope.
#[derive(Debug, Clone, Copy)]
struct SlopedGround {
    normal: Vec3,
    surface: SurfaceFlags,
}

impl SlopedGround {
    /// A plane through the world origin whose normal leans `degrees` away from
    /// straight up, tilting towards +X — so the surface *rises* with X.
    fn tilted(degrees: Scalar) -> Self {
        let r = degrees * s(core::f32::consts::PI / 180.0);
        Self {
            normal: vec3(-r.sin(), s(0.0), r.cos()),
            surface: SurfaceFlags::NONE,
        }
    }
}

impl World for SlopedGround {
    fn trace(&self, sweep: &Sweep) -> Trace {
        // Signed distance from the plane to the hull's deepest corner. For an
        // axis-aligned box that corner is found by walking each axis in the
        // direction that reduces the distance, which is `|n| . half_extents`.
        let reach = self.normal.abs().dot(sweep.half_extents);
        let depth = |p: Vec3| (p + sweep.center_offset).dot(self.normal) - reach;

        let d0 = depth(sweep.start);
        let d1 = depth(sweep.end);

        if d0 < s(0.0) {
            return Trace {
                fraction: s(0.0),
                normal: self.normal,
                surface: self.surface,
                start_solid: true,
                all_solid: d1 < s(0.0),
            };
        }
        if d1 >= s(0.0) {
            return Trace::clear();
        }
        Trace {
            fraction: (d0 / (d0 - d1)).clamp(s(0.0), s(1.0)),
            normal: self.normal,
            surface: self.surface,
            start_solid: false,
            all_solid: false,
        }
    }
}

/// A world made of axis-aligned solid boxes.
///
/// Enough to build a stair, a wall and an inside corner, which are the three
/// shapes the slide solver has anything interesting to say about.
#[derive(Debug, Clone)]
struct Boxes(Vec<(Vec3, Vec3)>);

impl Boxes {
    /// A floor at z=0 with a step of `height` starting at x=0.
    fn stair(height: Scalar) -> Self {
        Self(vec![
            (
                vec3(s(-4096.0), s(-4096.0), s(-512.0)),
                vec3(s(4096.0), s(4096.0), s(0.0)),
            ),
            (
                vec3(s(0.0), s(-4096.0), s(-512.0)),
                vec3(s(4096.0), s(4096.0), height),
            ),
        ])
    }

    /// A floor at z=0 and two walls meeting at an inside corner around
    /// (x=64, y=64), so a player driven into it meets two planes at once.
    fn corner() -> Self {
        Self(vec![
            (
                vec3(s(-4096.0), s(-4096.0), s(-512.0)),
                vec3(s(4096.0), s(4096.0), s(0.0)),
            ),
            (
                vec3(s(64.0), s(-4096.0), s(0.0)),
                vec3(s(4096.0), s(4096.0), s(512.0)),
            ),
            (
                vec3(s(-4096.0), s(64.0), s(0.0)),
                vec3(s(4096.0), s(4096.0), s(512.0)),
            ),
        ])
    }
}

impl World for Boxes {
    fn trace(&self, sweep: &Sweep) -> Trace {
        // Minkowski trick: grow each box by the hull's half extents and sweep
        // the hull's centre through the result as a point.
        let start = sweep.start + sweep.center_offset;
        let end = sweep.end + sweep.center_offset;
        let delta = end - start;

        let mut best = Trace::clear();
        let mut start_solid = false;
        let mut all_solid = false;

        for (bmins, bmaxs) in &self.0 {
            let lo = *bmins - sweep.half_extents;
            let hi = *bmaxs + sweep.half_extents;

            let inside = |p: Vec3| (0..3).all(|a| p[a] > lo[a] && p[a] < hi[a]);
            if inside(start) {
                start_solid = true;
                all_solid |= inside(end);
                continue;
            }

            let mut enter = s(0.0);
            let mut leave = s(1.0);
            let mut normal = Vec3::ZERO;
            let mut missed = false;

            for axis in 0..3 {
                if delta[axis].abs() < s(1e-9) {
                    if start[axis] < lo[axis] || start[axis] > hi[axis] {
                        missed = true;
                        break;
                    }
                    continue;
                }
                let mut t_lo = (lo[axis] - start[axis]) / delta[axis];
                let mut t_hi = (hi[axis] - start[axis]) / delta[axis];
                let mut sign = s(-1.0); // crossing the low face: normal points -axis
                if t_lo > t_hi {
                    core::mem::swap(&mut t_lo, &mut t_hi);
                    sign = s(1.0);
                }
                if t_lo > enter {
                    enter = t_lo;
                    normal = Vec3::ZERO;
                    normal[axis] = sign;
                }
                if t_hi < leave {
                    leave = t_hi;
                }
                if enter > leave {
                    missed = true;
                    break;
                }
            }

            if missed || enter >= s(1.0) || normal == Vec3::ZERO {
                continue;
            }
            if enter < best.fraction {
                best = Trace {
                    fraction: enter,
                    normal,
                    surface: SurfaceFlags::NONE,
                    start_solid: false,
                    all_solid: false,
                };
            }
        }

        if start_solid {
            return Trace {
                fraction: s(0.0),
                normal: Vec3::Z,
                surface: SurfaceFlags::NONE,
                start_solid: true,
                all_solid,
            };
        }
        best
    }
}

// ═══ harness ═══════════════════════════════════════════════════════════════

fn still() -> UserCmd {
    UserCmd::still(MS)
}

fn horizontal_speed(st: &SimState) -> Scalar {
    let v = st.player.velocity;
    (v.x * v.x + v.y * v.y).sqrt()
}

fn heading_degrees(st: &SimState) -> Scalar {
    let v = st.player.velocity;
    v.y.atan2(v.x) * s(180.0 / core::f32::consts::PI)
}

/// Drop the player onto `world` and wait for them to settle.
fn settle_on<W: World>(world: &W, profile: &PhysicsProfile, from: Vec3) -> SimState {
    let spawn = SimState::spawned_at(from, s(0.0));
    let mut st = run(&spawn, &vec![still(); 400], world, profile);
    st.player.velocity = Vec3::ZERO;
    st
}

/// Which command axis the "perfect strafer" below holds.
#[derive(Clone, Copy, PartialEq)]
enum Axis {
    /// Forward only — Quake's `movementDir` 0, the axis VQ3 strafejumps on and
    /// the only one CPM grants air control to.
    Forward,
    /// Right only — `movementDir` 6, the axis CPM's strafe acceleration model
    /// is written for.
    Strafe,
}

/// A player who turns perfectly: every command, the view is set so that the
/// wish direction sits exactly `angle` degrees off the current velocity.
///
/// This is the technique itself, expressed as a function. A human does this
/// with a mouse and does it imperfectly; holding the angle exactly is what
/// makes the result a property of the physics rather than of the input.
fn perfect_strafe<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    start: &SimState,
    angle: Scalar,
    axis: Axis,
    commands: usize,
) -> SimState {
    let mut st = *start;
    for _ in 0..commands {
        let want = heading_degrees(&st) + angle;
        // `forward` points along yaw; `right` points 90 degrees clockwise of
        // it (Quake's right is -Y at yaw 0), so aiming the two at the same
        // world direction needs different yaws.
        let yaw = match axis {
            Axis::Forward => want,
            Axis::Strafe => want + s(90.0),
        };
        let cmd = UserCmd {
            forward_move: if axis == Axis::Forward { 127 } else { 0 },
            right_move: if axis == Axis::Strafe { 127 } else { 0 },
            view: ViewAngles {
                pitch: s(0.0),
                yaw,
                roll: s(0.0),
            },
            ..still()
        };
        st = step(&st, &cmd, world, profile);
    }
    st
}

/// An airborne player travelling along +X at `speed`, in a world with nothing
/// in it — the cleanest place to observe an air rule, because nothing else can
/// touch the result.
fn flying_at(speed: Scalar) -> SimState {
    let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(1024.0)), s(0.0));
    st.player.velocity = vec3(speed, s(0.0), s(0.0));
    st
}

/// Hold one direction in the air for `commands` commands without turning.
fn hold_in_air(
    profile: &PhysicsProfile,
    start: &SimState,
    yaw: Scalar,
    commands: usize,
) -> SimState {
    let cmd = UserCmd {
        forward_move: 127,
        view: ViewAngles {
            pitch: s(0.0),
            yaw,
            roll: s(0.0),
        },
        ..still()
    };
    run(
        start,
        &vec![cmd; commands],
        &straf3_sim::world::EmptyWorld,
        profile,
    )
}

// ═══ criterion 1: the VQ3 numbers, observed rather than declared ═══════════

/// `profile.rs` asserts the constants are stored correctly. This asserts they
/// are *used*: each one is recovered from the behaviour of a run.
#[test]
fn the_verified_constants_are_visible_in_the_movement_itself() {
    let p = PhysicsProfile::vq3();
    let ground = straf3_sim::world::FlatGround::at(s(0.0));
    let base = settle_on(&ground, &p, vec3(s(0.0), s(0.0), s(64.0)));

    // JUMP_VELOCITY = 270, less one frame of gravity = 800.
    let jumped = step(
        &base,
        &UserCmd {
            buttons: Buttons::JUMP,
            ..still()
        },
        &ground,
        &p,
    );
    assert!(
        (jumped.player.velocity.z - (s(270.0) - s(800.0) * DT)).abs() < s(0.01),
        "jump velocity {}",
        jumped.player.velocity.z
    );

    // pm_friction = 6, pm_stopspeed = 100: from 200 ups, one frame leaves
    // 200 - 200*6*0.008.
    let mut moving = base;
    moving.player.velocity = vec3(s(200.0), s(0.0), s(0.0));
    let rubbed = step(&moving, &still(), &ground, &p);
    assert!(
        (rubbed.player.velocity.x - (s(200.0) - s(200.0) * s(6.0) * DT)).abs() < s(0.05),
        "friction left {}",
        rubbed.player.velocity.x
    );

    // pm_accelerate = 10, pm_speed = 320: from rest, one frame of forward
    // input adds accel * dt * wishspeed = 10 * 0.008 * 320 = 25.6.
    let pushed = step(
        &base,
        &UserCmd {
            forward_move: 127,
            ..still()
        },
        &ground,
        &p,
    );
    assert!(
        (horizontal_speed(&pushed) - s(25.6)).abs() < s(0.05),
        "one frame of acceleration gave {}",
        horizontal_speed(&pushed)
    );

    // pm_duckScale = 0.25: crouched, the same run settles at a quarter speed.
    let crouch = UserCmd {
        forward_move: 127,
        buttons: Buttons::CROUCH,
        ..still()
    };
    let ducked = run(&base, &vec![crouch; 400], &ground, &p);
    assert!(ducked.player.crouched);
    assert!(
        (horizontal_speed(&ducked) - s(320.0) * s(0.25)).abs() < s(1.0),
        "crouched top speed {}",
        horizontal_speed(&ducked)
    );

    // pm_speed = 320: standing, running straight ahead settles there exactly.
    let sprint = UserCmd {
        forward_move: 127,
        ..still()
    };
    let running = run(&base, &vec![sprint; 400], &ground, &p);
    assert!(
        (horizontal_speed(&running) - s(320.0)).abs() < s(1.0),
        "top speed {}",
        horizontal_speed(&running)
    );
}

// ═══ criterion 2: strafe acceleration ══════════════════════════════════════

/// The technique the whole project is named after.
///
/// Running straight forward is capped at `max_speed`. Turning while
/// accelerating is not, because `PM_Accelerate` clamps the projection of
/// velocity onto the wish direction and nothing clamps the total. The cap for a
/// held angle is `max_speed / cos(angle)`, and that closed form is what is
/// asserted — a test that only said "faster than 320" would pass for a
/// simulation that had lost the mechanism and gained a bug.
#[test]
fn vq3_strafe_acceleration_beats_the_speed_cap_by_exactly_the_cosine() {
    let p = PhysicsProfile::vq3();
    let world = straf3_sim::world::EmptyWorld;
    let start = flying_at(s(320.0));

    // Straight ahead: no gain at all, because the projection is the speed.
    let straight = perfect_strafe(&world, &p, &start, s(0.0), Axis::Forward, 400);
    assert!(
        (horizontal_speed(&straight) - s(320.0)).abs() < s(0.5),
        "straight-ahead speed drifted to {}",
        horizontal_speed(&straight)
    );

    for angle in [s(30.0), s(40.0), s(50.0)] {
        let end = perfect_strafe(&world, &p, &start, angle, Axis::Forward, 900);
        let expected = s(320.0) / (angle * s(core::f32::consts::PI / 180.0)).cos();
        assert!(
            (horizontal_speed(&end) - expected).abs() < s(2.0),
            "at {angle} degrees reached {}, expected {expected}",
            horizontal_speed(&end)
        );
        assert!(horizontal_speed(&end) > s(320.0), "no gain at {angle}");
    }
}

/// CPM replaces that model on the pure-strafe axis: a much larger acceleration
/// against a much smaller wish speed.
///
/// The consequence is that CPM keeps paying out at angles where VQ3 has already
/// stopped — near 90 degrees, where VQ3's projection clamp has closed, CPM's
/// has not, because its wish speed is 30 rather than 320.
#[test]
fn cpm_strafe_acceleration_keeps_paying_where_vq3_has_stopped() {
    let world = straf3_sim::world::EmptyWorld;
    let cpm = PhysicsProfile::cpm();
    let vq3 = PhysicsProfile::vq3();
    // The same profile with only the strafe model switched off: this is what
    // "the technique regressed" looks like, and the assertion below has to
    // notice it.
    let cpm_without = PhysicsProfile {
        strafe_accelerate: s(0.0),
        strafe_wish_speed_cap: s(0.0),
        ..cpm
    };

    let start = flying_at(s(400.0));
    let angle = s(88.0);

    let with = perfect_strafe(&world, &cpm, &start, angle, Axis::Strafe, 200);
    let without = perfect_strafe(&world, &cpm_without, &start, angle, Axis::Strafe, 200);
    let plain = perfect_strafe(&world, &vq3, &start, angle, Axis::Strafe, 200);

    // Zeroing the two constants must reproduce VQ3 exactly — that is the whole
    // claim of the data seam, and if it ever stops being true the extension has
    // grown a branch.
    assert_eq!(
        without.checksum(),
        plain.checksum(),
        "CPM with the strafe model zeroed is no longer VQ3"
    );

    // And the model must actually do something large.
    assert!(
        horizontal_speed(&with) > horizontal_speed(&without) + s(50.0),
        "CPM {} vs VQ3 {}",
        horizontal_speed(&with),
        horizontal_speed(&without)
    );
    // Per-command gain is bounded by accelspeed = 70 * 0.008 * 30 = 16.8,
    // taken almost perpendicular to the velocity, so the speed gain per
    // command approaches `accelspeed * cos(88 degrees)` plus the second-order
    // term. Pinning the total keeps the arithmetic honest.
    assert!(
        (horizontal_speed(&with) - s(400.0)) > s(80.0),
        "CPM gained only {}",
        horizontal_speed(&with) - s(400.0)
    );

    // And an *upper* bound, which is the half that actually pins the model.
    //
    // `strafe_accelerate` and `strafe_wish_speed_cap` are a pair: a very large
    // acceleration against a very small wish speed. A lower bound alone only
    // says "faster than VQ3", which a run with the cap deleted satisfies by
    // going 6.5x too fast — `wishspeed` would then be the full 320 rather than
    // 30, and `PM_Accelerate`'s clamp would stay open far longer than CPM
    // intends. Deleting the cap takes this fixture from 534 to 3474 ups, so the
    // two-sided pin is what makes the constant load-bearing rather than
    // merely present.
    assert!(
        (horizontal_speed(&with) - s(534.07)).abs() < s(2.0),
        "CPM strafe run landed at {} ups, expected 534.07 — the wish-speed cap \
         is the likely suspect: without it this fixture reaches about 3474",
        horizontal_speed(&with)
    );
}

// ═══ criterion 2: air control ══════════════════════════════════════════════

/// CPM's air control turns the velocity vector without changing its length.
///
/// Two things are asserted, and both matter: the direction really does follow
/// the view, and the *speed does not change* while it happens. A "steering"
/// implementation that accelerated towards the view instead would pass the
/// first and fail the second, and would feel completely different.
#[test]
fn cpm_air_control_turns_velocity_without_spending_speed() {
    let cpm = PhysicsProfile::cpm();
    let vq3 = PhysicsProfile::vq3();
    let without = PhysicsProfile {
        air_control: s(0.0),
        ..cpm
    };

    let start = flying_at(s(400.0));
    let turn_to = s(40.0);

    let steered = hold_in_air(&cpm, &start, turn_to, 25);
    let unsteered = hold_in_air(&without, &start, turn_to, 25);
    let vanilla = hold_in_air(&vq3, &start, turn_to, 25);

    // Zeroing one constant must land exactly on VQ3.
    assert_eq!(
        unsteered.checksum(),
        vanilla.checksum(),
        "CPM with air control zeroed is no longer VQ3"
    );

    // With air control the velocity swings most of the way to the view.
    //
    // The window is deliberately tight rather than "somewhere past 25 degrees".
    // `k` carries a `dot * dot` term, and that squaring is what makes air
    // control *steer* — the effect falls away sharply as the wish direction
    // diverges from the current heading. A linear-`dot` version is a different
    // feel entirely, but at this angle it is worth only 0.86 degrees
    // (34.28 -> 35.14), so a wide acceptance band cannot see it. Pinning near
    // the measured value is what makes the exponent load-bearing.
    let turned = heading_degrees(&steered);
    assert!(
        (turned - s(34.28)).abs() < s(0.5),
        "air control turned the player to {turned} degrees, expected 34.28 \
         (a linear-dot falloff instead of dot-squared gives about 35.14)"
    );
    assert!(
        turned <= turn_to + s(1.0),
        "air control overshot the view: {turned} degrees against a view of {turn_to}"
    );
    // Without it, barely at all.
    assert!(
        heading_degrees(&unsteered) < s(6.0),
        "VQ3 turned {} degrees, which it should not",
        heading_degrees(&unsteered)
    );

    // And the turn is close to free. The small gain that remains is
    // `PM_Accelerate` still running underneath, not air control.
    let gained = horizontal_speed(&steered) - s(400.0);
    assert!(
        gained.abs() < s(15.0),
        "air control changed speed by {gained}, which it must not"
    );
}

/// Air control refuses to steer while the player is slowing down — CPM's
/// `dot > 0` gate.
///
/// The other two air-control tests both live entirely in `dot > 0`: one steers
/// 40 degrees off the velocity, and the air-stop test zeroes `air_control` in
/// both profiles it compares (it has to — the normalise-and-rescale round trip
/// is not bit-exact, which would break its checksum equality for a reason that
/// has nothing to do with braking). So the negative-`dot` half of `air_control`
/// had no assertion over it at all.
///
/// That gap is worth closing on its own terms. Removing the gate — letting the
/// steer run while `dot < 0` — leaves the player pointing 63 degrees off
/// instead of 6.5, because air control would be free to swing the velocity
/// around while `air_stop_accelerate` is supposed to own that case. The two
/// mechanisms would fight, and the one that is meant to make CPM's direction
/// changes *sharp* would instead make them curved.
#[test]
fn air_control_refuses_to_steer_while_slowing_down() {
    let cpm = PhysicsProfile::cpm();
    let start = flying_at(s(400.0));

    // Flying along +X, holding forward while looking back down the velocity at
    // yaw 170 — the wish direction opposes the motion, so `dot` is negative and
    // the gate must hold the heading nearly still while the player brakes.
    let braking = hold_in_air(&cpm, &start, s(170.0), 25);

    assert!(
        (heading_degrees(&braking) - s(6.54)).abs() < s(1.0),
        "expected the heading to stay near 6.54 degrees while braking, got {} \
         — removing the `dot > 0` gate in air_control gives about 63.5",
        heading_degrees(&braking)
    );
    // And the braking itself still happened: this is air-stop's regime, not a
    // test that nothing occurred.
    assert!(
        (horizontal_speed(&braking) - s(244.02)).abs() < s(2.0),
        "expected to brake from 400 to about 244 ups, got {}",
        horizontal_speed(&braking)
    );
}

/// Air control is gated on holding forward or back **alone**.
///
/// This is not a detail: it is why CPM air movement is played one key at a
/// time. With a strafe key added, CPM must fall back to the vanilla air rules
/// exactly.
#[test]
fn air_control_is_refused_when_a_strafe_key_is_also_held() {
    let cpm = PhysicsProfile::cpm();
    let vq3 = PhysicsProfile::vq3();
    let start = flying_at(s(400.0));

    let diagonal = UserCmd {
        forward_move: 127,
        right_move: 127,
        view: ViewAngles {
            pitch: s(0.0),
            yaw: s(20.0),
            roll: s(0.0),
        },
        ..still()
    };
    let world = straf3_sim::world::EmptyWorld;
    let under_cpm = run(&start, &vec![diagonal; 25], &world, &cpm);
    let under_vq3 = run(&start, &vec![diagonal; 25], &world, &vq3);

    assert_eq!(
        under_cpm.checksum(),
        under_vq3.checksum(),
        "CPM applied an air rule to a diagonal input; it must not"
    );
}

// ═══ criterion 2: air-stop ═════════════════════════════════════════════════

/// CPM's air-stop acceleration: pushing against your own velocity in the air
/// brakes far harder than vanilla, which is what makes mid-air direction
/// changes sharp instead of floaty.
#[test]
fn cpm_air_stop_brakes_harder_than_vanilla() {
    let cpm = PhysicsProfile::cpm();
    let vq3 = PhysicsProfile::vq3();
    // Air control has to be zeroed here too, not because it changes the
    // braking — it refuses to steer while `dot` is negative — but because it
    // still rewrites the velocity through a normalise-and-rescale, and that
    // round trip is not bit-exact in `f32`. Leaving it on would make the
    // checksum comparison below fail for a reason that has nothing to do with
    // air-stop.
    let without = PhysicsProfile {
        air_stop_accelerate: s(0.0),
        air_control: s(0.0),
        ..cpm
    };

    // Flying along +X, holding backwards.
    let start = flying_at(s(400.0));
    let braking = s(180.0);

    let cpm_end = hold_in_air(&cpm, &start, braking, 25);
    let off_end = hold_in_air(&without, &start, braking, 25);
    let vq3_end = hold_in_air(&vq3, &start, braking, 25);

    assert_eq!(
        off_end.checksum(),
        vq3_end.checksum(),
        "CPM with the air extensions zeroed is no longer VQ3"
    );
    assert_ne!(
        cpm_end.checksum(),
        off_end.checksum(),
        "zeroing air-stop changed nothing, so nothing reads it"
    );

    // VQ3: accel 1 * 0.008 * 320 = 2.56 per command, 25 commands = 64 lost.
    assert!(
        (vq3_end.player.velocity.x - (s(400.0) - s(64.0))).abs() < s(1.0),
        "VQ3 braking left {}",
        vq3_end.player.velocity.x
    );
    // CPM: accel 2.5 * 0.008 * 320 = 6.4 per command, 25 commands = 160 lost.
    assert!(
        (cpm_end.player.velocity.x - (s(400.0) - s(160.0))).abs() < s(1.0),
        "CPM braking left {}",
        cpm_end.player.velocity.x
    );
}

// ═══ criterion 2: double jump ══════════════════════════════════════════════

/// Jump, land, jump again inside the window: the second jump is boosted.
#[test]
fn cpm_double_jump_boosts_only_inside_the_window() {
    let cpm = PhysicsProfile::cpm();
    let vq3 = PhysicsProfile::vq3();
    let ground = straf3_sim::world::FlatGround::at(s(0.0));

    let jump = UserCmd {
        buttons: Buttons::JUMP,
        ..still()
    };

    // Jump, then release and wait to land. A 270 ups jump under 800 gravity is
    // airborne for about 675 ms; 120 commands is 960 ms, comfortably past it
    // and still inside the 400 ms window that opens on landing.
    let land_after_jumping = |p: &PhysicsProfile, idle: usize| {
        let base = settle_on(&ground, p, vec3(s(0.0), s(0.0), s(64.0)));
        let mut cmds = vec![jump];
        cmds.extend(vec![still(); idle]);
        let st = run(&base, &cmds, &ground, p);
        assert!(st.player.ground.is_grounded(), "should have landed by now");
        st
    };

    let landed = land_after_jumping(&cpm, 120);
    assert!(
        landed.player.timers.double_jump_ms > 0,
        "the window should be open on landing"
    );
    let second = step(&landed, &jump, &ground, &cpm);
    let expect_boosted = s(270.0) + s(100.0) - s(800.0) * DT;
    assert!(
        (second.player.velocity.z - expect_boosted).abs() < s(0.01),
        "boosted jump gave {}, expected {expect_boosted}",
        second.player.velocity.z
    );

    // Wait out the window — 400 ms is 50 commands — and the boost is gone.
    let stale = run(&landed, &vec![still(); 60], &ground, &cpm);
    assert_eq!(stale.player.timers.double_jump_ms, 0);
    let late = step(&stale, &jump, &ground, &cpm);
    let expect_plain = s(270.0) - s(800.0) * DT;
    assert!(
        (late.player.velocity.z - expect_plain).abs() < s(0.01),
        "late jump gave {}, expected {expect_plain}",
        late.player.velocity.z
    );

    // VQ3 never boosts, however fast you are.
    let vq3_landed = land_after_jumping(&vq3, 120);
    assert_eq!(vq3_landed.player.timers.double_jump_ms, 0);
    let vq3_second = step(&vq3_landed, &jump, &ground, &vq3);
    assert!(
        (vq3_second.player.velocity.z - expect_plain).abs() < s(0.01),
        "VQ3 boosted a jump: {}",
        vq3_second.player.velocity.z
    );
}

/// A window buys exactly one boosted jump, not a boosted state.
///
/// Without this the technique degenerates: land once, and every jump for the
/// next 400 ms is boosted, including jumps that never touched the ground.
#[test]
fn a_double_jump_window_is_spent_by_the_jump_that_uses_it() {
    let cpm = PhysicsProfile::cpm();
    let ground = straf3_sim::world::FlatGround::at(s(0.0));
    let jump = UserCmd {
        buttons: Buttons::JUMP,
        ..still()
    };

    let base = settle_on(&ground, &cpm, vec3(s(0.0), s(0.0), s(64.0)));
    let mut cmds = vec![jump];
    cmds.extend(vec![still(); 120]);
    let landed = run(&base, &cmds, &ground, &cpm);

    let boosted = step(&landed, &jump, &ground, &cpm);
    assert_eq!(boosted.player.timers.double_jump_ms, 0, "window not spent");
    assert!(boosted.player.velocity.z > s(350.0));
}

/// Walking off a ledge and jumping on contact is not a double jump.
///
/// The window is provenance-gated: it opens for a landing that ended a jump. A
/// simple "time since landed" comparison would hand out the boost here, and
/// that is a different game.
#[test]
fn walking_off_a_ledge_does_not_arm_the_double_jump() {
    let cpm = PhysicsProfile::cpm();
    let ground = straf3_sim::world::FlatGround::at(s(0.0));
    let jump = UserCmd {
        buttons: Buttons::JUMP,
        ..still()
    };

    // Start in the air without having jumped, and fall onto the floor.
    let mut falling = SimState::spawned_at(vec3(s(0.0), s(0.0), s(200.0)), s(0.0));
    falling.player.ground = GroundState::Airborne;
    assert!(!falling.player.left_ground_by_jumping);

    // Step until the moment of landing and check *there*. Running a fixed
    // number of commands and checking at the end would let a wrongly-armed
    // 400 ms window count itself back down to zero before the assertion ran,
    // and the test would pass for the wrong reason.
    let mut landed = falling;
    let mut commands = 0;
    while !landed.player.ground.is_grounded() {
        landed = step(&landed, &still(), &ground, &cpm);
        commands += 1;
        assert!(commands < 400, "never landed");
    }
    assert_eq!(
        landed.player.timers.double_jump_ms, 0,
        "a fall armed the double-jump window"
    );
    assert!(!landed.player.left_ground_by_jumping);

    let after = step(&landed, &jump, &ground, &cpm);
    let expect_plain = s(270.0) - s(800.0) * DT;
    assert!(
        (after.player.velocity.z - expect_plain).abs() < s(0.01),
        "got {}",
        after.player.velocity.z
    );
}

// ═══ criterion 2: ramps ════════════════════════════════════════════════════

/// Q3's "don't decrease velocity when going up or down a slope".
///
/// Velocity is clipped to the ground plane — which shortens it — and then
/// rescaled back to the length it had. The direction follows the surface; the
/// magnitude does not notice the surface tilted. Delete the rescale and a
/// player loses speed every time the ground is not flat, which is the single
/// most noticeable way ramp movement can regress.
#[test]
fn a_walkable_ramp_preserves_speed_through_the_ground_plane_clip() {
    // Friction off so the only thing that can change the speed is the clip.
    // This is a legitimate use of the data seam, not a special case in the
    // physics: `friction` is a number, so a test may choose it.
    let p = PhysicsProfile {
        friction: s(0.0),
        ..PhysicsProfile::vq3()
    };
    let ramp = SlopedGround::tilted(s(26.0)); // normal.z = 0.899, walkable
    let base = settle_on(&ramp, &p, vec3(s(0.0), s(0.0), s(200.0)));
    assert!(base.player.ground.is_grounded(), "should be standing on it");

    // Drive horizontally into the uphill face.
    let mut moving = base;
    moving.player.velocity = vec3(s(200.0), s(0.0), s(0.0));
    let after = step(&moving, &still(), &ramp, &p);

    let speed = after.player.velocity.length();
    assert!(
        (speed - s(200.0)).abs() < s(2.0),
        "ramp cost {} ups of the 200 it started with",
        s(200.0) - speed
    );
    // The direction genuinely changed — otherwise "preserved" would be trivial.
    assert!(
        after.player.velocity.z > s(60.0),
        "not actually climbing: vz = {}",
        after.player.velocity.z
    );
    // Sanity: bare clipping alone would have left about 179 ups.
    assert!(speed > s(190.0));
}

/// A slope too steep to stand on is still a surface to slide along.
///
/// This is the ramp-slide mechanism from the movement reference: the player is
/// not *walking*, so no ground friction is applied, and velocity is clipped to
/// the plane rather than stopped by it. Only gravity's component along the
/// slope bleeds speed. Collapse `Sliding` into `Grounded` and friction returns,
/// which this test is here to catch.
#[test]
fn a_steep_ramp_slides_without_friction() {
    let p = PhysicsProfile::vq3();
    // 50 degrees: normal.z = 0.643, below MIN_WALK_NORMAL = 0.7.
    let steep = SlopedGround::tilted(s(50.0));
    assert!(steep.normal.z < p.min_walk_normal);

    let base = settle_on(&steep, &p, vec3(s(0.0), s(0.0), s(400.0)));
    assert!(
        matches!(base.player.ground, GroundState::Sliding { .. }),
        "expected to be sliding, got {:?}",
        base.player.ground
    );
    assert!(!base.player.ground.is_grounded());

    // Same starting speed on the steep ramp and on flat ground, no input.
    let mut on_steep = base;
    on_steep.player.velocity = vec3(s(-300.0), s(0.0), s(0.0)); // downhill
    let slid = run(&on_steep, &vec![still(); 25], &steep, &p);

    let flat = straf3_sim::world::FlatGround::at(s(0.0));
    let mut on_flat = settle_on(&flat, &p, vec3(s(0.0), s(0.0), s(64.0)));
    on_flat.player.velocity = vec3(s(-300.0), s(0.0), s(0.0));
    let rubbed = run(&on_flat, &vec![still(); 25], &flat, &p);

    // Flat ground: 25 commands of pm_friction 6 leave 300 * (1 - 6*0.008)^25,
    // about 88 ups. The ramp keeps essentially all of it, and gains from
    // gravity besides.
    assert!(
        horizontal_speed(&rubbed) < s(120.0),
        "flat ground kept {} ups; friction is not being applied",
        horizontal_speed(&rubbed)
    );
    assert!(
        slid.player.velocity.length() > s(300.0),
        "the steep ramp bled speed down to {}",
        slid.player.velocity.length()
    );

    // The magnitude alone is not enough. Deleting the airborne ground-plane
    // clip in `air_move` — the redirect this test exists to cover — makes the
    // player *faster* (340 ups), so a lower bound sails straight past it. Pin
    // the vector: the speed has to be pointing along the ramp, not through it.
    let want = vec3(s(-199.59), s(0.0), s(-244.25));
    assert!(
        (slid.player.velocity - want).length() < s(2.0),
        "expected to be redirected along the slope to {want:?}, got {:?}",
        slid.player.velocity
    );

    // The sharpest form of the same property: drive *into* the hill. Correct
    // behaviour turns the motion up the surface; without the clip the player
    // keeps their horizontal velocity and sinks, i.e. travels through the ramp
    // as though it were not there.
    let mut uphill = base;
    uphill.player.velocity = vec3(s(400.0), s(0.0), s(0.0));
    let climbed = run(&uphill, &vec![still(); 1], &steep, &p);
    assert!(
        climbed.player.velocity.z > s(150.0),
        "driving into a 50-degree slope at 400 ups should redirect upward; \
         got {:?} (without the clip this is about (400, 0, -6.4))",
        climbed.player.velocity
    );
}

// ═══ the rest of the pipeline ══════════════════════════════════════════════

/// STEPSIZE = 18: an 18-unit step is walked up, a 24-unit one is not.
#[test]
fn a_step_within_stepsize_is_climbed_and_a_taller_one_is_not() {
    let p = PhysicsProfile::vq3();
    let walk_east = UserCmd {
        forward_move: 127,
        ..still()
    };

    let climb = |height: Scalar| {
        let world = Boxes::stair(height);
        let base = settle_on(&world, &p, vec3(s(-64.0), s(0.0), s(64.0)));
        assert!(base.player.ground.is_grounded());
        let end = run(&base, &vec![walk_east; 250], &world, &p);
        (end.player.origin.x, end.player.origin.z)
    };

    // The origin sits 24 units above the feet, so standing on the floor is
    // z = 24 and standing on an 18-unit step is z = 42, both plus the clip
    // epsilon the hull is held clear by.
    let (x_low, z_low) = climb(p.step_height);
    assert!(
        x_low > s(0.0),
        "stopped short of an 18-unit step at x={x_low}"
    );
    assert!(
        z_low > s(41.0),
        "did not end up on top of the 18-unit step, z={z_low}"
    );

    let (x_high, z_high) = climb(s(24.0));
    assert!(
        x_high < s(0.0),
        "walked up a 24-unit step, which is taller than STEPSIZE"
    );
    assert!(z_high < s(25.0), "unexpectedly climbed, z={z_high}");
}

/// MAX_CLIP_PLANES and the crease path: driven into an inside corner, the
/// player stops rather than squeezing through it.
#[test]
fn an_inside_corner_stops_the_player_instead_of_leaking() {
    let p = PhysicsProfile::vq3();
    let world = Boxes::corner();
    let base = settle_on(&world, &p, vec3(s(0.0), s(0.0), s(64.0)));

    // Drive north-east into the corner for two seconds.
    let into_corner = UserCmd {
        forward_move: 127,
        view: ViewAngles {
            pitch: s(0.0),
            yaw: s(45.0),
            roll: s(0.0),
        },
        ..still()
    };
    let end = run(&base, &vec![into_corner; 250], &world, &p);

    // The walls stand at x=64 and y=64, and the hull is 15 units wide, so the
    // furthest the origin can legally reach is 49 on each axis.
    assert!(
        end.player.origin.x <= s(49.5) && end.player.origin.y <= s(49.5),
        "leaked through the corner to {:?}",
        end.player.origin
    );
    assert!(
        end.player.origin.x > s(40.0) && end.player.origin.y > s(40.0),
        "never reached the corner: {:?}",
        end.player.origin
    );
    assert!(
        horizontal_speed(&end) < s(30.0),
        "still moving at {} in a corner",
        horizontal_speed(&end)
    );
}

/// SURF_SLICK hands the player the air rules while still standing on the
/// ground: no friction, and air acceleration instead of ground acceleration.
#[test]
fn slick_ground_removes_friction_and_ground_acceleration() {
    let p = PhysicsProfile::vq3();
    let ice = straf3_sim::world::FlatGround {
        height: s(0.0),
        surface: SurfaceFlags::SLICK,
    };
    let grippy = straf3_sim::world::FlatGround::at(s(0.0));

    let start_speed = s(300.0);
    let coast = |world: &straf3_sim::world::FlatGround| {
        let mut st = settle_on(world, &p, vec3(s(0.0), s(0.0), s(64.0)));
        st.player.velocity = vec3(start_speed, s(0.0), s(0.0));
        horizontal_speed(&run(&st, &vec![still(); 25], world, &p))
    };

    // Ice keeps every unit of the 300, and in fact gains a little: slick
    // ground applies gravity while walking, and the slope rescale above then
    // turns each frame's fall speed back into horizontal speed. That is a
    // genuine Q3 behaviour, not slack in the assertion, so the bound is
    // one-sided and tight.
    let on_ice = coast(&ice);
    assert!(
        on_ice >= start_speed && on_ice < start_speed + s(3.0),
        "ice left {on_ice} ups, expected just over {start_speed}"
    );
    assert!(
        coast(&grippy) < s(120.0),
        "normal ground did not apply friction: {} ups left",
        coast(&grippy)
    );

    // The friction half is only half the claim. Coasting never asks for a wish
    // direction, so `wishspeed` is zero and the acceleration branch is never
    // reached — meaning "and ground acceleration" was untested. Replacing the
    // whole slick test with `false` in `walk_move` leaves a coasting fixture
    // completely unchanged, because `friction()` carries its own independent
    // SLICK check that the substitution does not touch.
    //
    // So: hold forward from a standstill. On ice the player must accelerate at
    // `air_accelerate` (1) rather than `accelerate` (10), which is the
    // difference between crawling to 82 ups and reaching the full 320.
    let push = |world: &straf3_sim::world::FlatGround| {
        let st = settle_on(world, &p, vec3(s(0.0), s(0.0), s(64.0)));
        let forward = UserCmd {
            forward_move: 127,
            ..still()
        };
        horizontal_speed(&run(&st, &vec![forward; 25], world, &p))
    };

    let accelerated_on_ice = push(&ice);
    assert!(
        (accelerated_on_ice - s(82.15)).abs() < s(2.0),
        "holding forward on ice for 25 commands reached {accelerated_on_ice} ups, \
         expected about 82 — ground acceleration is being applied on a slick \
         surface, which would reach 320"
    );
    // The same input on normal ground reaches the full speed, so the number
    // above is a property of the surface and not of the fixture being too short.
    let accelerated_on_grip = push(&grippy);
    assert!(
        accelerated_on_grip > s(300.0),
        "normal ground only reached {accelerated_on_grip} ups in 25 commands"
    );
}

/// Crouching lowers the hull, and standing up is refused while something is in
/// the way.
#[test]
fn standing_up_is_refused_under_a_low_ceiling() {
    let p = PhysicsProfile::vq3();
    // Floor at z=0 and a ceiling slab whose underside is at z=20 — above the
    // crouched hull's 16, below the standing hull's 32.
    let world = Boxes(vec![
        (
            vec3(s(-4096.0), s(-4096.0), s(-512.0)),
            vec3(s(4096.0), s(4096.0), s(0.0)),
        ),
        (
            vec3(s(-4096.0), s(-4096.0), s(20.0)),
            vec3(s(4096.0), s(4096.0), s(512.0)),
        ),
    ]);

    let crouch = UserCmd {
        buttons: Buttons::CROUCH,
        ..still()
    };
    let base = settle_on(&world, &p, vec3(s(0.0), s(0.0), s(24.0)));
    let ducked = run(&base, &vec![crouch; 10], &world, &p);
    assert!(ducked.player.crouched);

    // Release crouch: the standing hull does not fit, so the player stays down.
    let still_stuck = run(&ducked, &vec![still(); 10], &world, &p);
    assert!(
        still_stuck.player.crouched,
        "stood up into a ceiling 20 units above the floor"
    );
}

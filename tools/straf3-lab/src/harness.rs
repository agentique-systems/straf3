//! The techniques, expressed as functions of the physics rather than of a
//! human hand.
//!
//! A player holds a strafe angle imperfectly; the lab holds it exactly. That is
//! the difference between measuring a technique and measuring somebody doing a
//! technique, and it is what makes the numbers here a property of the tree.
//!
//! The shapes here are deliberately the same as `crates/straf3-sim/tests/movement.rs`'s
//! `perfect_strafe` and `settle_on`, because a lab number that did not
//! reproduce the corresponding test's number would be measuring something else.
//! They are re-expressed rather than imported: an integration test is not a
//! library, and `#[path]`-including one would make the test file a dependency of
//! the published document.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::World;
use straf3_sim::{
    Buttons, GroundState, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, run, step,
};

use crate::num::{heading_degrees, horizontal_speed};

/// The command rate every measurement is taken at: 125 Hz, spec D2's default.
///
/// Not swept. The rate is part of the physics (see [`straf3_sim::TickRate`]),
/// so a sweep over it would be a different report — worth having, and named as
/// a gap in the results document rather than half-done here.
pub const RATE: TickRate = TickRate::HZ_125;

/// One command's duration in milliseconds, at [`RATE`].
pub const MS: u16 = RATE.command_millis();

/// Commands per second at [`RATE`].
pub const HZ: usize = 125;

/// Which command axis a strafe run holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Forward only — Quake's `movementDir` 0. The axis VQ3 strafejumps on, and
    /// the only one CPM grants air control to.
    Forward,
    /// Right only — `movementDir` 6. The axis CPM's strafe-acceleration model
    /// is written for.
    Strafe,
}

impl Axis {
    /// The name this axis carries in a measurement key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Strafe => "strafe",
        }
    }
}

/// An empty command of one tick.
#[must_use]
pub fn still() -> UserCmd {
    UserCmd::still(MS)
}

/// A command holding one axis, looking along `yaw`.
#[must_use]
pub fn holding(axis: Axis, yaw: Scalar) -> UserCmd {
    UserCmd {
        forward_move: if axis == Axis::Forward { 127 } else { 0 },
        right_move: if axis == Axis::Strafe { 127 } else { 0 },
        view: ViewAngles::from_degrees(s(0.0), yaw, s(0.0)),
        ..still()
    }
}

/// A command that presses jump.
#[must_use]
pub fn jump() -> UserCmd {
    UserCmd {
        buttons: Buttons::JUMP,
        ..still()
    }
}

/// The yaw that aims `axis`'s wish direction at world heading `want`.
///
/// Quake's `right` points along −Y at yaw 0, so aiming the strafe axis at a
/// direction needs a yaw 90° round from aiming the forward axis at it. The sign
/// is baked into every recorded `right_move` and is not a mistake to tidy.
#[must_use]
pub fn yaw_for(axis: Axis, want: Scalar) -> Scalar {
    match axis {
        Axis::Forward => want,
        Axis::Strafe => want + s(90.0),
    }
}

/// Drop the player onto `world` from `from` and let them come to rest.
///
/// Velocity is zeroed afterwards rather than waited out: a settled player still
/// carries the fraction of a unit per second that friction's `< 1.0` cutoff
/// leaves, and a measurement that started from it would be starting from noise.
#[must_use]
pub fn settle_on<W: World>(world: &W, profile: &PhysicsProfile, from: Vec3) -> SimState {
    let spawn = SimState::spawned_at(from, s(0.0));
    let mut st = run(&spawn, &vec![still(); 400], world, profile);
    st.player.velocity = Vec3::ZERO;
    st
}

/// An airborne player travelling along +X at `speed`, in empty space.
///
/// The cleanest place to observe an air rule: nothing else can touch the
/// result, and the player never lands, so a long run measures the air model and
/// not the ground one.
#[must_use]
pub fn flying_at(speed: Scalar) -> SimState {
    let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(16_384.0)), s(0.0));
    st.player.velocity = vec3(speed, s(0.0), s(0.0));
    st.player.ground = GroundState::Airborne;
    st
}

/// A player standing on `world`, moving along +X at `speed`.
#[must_use]
pub fn running_at<W: World>(world: &W, profile: &PhysicsProfile, speed: Scalar) -> SimState {
    let mut st = settle_on(world, profile, vec3(s(-1024.0), s(0.0), s(64.0)));
    st.player.velocity = vec3(speed, s(0.0), s(0.0));
    st
}

/// One command of the perfect strafe: aim `angle` degrees off the current
/// heading and hold `axis`.
#[must_use]
pub fn strafe_once<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    st: &SimState,
    angle: Scalar,
    axis: Axis,
) -> SimState {
    let want = heading_degrees(st.player.velocity) + angle;
    step(st, &holding(axis, yaw_for(axis, want)), world, profile)
}

/// Hold the perfect strafe for `commands` commands.
#[must_use]
pub fn strafe_for<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    start: &SimState,
    angle: Scalar,
    axis: Axis,
    commands: usize,
) -> SimState {
    let mut st = *start;
    for _ in 0..commands {
        st = strafe_once(world, profile, &st, angle, axis);
    }
    st
}

/// Speed gained per second of held strafe, starting from `start`.
///
/// One second, not one command: a per-command number is dominated by the
/// jump frame's `PM_CmdScale` dip and by whichever side of a quantised view
/// angle the first command lands on. A second is also the unit a player thinks
/// in — "this technique is worth 40 ups a second" is a sentence about the game.
#[must_use]
pub fn gain_per_second<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    start: &SimState,
    angle: Scalar,
    axis: Axis,
) -> Scalar {
    let before = horizontal_speed(start.player.velocity);
    let after = strafe_for(world, profile, start, angle, axis, HZ);
    horizontal_speed(after.player.velocity) - before
}

/// The best held angle at `start`, searched at 1° resolution, and what it is
/// worth.
///
/// Coarse on purpose. The gain surface is smooth and nearly flat near its
/// maximum — that flatness *is* why the technique is playable by a human — so a
/// finer search would report a number no player could hold and no reader could
/// use. The report says the resolution beside the number.
#[must_use]
pub fn optimal_angle<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    start: &SimState,
    axis: Axis,
) -> (Scalar, Scalar) {
    let mut best = (s(0.0), Scalar::NEG_INFINITY);
    for whole_degrees in 0..=90 {
        let angle = s(whole_degrees as f32);
        let gain = gain_per_second(world, profile, start, angle, axis);
        if gain > best.1 {
            best = (angle, gain);
        }
    }
    best
}

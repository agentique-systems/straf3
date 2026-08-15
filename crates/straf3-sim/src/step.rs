//! The step function: the one entry point the whole project depends on.

use crate::cmd::{Buttons, UserCmd};
use crate::num::{self, Vec3, s};
use crate::profile::PhysicsProfile;
use crate::state::{GroundState, SimState};
use crate::world::{Sweep, World};

/// Advance the simulation by exactly one command.
///
/// # The contract this function exists to state
///
/// The result depends on the four arguments and **nothing else**. No globals,
/// no clock, no filesystem, no unseeded randomness, no thread-local cache, no
/// interior mutability. Call it twice with the same inputs and you get the
/// same bits.
///
/// That is not fastidiousness; it is the property everything downstream is
/// built on:
///
/// - **Replays and ghosts** store inputs, not positions. A ghost is this
///   function re-run.
/// - **Regression tests** replay a recorded run and compare the result, which
///   only detects a change in movement feel if nothing else can vary.
/// - **Headless servers and RL environments** run this with no window, no GPU
///   and no frame loop, faster than real time, in parallel.
/// - **Debugging** a movement bug means replaying the input that caused it.
///   Once, exactly, rather than trying to reproduce it by hand.
///
/// Anything that would break it — reading a config file here, asking what time
/// it is, hashing a pointer address — must go somewhere above the line
/// instead. `cargo xtask check-seam` fails the build over it, and that check
/// is deliberately hard to argue with.
///
/// # Why a `World` and a `PhysicsProfile` are arguments
///
/// Because they are inputs, and inputs to a pure function are arguments. A
/// global "current map" or "current physics mode" would make two callers in
/// the same process interfere — which is exactly what a headless server
/// running many simulations at once, or a test suite running in parallel
/// threads, does.
///
/// # Status
///
/// **The movement physics is a placeholder.** It integrates velocity under
/// gravity and stops at geometry, which is enough to make the shape real and
/// the determinism test meaningful. Strafejumping, friction, acceleration,
/// the slide solver, step-up, jumping and the CPM extensions are Wave 2's
/// work, and they belong here, behind this unchanged signature.
#[must_use]
pub fn step<W>(state: &SimState, cmd: &UserCmd, world: &W, profile: &PhysicsProfile) -> SimState
where
    W: World + ?Sized,
{
    let mut next = *state;
    step_in_place(&mut next, cmd, world, profile);
    next
}

/// [`step`], writing into an existing state.
///
/// Identical in behaviour; it exists because a headless server stepping
/// thousands of simulations per second should not be forced to copy state it
/// is about to overwrite. `step` is defined in terms of this one, so there is
/// no second implementation to keep in agreement.
pub fn step_in_place<W>(state: &mut SimState, cmd: &UserCmd, world: &W, profile: &PhysicsProfile)
where
    W: World + ?Sized,
{
    // A zero-length command advances nothing. Returning early rather than
    // integrating by zero keeps `tick` counting commands that did something.
    if cmd.duration_ms == 0 {
        return;
    }

    // TODO(wave2): Q3's PmoveSingle splits a long command into several
    // sub-steps (pmove_msec) so that a large duration cannot tunnel through
    // geometry or skip a jump window. That split changes results, so it must
    // land before any reference replay is recorded.
    let ms = cmd.duration_ms;
    let dt = num::seconds_from_millis(u32::from(ms));

    // The view is player input, applied whole. Movement never rotates the
    // player: what you looked at is what the recording says you looked at.
    state.player.view = cmd.view;
    state.player.crouched = cmd.buttons.contains(Buttons::CROUCH);

    // ── placeholder movement ───────────────────────────────────────────
    // TODO(wave2): replace everything between here and the ground probe with
    // the real pipeline — friction, acceleration (ground/air/CPM air control),
    // jump, and the multi-plane slide solver bounded by
    // `profile.max_clip_planes`. The signature above does not change.
    if !state.player.ground.is_grounded() {
        state.player.velocity.z -= profile.gravity * dt;
    }

    let half_extents = profile.hull_half_extents();
    let center_offset = profile.hull_center_offset();

    let motion = state.player.velocity * dt;
    let trace = world.trace(&Sweep {
        start: state.player.origin,
        end: state.player.origin + motion,
        half_extents,
        center_offset,
    });

    if trace.start_solid {
        // Stuck. Do not integrate out of it by force — the real solver has to
        // decide, and silently teleporting here would hide the bug that put
        // the player inside geometry.
        // TODO(wave2): Q3 nudges out of solid; decide the policy there.
    } else {
        state.player.origin += motion * trace.fraction;
        if trace.hit() {
            state.player.velocity = clip_velocity(state.player.velocity, trace.normal, profile);
        }
    }

    // ── ground state ───────────────────────────────────────────────────
    // Probe below the hull rather than trusting what the move trace hit: the
    // player can finish a command a hair above a surface and must still count
    // as standing on it, which is what makes landings consistent.
    let probe = world.trace(&Sweep {
        start: state.player.origin,
        end: state.player.origin - num::UP * profile.ground_trace_probe,
        half_extents,
        center_offset,
    });
    let was_grounded = state.player.ground.is_grounded();
    state.player.ground =
        if probe.hit() && !probe.start_solid && probe.normal.z >= profile.min_walk_normal {
            GroundState::Grounded {
                normal: probe.normal,
            }
        } else {
            GroundState::Airborne
        };

    // ── timers and bookkeeping ─────────────────────────────────────────
    state.player.timers.advance(ms);
    if !was_grounded && state.player.ground.is_grounded() {
        state.player.timers.since_landed_ms = 0;
    }
    state.player.jump_held = cmd.buttons.contains(Buttons::JUMP);

    state.tick += 1;
    state.time_ms += u32::from(ms);
}

/// Quake's `PM_ClipVelocity`: remove the component of `velocity` heading into
/// a plane, and then a little more.
///
/// The "little more" is [`PhysicsProfile::overclip`], and it is not rounding
/// slack to be tidied away — pushing slightly out of the plane rather than
/// exactly onto it is what produces overbounce and ramp boosts. Changing this
/// changes the game.
fn clip_velocity(velocity: Vec3, normal: Vec3, profile: &PhysicsProfile) -> Vec3 {
    let mut backoff = velocity.dot(normal);
    backoff = if backoff < s(0.0) {
        backoff * profile.overclip
    } else {
        backoff / profile.overclip
    };
    velocity - normal * backoff
}

/// Apply a whole sequence of commands in order.
///
/// A convenience over [`step`] with no behaviour of its own — it exists so
/// that the headless runner, the tests and a future replay verifier all drive
/// the simulation through exactly the same loop rather than each writing their
/// own subtly different one.
#[must_use]
pub fn run<'a, W, I>(initial: &SimState, cmds: I, world: &W, profile: &PhysicsProfile) -> SimState
where
    W: World + ?Sized,
    I: IntoIterator<Item = &'a UserCmd>,
{
    let mut state = *initial;
    for cmd in cmds {
        step_in_place(&mut state, cmd, world, profile);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::TickRate;
    use crate::num::vec3;
    use crate::world::{EmptyWorld, FlatGround};

    #[test]
    fn a_zero_length_command_changes_nothing() {
        let before = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));
        let after = step(
            &before,
            &UserCmd::still(0),
            &EmptyWorld,
            &PhysicsProfile::default(),
        );
        assert_eq!(before.checksum(), after.checksum());
    }

    #[test]
    fn time_is_the_exact_sum_of_command_durations() {
        let rate = TickRate::HZ_76; // 13 ms: the rate whose ms does not divide evenly
        let cmds = vec![UserCmd::still_at(rate); 1000];
        let end = run(
            &SimState::default(),
            &cmds,
            &EmptyWorld,
            &PhysicsProfile::default(),
        );
        assert_eq!(end.tick, 1000);
        assert_eq!(end.time_ms, 13_000); // exact, no float drift
    }

    #[test]
    fn the_tick_rate_changes_the_simulation() {
        // The point of D2: 250 commands of 4 ms and 125 of 8 ms cover the same
        // wall-clock second but are not the same simulation, because gravity
        // is applied per command.
        let spawn = SimState::spawned_at(vec3(s(0.0), s(0.0), s(1000.0)), s(0.0));
        let profile = PhysicsProfile::default();

        let at_125 = run(&spawn, &vec![UserCmd::still(8); 125], &EmptyWorld, &profile);
        let at_250 = run(&spawn, &vec![UserCmd::still(4); 250], &EmptyWorld, &profile);

        assert_eq!(at_125.time_ms, at_250.time_ms);
        assert_ne!(at_125.checksum(), at_250.checksum());
    }

    #[test]
    fn gravity_pulls_a_player_down_onto_ground_and_stops_there() {
        let ground = FlatGround::at(s(0.0));
        let profile = PhysicsProfile::default();
        let spawn = SimState::spawned_at(vec3(s(0.0), s(0.0), s(100.0)), s(0.0));

        let end = run(&spawn, &vec![UserCmd::still(8); 400], &ground, &profile);
        assert!(end.player.ground.is_grounded(), "should have landed");
        // Quake's origin is not at the feet: a standing player resting on a
        // floor at z=0 sits at z = -hull_mins.z = 24.
        let expected = -profile.hull_mins.z;
        assert!(
            (end.player.origin.z - expected).abs() < s(0.1),
            "came to rest at {}, expected about {expected}",
            end.player.origin.z
        );
    }

    #[test]
    fn the_step_function_does_not_mutate_its_input() {
        let before = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));
        let snapshot = before;
        let _ = step(
            &before,
            &UserCmd::still(8),
            &EmptyWorld,
            &PhysicsProfile::default(),
        );
        assert_eq!(before.checksum(), snapshot.checksum());
    }

    #[test]
    fn clip_velocity_pushes_slightly_out_of_the_plane() {
        let profile = PhysicsProfile::vq3();
        let v = vec3(s(0.0), s(0.0), s(-100.0));
        let clipped = clip_velocity(v, num::UP, &profile);
        // Overclip means the result is not merely zeroed against the plane:
        // it retains a small outward component. This is the mechanism behind
        // overbounce, so it is asserted rather than assumed.
        assert!(
            clipped.z > s(0.0),
            "expected outward push, got {}",
            clipped.z
        );
    }
}

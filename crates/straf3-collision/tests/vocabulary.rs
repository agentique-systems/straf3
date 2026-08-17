//! The emergent vocabulary, measured on brush geometry and pinned.
//!
//! Spec goal 2 and criterion 3: ramp boost, overbounce, edge clip, step-up and
//! slide each get a test that fails if the behaviour disappears, and a lab
//! measurement that quantifies it. This is the first half. The worlds are
//! `straf3_collision::testbed`, which is also what the lab measures in, so the
//! two halves describe the same geometry rather than two similar ones.
//!
//! # These tests answer a question before they pin an answer
//!
//! The wave brief listed five behaviours "Q3 had" and asked whether this tree
//! actually has them. That is not rhetorical: three of the five turned out to
//! behave differently here than the folklore says, and one does not occur at
//! all. Each test below therefore states what was *measured*, with the sweep
//! that measured it, rather than asserting what Quake 3 is reputed to do.
//!
//! The headline, since a reader deserves it before the details:
//!
//! | behaviour | occurs? | how |
//! |---|---|---|
//! | ramp boost | **yes, but not by traversing a ramp** | a fall redirected along the slope, in the overbounce state |
//! | overbounce | **yes, exactly** | a grounded player carrying downward speed is relaunched at that speed |
//! | edge clip | **no** | running off a ledge costs nothing at any speed |
//! | step-up | **yes, and it is free** | STEPSIZE costs zero speed; one unit taller stops the player dead |
//! | slide | **yes, exactly at the constant** | the flip is at 45.57°, which is `acos(0.7)` |
//!
//! # Ramp boost and overbounce are one mechanism
//!
//! This is the finding that reorganises the rest of the file, so it is stated
//! once here rather than four times below.
//!
//! `PM_WalkMove` ends with Q3's "don't decrease velocity when going up or down a
//! slope" — `step.rs`'s
//!
//! ```text
//! let vel = p.velocity.length();
//! p.velocity = clip_velocity(p.velocity, self.ground_normal, self.profile);
//! p.velocity = normalize(p.velocity).0 * vel;
//! ```
//!
//! It is written for a player walking along a surface, where it means "the
//! direction follows the ground, the magnitude does not notice the ground
//! tilted". But it runs on **whatever velocity the player has**, including a
//! large *downward* one. Clip a 600-ups fall against a floor and `overclip`
//! leaves 0.6 ups pointing up; rescale that to its original length and the
//! player is travelling **600 ups upward**. That is overbounce, and it is not
//! an approximation of Q3's — it is the same three lines.
//!
//! On a ramp the same three lines redirect the fall *along the slope* instead of
//! straight up, at full magnitude. That is ramp boost.
//!
//! Both therefore need the same precondition: **walkable ground underfoot at the
//! start of a command, while still carrying downward speed**. Reaching it by
//! playing is the rare part, and
//! [`overbounce_is_reachable_by_falling_and_is_rare`] measures how rare.
//!
//! ## `overclip` is load-bearing in exactly one case, and it is not the common one
//!
//! `profile.rs` says the `overclip` excess "is the direct cause of overbounce and
//! ramp boost behaviour". Measured, that is true of overbounce **only when the
//! velocity is exactly perpendicular to the surface**, and false otherwise —
//! a correction worth stating plainly because it is the kind of comment a
//! reader trusts.
//!
//! The reason is that the rescale needs a direction to rescale, and it takes it
//! from whatever the clip left behind. Falling straight down onto a flat floor,
//! the clip leaves *nothing* except the `overclip` excess, so the excess is the
//! entire direction and setting `overclip` to 1.0 leaves a zero vector and a
//! stopped player. Add any horizontal velocity, or tilt the floor, and the clip
//! already leaves a substantial tangential component — the excess is then a
//! rounding term on a direction that exists without it, and switching it off
//! changes the answer by a fraction of a percent.
//!
//! So the mechanism behind the big numbers below is the **rescale**, not the
//! excess. That matters for anyone trying to tune this: `overclip` is not the
//! dial, and turning it to 1.0 would remove the vertical pop while leaving the
//! horizontal boost entirely intact.
//!
//! # A note on `friction: 0.0`
//!
//! Several tests below set it. That is a legitimate use of the data seam and not
//! a special case in the physics — `friction` is a number, so a test may choose
//! it — and it is used only where friction would mask the effect being measured
//! by bleeding speed on every command. Where the *player-facing* number matters,
//! canon friction is left on and the test says so.

use straf3_collision::testbed;
use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::state::GroundState;
use straf3_sim::world::World;
use straf3_sim::{PhysicsProfile, SimState, UserCmd, step};

/// 125 Hz, spec D2's default and the rate the client runs at.
const MS: u16 = 8;

fn still() -> UserCmd {
    UserCmd::still(MS)
}

fn hspeed(v: Vec3) -> Scalar {
    (v.x * v.x + v.y * v.y).sqrt()
}

/// Canon VQ3 with friction switched off. See the module docs.
fn frictionless() -> PhysicsProfile {
    PhysicsProfile {
        friction: s(0.0),
        ..PhysicsProfile::vq3()
    }
}

/// The same profile with `overclip` at its neutral value.
///
/// 1.0 means "clip exactly onto the plane". Used below as a one-constant A/B in
/// both directions: it *does* disable the perpendicular overbounce, and it
/// pointedly *does not* disable the horizontal boost or the ramp redirect. See
/// the module docs — that asymmetry is a correction to `profile.rs`'s comment,
/// and it is asserted in both forms rather than assumed in either.
fn without_overclip(p: PhysicsProfile) -> PhysicsProfile {
    PhysicsProfile {
        overclip: s(1.0),
        ..p
    }
}

/// Drop the player onto a world and wait for them to come to rest.
fn settle<W: World>(world: &W, p: &PhysicsProfile, from: Vec3) -> SimState {
    let mut st = SimState::spawned_at(from, s(0.0));
    for _ in 0..600 {
        st = step(&st, &still(), world, p);
        if st.player.ground.is_grounded() && st.player.velocity.length() < s(1.0) {
            return st;
        }
    }
    panic!("never settled from {from:?}");
}

/// A player standing on `surface_z` at `x`, carrying `velocity`.
///
/// The origin is placed exactly where the mover leaves a resting player —
/// `SURFACE_CLIP_EPSILON` above the surface, 24 units under the origin — because
/// that is the state a fall actually produces, not a convenient fiction. The
/// ground probe reaches 0.25 below the origin, so from here it finds ground.
fn resting_at(x: Scalar, surface_z: Scalar, normal: Vec3, velocity: Vec3) -> SimState {
    let mut st = SimState::spawned_at(vec3(x, s(0.0), surface_z + s(24.125)), s(0.0));
    st.player.ground = GroundState::Grounded { normal };
    st.player.velocity = velocity;
    st
}

// ═══ overbounce ════════════════════════════════════════════════════════════

/// **Overbounce occurs, and it returns the fall exactly.**
///
/// A player who is on walkable ground at the start of a command while still
/// carrying downward speed is launched upward at *precisely* that speed. Not
/// approximately, not a fraction: 600 ups down becomes 600 ups up.
///
/// Two-sided by construction. The lower bound catches the mechanism being
/// deleted; the absolute value catches it being got wrong — a version that
/// merely reflected the `overclip` excess would give 0.6 ups, and a version that
/// zeroed the velocity would give nothing.
///
/// The A/B is `overclip`. At 1.0 the clip lands exactly on the plane, the
/// rescale has a zero vector to normalise, and the player simply stops. This is
/// the one case where `profile.rs`'s "the excess is the direct cause of
/// overbounce" holds exactly — because a perpendicular fall leaves the excess as
/// the entire remaining direction. Every other form of the behaviour survives
/// `overclip = 1.0`; see the module docs.
#[test]
fn overbounce_relaunches_a_grounded_player_at_their_own_falling_speed() {
    let p = PhysicsProfile::vq3();
    let world = testbed::floor();

    for fall in [s(-100.0), s(-300.0), s(-600.0), s(-1000.0)] {
        let before = resting_at(
            s(0.0),
            testbed::FLOOR_TOP,
            vec3(s(0.0), s(0.0), s(1.0)),
            vec3(s(0.0), s(0.0), fall),
        );
        assert!(before.player.ground.is_grounded());

        let after = step(&before, &still(), &world, &p);
        assert!(
            (after.player.velocity.z - (-fall)).abs() < s(0.01),
            "a {} ups fall relaunched at {} ups; overbounce returns the fall exactly",
            -fall,
            after.player.velocity.z
        );
        assert!(
            !after.player.ground.is_on_plane(),
            "the launch must throw the player off the ground, got {:?}",
            after.player.ground
        );

        // …and with the overclip excess removed, nothing at all happens.
        let neutral = step(&before, &still(), &world, &without_overclip(p));
        assert!(
            neutral.player.velocity.length() < s(0.001),
            "with overclip at 1.0 the player should simply stop, got {:?}",
            neutral.player.velocity
        );
    }
}

/// **Overbounce is reachable by falling, and it is rare.**
///
/// The mechanism above needs the player to be *grounded while still falling*,
/// which sounds unreachable: a landing normally clips the velocity flat on the
/// same command it happens. It is reachable when a command ends with the feet
/// inside the ground probe's 0.25 units but above the surface — the sweep did
/// not hit, so the velocity was never clipped, and the end-of-command ground
/// trace finds ground anyway.
///
/// That is a 0.25-unit window in a fall that covers `|v| * 0.008` units per
/// command, so it happens on a narrow band of drop heights and not at all on the
/// heights between. **151 of the 1920 heights** between 16 and 256 units, in
/// 0.125-unit steps, overbounce: 7.9 %, in bands that get sparser as the fall
/// gets faster.
///
/// The count is pinned exactly and in both directions. A change that made
/// overbounce impossible drives it to zero; a change that made every landing
/// bounce drives it to 1920. Either is a different game.
#[test]
fn overbounce_is_reachable_by_falling_and_is_rare() {
    let p = PhysicsProfile::vq3();
    let world = testbed::floor();

    let bounces_from = |height: Scalar| {
        let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(24.125) + height), s(0.0));
        for _ in 0..300 {
            st = step(&st, &still(), &world, &p);
            if st.player.velocity.z > s(50.0) {
                return Some(st.player.velocity.z);
            }
            if st.player.ground.is_grounded() && st.player.velocity.z.abs() < s(1.0) {
                return None;
            }
        }
        None
    };

    let mut found = 0usize;
    let mut sampled = 0usize;
    let mut height = s(16.0);
    while height < s(256.0) {
        if bounces_from(height).is_some() {
            found += 1;
        }
        sampled += 1;
        height += s(0.125);
    }

    assert_eq!(sampled, 1920, "the sweep itself changed shape");
    assert_eq!(
        found, 151,
        "{found} of {sampled} drop heights overbounce, and 151 were frozen. \
         Zero means the mechanism is gone; {sampled} means every landing now \
         bounces. Anything else means the size of the window moved — check \
         `ground_trace_probe` (0.25) and `SURFACE_CLIP_EPSILON` (0.125)."
    );

    // And the specific heights, so a failure is a number and not a census.
    // A 16-unit drop reaches 160 ups (`sqrt(2 * 800 * 16)`), and comes back up
    // at 160 ups. Half a unit lower and the same fall lands flat.
    let bounced = bounces_from(s(16.0)).expect("a 16-unit drop overbounces");
    assert!(
        (bounced - s(160.0)).abs() < s(0.5),
        "a 16-unit drop relaunched at {bounced} ups, expected the impact speed of 160"
    );
    assert!(
        bounces_from(s(16.5)).is_none(),
        "a 16.5-unit drop overbounced; the window is meant to be narrow"
    );
}

/// **Overbounce carrying speed is a permanent horizontal boost**, and this is
/// the form a player would actually meet.
///
/// With horizontal velocity in play the rescale does not point the player
/// upward — it points them very nearly along their existing heading, and hands
/// them the *whole* magnitude of the combined vector. A player running at 300
/// ups who overbounces off a 16-unit drop leaves the ground at
/// `sqrt(300² + 160²) = 340` ups, and, with no friction to take it back, keeps
/// it: 340 ups, on the ground, for as long as they like.
///
/// The scale of that matters for the doctrine argument rather than the
/// arithmetic: from a 509-unit drop the same player leaves at 951 ups. It is
/// the largest single speed gain available anywhere in the current movement
/// vocabulary, and it is available by accident.
#[test]
fn overbounce_turns_a_fall_into_permanent_horizontal_speed() {
    let p = frictionless();
    let world = testbed::floor();

    let run_off = |profile: &PhysicsProfile, height: Scalar, entry: Scalar| {
        let mut st = SimState::spawned_at(vec3(s(-2000.0), s(0.0), s(24.125) + height), s(0.0));
        st.player.velocity = vec3(entry, s(0.0), s(0.0));
        for _ in 0..60 {
            st = step(&st, &still(), &world, profile);
        }
        st
    };

    let boosted = run_off(&p, s(16.0), s(300.0));
    assert!(
        boosted.player.ground.is_grounded(),
        "expected to be running along the floor, got {:?}",
        boosted.player.ground
    );
    assert!(
        (hspeed(boosted.player.velocity) - s(340.0)).abs() < s(0.05),
        "300 ups over a 16-unit drop settled at {} ups; the fall contributes its own \
         160 ups in quadrature, so the answer is sqrt(300^2 + 160^2) = 340",
        hspeed(boosted.player.velocity)
    );

    // The gain is kept, not borrowed: fifty commands later it is still there.
    // (The check above already runs 60 commands; this states the claim.)
    assert!(
        boosted.player.velocity.z.abs() < s(1.0),
        "still moving vertically at {}",
        boosted.player.velocity.z
    );

    // And `overclip` is NOT what does this — see the module docs. With the
    // excess removed the boost is unchanged to within a twentieth of a ups,
    // because the clip already leaves a large horizontal component for the
    // rescale to work on. Asserted rather than omitted: `profile.rs` claims
    // overclip causes ramp boost and overbounce, and for this — the form a
    // player actually meets — it does not.
    let without = run_off(&without_overclip(p), s(16.0), s(300.0));
    assert!(
        (hspeed(without.player.velocity) - hspeed(boosted.player.velocity)).abs() < s(0.05),
        "removing the overclip excess changed the horizontal boost from {} to {} ups. \
         It is not supposed to matter here — if it now does, the mechanism has moved \
         from the rescale to the excess and the module docs need rewriting.",
        hspeed(boosted.player.velocity),
        hspeed(without.player.velocity)
    );

    // What overclip *is* load-bearing for is the perpendicular case, which
    // `overbounce_relaunches_a_grounded_player_at_their_own_falling_speed`
    // pins. A drop with no horizontal speed at all, same height, same profile:
    // there the excess is the whole of the direction and 1.0 stops the player.
    let straight_down = resting_at(
        s(0.0),
        testbed::FLOOR_TOP,
        vec3(s(0.0), s(0.0), s(1.0)),
        vec3(s(0.0), s(0.0), s(-160.0)),
    );
    let popped = step(&straight_down, &still(), &world, &p);
    let flattened = step(&straight_down, &still(), &world, &without_overclip(p));
    assert!(popped.player.velocity.z > s(159.0) && flattened.player.velocity.length() < s(0.001));
}

// ═══ ramp boost ════════════════════════════════════════════════════════════

/// **Walking onto a ramp never gains speed.** It costs `cos(angle)`.
///
/// This is the negative the brief asked for, with the sweep behind it. Across
/// every angle from 5° to 45° and every entry speed from 200 to 900 ups, the
/// peak total speed over the whole crossing is *exactly* the entry speed — the
/// ratio is 1.00000, not 1.0001. There is no traversal boost in this tree.
///
/// What there is, is a one-time loss at the seam. Crossing from the flat
/// approach onto the slope, the slide solver clips the horizontal velocity
/// against the ramp plane and — unlike `walk_move`'s clip — does **not** rescale
/// it, so the into-plane component is simply gone. The player arrives on the
/// ramp at `entry * cos(angle)`: 600 ups onto a 26° ramp becomes 539.28.
///
/// Both halves are pinned. Delete the loss and the ratio goes to 1; turn the
/// loss into a gain and the peak assertion fails.
///
/// Note what this does *not* contradict:
/// `crates/straf3-sim/tests/movement.rs`'s
/// `a_walkable_ramp_preserves_speed_through_the_ground_plane_clip` is about a
/// player *already standing on* a slope, where the ground normal is the ramp's
/// and the rescale does preserve the magnitude. Both are true. The difference
/// is whether the ramp is the ground you are on or the geometry you are running
/// into, and only brush geometry can tell them apart — which is why that test's
/// analytic infinite plane could not have found this.
#[test]
fn walking_onto_a_ramp_never_gains_speed_and_costs_the_into_plane_component() {
    let p = frictionless();

    for degrees in [s(5.0), s(10.0), s(15.0), s(20.0), s(30.0), s(45.0)] {
        let world = testbed::ramp(degrees);
        for entry in [s(300.0), s(600.0), s(900.0)] {
            let mut st = settle(&world, &p, vec3(s(-128.0), s(0.0), s(64.0)));
            st.player.velocity = vec3(entry, s(0.0), s(0.0));

            let mut peak = entry;
            for _ in 0..150 {
                st = step(&st, &still(), &world, &p);
                peak = peak.max(st.player.velocity.length());
                if st.player.origin.x > s(900.0) {
                    break;
                }
            }
            assert!(
                peak <= entry + s(0.001),
                "a {degrees}-degree ramp boosted a {entry} ups run to {peak}; \
                 traversing a ramp must not gain speed"
            );
        }
    }

    // The cost, pinned as a number rather than an inequality. 26 degrees is the
    // angle `movement.rs` uses for its walkable-ramp test, so the two are
    // directly comparable: cos(26 degrees) = 0.89879.
    let world = testbed::ramp(s(26.0));
    let cos = testbed::ramp_normal(s(26.0)).z;
    for entry in [s(200.0), s(400.0), s(600.0)] {
        let mut st = settle(&world, &p, vec3(s(-128.0), s(0.0), s(64.0)));
        st.player.velocity = vec3(entry, s(0.0), s(0.0));
        for _ in 0..150 {
            st = step(&st, &still(), &world, &p);
            if st.player.origin.x > s(500.0) {
                break;
            }
        }
        let kept = st.player.velocity.length() / entry;
        assert!(
            (kept - cos).abs() < s(0.001),
            "crossing onto a 26-degree ramp at {entry} ups kept {kept} of the speed; \
             the into-plane component is lost, so the answer is cos(26) = {cos}"
        );
    }
}

/// **Ramp boost occurs — as a fall redirected along the slope.**
///
/// The same three lines that produce overbounce on a floor produce this on a
/// ramp: a player who is on the slope at the start of a command while carrying
/// downward speed has that speed turned to point *down the slope*, at full
/// magnitude. A 1000 ups fall onto a 45° ramp leaves them travelling 711.63 ups
/// along it, of which 503.20 is horizontal — horizontal speed they did not have
/// a command earlier.
///
/// The two-sided pin is the vector, not the magnitude, and that is deliberate:
/// the direction is what distinguishes a redirect from a bounce, and a version
/// that sent the player *up* the slope would be a different mechanic with the
/// same speed.
#[test]
fn a_ramp_boost_is_a_fall_redirected_along_the_slope() {
    let p = PhysicsProfile::vq3();

    // (angle, fall speed, expected velocity after one command)
    let cases = [
        (s(26.0), s(-1000.0), vec3(s(-396.92), s(0.0), s(-192.59))),
        (s(45.0), s(-1000.0), vec3(s(-503.20), s(0.0), s(-503.20))),
        (s(45.0), s(-600.0), vec3(s(-303.20), s(0.0), s(-303.20))),
    ];

    for (degrees, fall, want) in cases {
        let world = testbed::ramp(degrees);
        let normal = testbed::ramp_normal(degrees);
        let x = s(400.0);
        let surface = x * (-normal.x / normal.z);

        let before = resting_at(x, surface, normal, vec3(s(0.0), s(0.0), fall));
        let after = step(&before, &still(), &world, &p);
        let got = after.player.velocity;

        assert!(
            (got - want).length() < s(1.0),
            "a {fall} ups fall on a {degrees}-degree ramp became {got:?}, expected {want:?}"
        );
        // The horizontal speed is new: the player had none.
        assert!(
            hspeed(got) > s(150.0),
            "no horizontal speed was produced: {got:?}"
        );
        // And it points down the slope, not up it. The ramp rises with +X.
        assert!(
            got.x < s(0.0),
            "the boost sent the player *up* the slope, which is a different mechanic: {got:?}"
        );

        // `overclip` is not the cause here, and the module docs explain why: a
        // tilted plane leaves a large tangential component for the rescale even
        // at 1.0, so the excess is a rounding term on a direction that already
        // exists. Pinned as a near-equality rather than left unsaid, because
        // `profile.rs` claims otherwise and a reader should meet the correction
        // where the behaviour is.
        let neutral = step(&before, &still(), &world, &without_overclip(p));
        assert!(
            (hspeed(neutral.player.velocity) - hspeed(got)).abs() < s(1.0),
            "removing the overclip excess changed the ramp boost from {} to {} ups \
             horizontal; on a slope it is the rescale, not the excess, that does this",
            hspeed(got),
            hspeed(neutral.player.velocity)
        );
    }
}

// ═══ edge clip ═════════════════════════════════════════════════════════════

/// **Edge clipping does not occur.** Running off a ledge costs nothing.
///
/// The brief asked at what approach speeds and offsets the hull catches on a
/// ledge corner and what it costs. The measured answer, swept over drops of 32,
/// 64 and 128 units and speeds of 200 to 900 ups: the worst per-command
/// horizontal loss is **exactly zero** in every combination. The box leaves the
/// lip and falls; the corner never touches it.
///
/// That is the correct outcome and it is not an accident. `q3map`'s bevels are
/// what a swept box needs at an edge, and `hull.rs` runs them; the failure mode
/// they prevent — stopping in mid-air a hull-width from anything visible — is
/// exactly what "edge clip" would look like. `trace.rs`'s
/// `the_bevels_are_what_stop_the_box_at_the_wall_instead_of_short_of_it`
/// measures the same property from the other side.
///
/// What *does* exist, and is worth a player knowing, is the release offset:
/// contact is kept until the player's origin is one hull half-width — 15 units
/// — past the lip. You run half a body past the edge before you drop. That is
/// Quake's coyote time, it is a consequence of the ground probe testing the
/// whole 30-unit-wide hull, and it is perceivable, which is more than can be
/// said for overbounce.
#[test]
fn running_off_a_ledge_costs_nothing_and_releases_one_hull_width_past_the_lip() {
    let p = frictionless();

    for drop in [s(32.0), s(64.0), s(128.0)] {
        let world = testbed::ledge(drop);
        for entry in [s(200.0), s(400.0), s(600.0), s(900.0)] {
            let mut st = settle(&world, &p, vec3(s(-256.0), s(0.0), s(64.0)));
            st.player.velocity = vec3(entry, s(0.0), s(0.0));

            let mut worst_loss = s(0.0);
            let mut previous = hspeed(st.player.velocity);
            let mut at_release = None;
            for _ in 0..300 {
                let was_on_ground = st.player.ground.is_on_plane();
                st = step(&st, &still(), &world, &p);
                let now = hspeed(st.player.velocity);
                worst_loss = worst_loss.max(previous - now);
                previous = now;
                if was_on_ground && !st.player.ground.is_on_plane() && at_release.is_none() {
                    at_release = Some(now);
                }
                if st.player.origin.x > s(600.0) {
                    break;
                }
            }
            assert!(
                worst_loss < s(0.001),
                "a {drop}-unit ledge cost {worst_loss} ups at {entry} ups; edge clipping \
                 is not supposed to happen here, and if it has started, suspect the \
                 bevels in hull.rs"
            );
            assert!(
                st.player.ground.is_grounded(),
                "never landed on the lower floor: {:?}",
                st.player.ground
            );
            // The edge itself is free: leaving it costs nothing at all.
            let released = at_release.expect("never left the upper floor");
            assert!(
                (released - entry).abs() < s(0.001),
                "left the lip at {released} ups having approached at {entry}"
            );
            // The *landing* is a different question, and it may well add speed:
            // a drop of this size lands in the overbounce window at some of
            // these entry speeds, which is the two mechanisms composing rather
            // than an edge effect. What it must never do is take speed away.
            assert!(
                hspeed(st.player.velocity) >= entry - s(0.001),
                "a {drop}-unit ledge left the player slower ({} ups) than the {entry} \
                 they approached with",
                hspeed(st.player.velocity)
            );
        }
    }

    // The release offset, pinned on both sides of 15.
    let world = testbed::ledge(s(64.0));
    let standing_at = |x: Scalar| {
        let st = resting_at(
            x,
            testbed::FLOOR_TOP,
            vec3(s(0.0), s(0.0), s(1.0)),
            Vec3::ZERO,
        );
        step(&st, &still(), &world, &PhysicsProfile::vq3())
            .player
            .ground
    };
    assert!(
        standing_at(s(14.9)).is_grounded(),
        "the player lost their footing before their box had cleared the lip"
    );
    assert!(
        !standing_at(s(15.1)).is_on_plane(),
        "the player kept their footing with their whole box past the lip"
    );
}

// ═══ step-up ═══════════════════════════════════════════════════════════════

/// **Step-up is free, and the cliff at the top of it is total.**
///
/// Two measured facts, and the gap between them is the whole of the behaviour:
///
/// - A step of 18.1 units or less is climbed at **zero speed cost**, at any
///   speed up to 900 ups. Not "little" — the horizontal speed after the climb
///   is bit-for-bit what it was before. `step_slide_move` restores the
///   pre-attempt velocity and re-runs the move from the lifted position, and
///   the drop back down clips against a floor normal, which a horizontal
///   velocity is already parallel to.
/// - A step of 18.125 units stops the player **dead**. 900 ups to zero, in one
///   command, against a lip a fifth of an inch taller than the one they just ran
///   up for free.
///
/// 18.125 is `STEPSIZE` plus `SURFACE_CLIP_EPSILON`, which is where the boundary
/// has to be: the lift is held clear of what it hits by the epsilon, so the
/// tallest surface the player can be placed on top of is `step_height` plus that
/// clearance. Pinning both sides is what makes `step_height` load-bearing —
/// `movement.rs` pins 18 against 24, which a solver that climbed anything up to
/// 23 would also pass.
///
/// The cliff is worth flagging on the vision's terms: it is legible (you can see
/// the lip you failed to climb) but it is not *graduated* — there is no partial
/// climb, no scrape, no speed penalty band. One unit of map geometry is the
/// difference between keeping 900 ups and keeping none.
#[test]
fn a_step_within_stepsize_is_free_and_one_unit_taller_stops_the_player_dead() {
    let p = frictionless();

    let run_into = |height: Scalar, entry: Scalar| {
        let world = testbed::step(height);
        let mut st = settle(&world, &p, vec3(s(-64.0), s(0.0), s(64.0)));
        st.player.velocity = vec3(entry, s(0.0), s(0.0));
        for _ in 0..120 {
            st = step(&st, &still(), &world, &p);
            if st.player.origin.x > s(150.0) {
                break;
            }
        }
        st
    };

    for entry in [s(400.0), s(600.0), s(900.0)] {
        for height in [s(4.0), s(12.0), s(18.0), s(18.1)] {
            let st = run_into(height, entry);
            assert!(
                (hspeed(st.player.velocity) - entry).abs() < s(0.001),
                "climbing a {height}-unit step at {entry} ups cost {} ups; step-up is free",
                entry - hspeed(st.player.velocity)
            );
            assert!(
                st.player.origin.x > testbed::STEP_RISER_X,
                "never got up a {height}-unit step at {entry} ups: x={}",
                st.player.origin.x
            );
            assert!(
                (st.player.origin.z - (height + s(24.125))).abs() < s(0.2),
                "ended at z={} rather than on top of the {height}-unit step",
                st.player.origin.z
            );
        }

        // One eighth of a unit taller — `SURFACE_CLIP_EPSILON` past `STEPSIZE` —
        // and the same run ends against the riser with nothing left.
        let stopped = run_into(s(18.125), entry);
        assert!(
            hspeed(stopped.player.velocity) < s(0.001),
            "an 18.125-unit step left {} ups; it should stop the player dead",
            hspeed(stopped.player.velocity)
        );
        assert!(
            stopped.player.origin.x < testbed::STEP_RISER_X,
            "climbed an 18.125-unit step, which is taller than STEPSIZE plus the \
             clip epsilon: x={}",
            stopped.player.origin.x
        );
    }
}

/// **A ledge cannot be mantled while still rising**, unless there is ground
/// right underneath.
///
/// `step.rs`'s `STEP_MIN_NORMAL` rule, which Q3 comments as "otherwise a jump
/// into a wall would climb it". Measured with the player's feet at 82 and a lip
/// at 88 to 96 — six to fourteen units up, comfortably inside `STEPSIZE` — and
/// the floor 82 units below, so the step-down probe finds nothing:
///
/// - falling or level (`vz <= 0`): the player is placed on top of the lip;
/// - rising (`vz > 0`): refused, and they slide up the face instead.
///
/// This is one of the few things in this file that is *readable by design*
/// rather than by accident: it says you mantle a ledge on the way down, not on
/// the way up, and a player can feel the difference between the two attempts.
#[test]
fn a_ledge_is_mantled_falling_and_refused_while_rising() {
    let p = frictionless();

    for lip in [s(88.0), s(92.0), s(96.0)] {
        let world = testbed::step(lip);
        let approach = |vz: Scalar| {
            // Feet at 82: the origin is 24 above them, and the floor is far
            // enough below that the step-down probe finds nothing.
            let mut st = SimState::spawned_at(vec3(s(-18.0), s(0.0), s(106.0)), s(0.0));
            st.player.velocity = vec3(s(200.0), s(0.0), vz);
            for _ in 0..30 {
                st = step(&st, &still(), &world, &p);
            }
            st
        };

        for falling in [s(-200.0), s(-50.0), s(0.0)] {
            let st = approach(falling);
            assert!(
                st.player.origin.x > testbed::STEP_RISER_X && st.player.ground.is_grounded(),
                "a lip {} units above the feet was not mantled at vz={falling}: x={} {:?}",
                lip - s(82.0),
                st.player.origin.x,
                st.player.ground
            );
        }
        for rising in [s(50.0), s(200.0)] {
            let st = approach(rising);
            assert!(
                st.player.origin.x < testbed::STEP_RISER_X,
                "a rising player was pulled onto a lip {} units up at vz={rising}; \
                 STEP_MIN_NORMAL is meant to refuse that, and without it a jump into \
                 a wall climbs it: x={}",
                lip - s(82.0),
                st.player.origin.x
            );
        }
    }
}

// ═══ slide ═════════════════════════════════════════════════════════════════

/// **The walk/slide flip is exactly where `min_walk_normal` says it is.**
///
/// `min_walk_normal` is 0.7, and `acos(0.7)` is 45.573°. Measured on brush
/// ramps: 45.57° is walkable (`normal.z = 0.700037`) and 45.60° slides
/// (`normal.z = 0.699663`). The constant is not approximately the boundary, it
/// *is* the boundary, to a third of a degree — which is as fine as the geometry
/// can be asked.
///
/// The brief asked for 45 against 46, and those are here too: a 45° ramp is
/// walked and a 46° ramp is slid down. The finer pair is what proves the
/// boundary is at the constant rather than merely somewhere between them.
#[test]
fn the_walk_slide_flip_is_where_min_walk_normal_says_it_is() {
    let p = PhysicsProfile::vq3();

    let regime = |degrees: Scalar| {
        let world = testbed::ramp(degrees);
        let normal = testbed::ramp_normal(degrees);
        let x = s(400.0);
        let surface = x * (-normal.x / normal.z);
        let mut st = SimState::spawned_at(vec3(x, s(0.0), surface + s(120.0)), s(0.0));
        let mut ever_sliding = false;
        for _ in 0..60 {
            st = step(&st, &still(), &world, &p);
            ever_sliding |= matches!(st.player.ground, GroundState::Sliding { .. });
        }
        (normal.z, ever_sliding, st)
    };

    for walkable in [s(44.0), s(45.0), s(45.5), s(45.57)] {
        let (nz, slid, st) = regime(walkable);
        assert!(
            nz >= p.min_walk_normal,
            "{walkable} degrees has normal.z {nz}, which is below min_walk_normal \
             {} — the fixture, not the physics, has moved",
            p.min_walk_normal
        );
        assert!(
            !slid && st.player.ground.is_grounded(),
            "{walkable} degrees (normal.z {nz}) slid, but min_walk_normal is {} so it \
             must be walkable",
            p.min_walk_normal
        );
    }

    for sliding in [s(45.6), s(46.0), s(47.0), s(50.0)] {
        let (nz, slid, _) = regime(sliding);
        assert!(
            nz < p.min_walk_normal,
            "{sliding} degrees has normal.z {nz}, at or above min_walk_normal {}",
            p.min_walk_normal
        );
        assert!(
            slid,
            "{sliding} degrees (normal.z {nz}) was walked on, but it is steeper than \
             min_walk_normal {} — collapsing Sliding into Grounded deletes ramp \
             sliding entirely",
            p.min_walk_normal
        );
    }
}

/// A steep brush ramp slides without friction, and a shallow one does not.
///
/// `movement.rs`'s `a_steep_ramp_slides_without_friction` establishes this on an
/// analytic infinite plane. Repeating it on compiled brushes is not redundancy:
/// the analytic plane has no foot, no top and no seam, so it cannot show that a
/// player who slides to the bottom of a real ramp arrives on the flat still
/// carrying the speed the slope gave them. That is the player-facing behaviour —
/// a steep ramp is a source of speed, not a wall.
///
/// Measured: released 120 units above a 46° slope, the player reaches the flat
/// approach at 351 ups. On a 44° ramp — a degree and a half shallower, and
/// walkable — the same release ends at rest, because friction applies the moment
/// the surface is standable.
#[test]
fn a_steep_brush_ramp_is_a_source_of_speed_and_a_walkable_one_is_not() {
    let p = PhysicsProfile::vq3();

    let released_above = |degrees: Scalar| {
        let world = testbed::ramp(degrees);
        let normal = testbed::ramp_normal(degrees);
        let x = s(400.0);
        let surface = x * (-normal.x / normal.z);
        let mut st = SimState::spawned_at(vec3(x, s(0.0), surface + s(120.0)), s(0.0));
        for _ in 0..200 {
            st = step(&st, &still(), &world, &p);
        }
        st
    };

    let slid = released_above(s(46.0));
    assert!(
        (hspeed(slid.player.velocity) - s(351.8)).abs() < s(5.0),
        "sliding down a 46-degree ramp reached the flat at {} ups, expected about 352 \
         — the slide is meant to convert the drop into speed and keep it",
        hspeed(slid.player.velocity)
    );
    assert!(
        slid.player.origin.x < s(0.0),
        "never reached the bottom: x={}",
        slid.player.origin.x
    );

    let walked = released_above(s(44.0));
    assert!(
        hspeed(walked.player.velocity) < s(1.0),
        "a 44-degree ramp is walkable, so friction should bring the player to rest; \
         they are still moving at {} ups",
        hspeed(walked.player.velocity)
    );
    assert!(
        walked.player.origin.x > s(300.0),
        "a walkable ramp should hold the player where they landed, not shed them to \
         x={}",
        walked.player.origin.x
    );
}

//! Headless replay: run a recorded command file through the *windowed
//! build's* own code path, with no window and no GPU.
//!
//! # Why this exists (criteria 4 and 5)
//!
//! Criterion 4 asks whether the renderer changed anything below the seam. The
//! way to answer it without believing anybody is to run the same commands two
//! ways — through `straf3-headless`, and through this binary — and compare the
//! checksum at *every* tick, not just the last one. Spec rev 6 §R is the
//! reason for "every": in one measured case the final checksum matched across
//! builds while 29 of 1200 intermediate ones did not, so end-state-only
//! verification would have certified a run that had actually diverged.
//!
//! Criterion 5 asks whether frame pacing is decoupled from simulation
//! stepping. [`ReplayOptions::frame_ms`] is how that becomes a measurement:
//! feed the same commands under a hostile frame schedule (1 ms, 200 ms, 0 ms,
//! 37 ms…) and under a perfectly regular one, and the two command streams —
//! and therefore the two checksum streams — must be identical. They go through
//! the same [`crate::FixedStep`] the window uses, so this is a test of the
//! real accumulator rather than of a second one written for the test.
//!
//! # Why the parser is written again here
//!
//! `straf3-sim`'s `bin/headless.rs` owns the definition of this format, and it
//! is below the line and outside Wave 3's ownership — lifting the parser out
//! of it would mean editing a crate another seat has work in flight in. So
//! this is an independent implementation of the same directives, and the
//! equivalence of the two is checked *empirically*, by running the same
//! fixture through both binaries and diffing the output, rather than asserted
//! by sharing code. That is the stronger check anyway: shared code cannot
//! disagree, so it cannot demonstrate agreement.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::{Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles};

use crate::game::Game;
use crate::scene::WorldChoice;
use crate::tick::FixedStep;

/// The CSV header `straf3-headless --trace --csv` prints, character for
/// character, so the two outputs can be diffed directly.
pub const TRACE_HEADER: &str = "tick,time_ms,x,y,z,vx,vy,vz,speed,grounded,checksum";

/// A parsed command file.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// The command rate the run was recorded at — part of the physics (D2).
    pub rate: TickRate,
    /// The movement constants.
    pub profile: PhysicsProfile,
    /// Their name, as written in the file.
    pub profile_name: String,
    /// Which world to run in.
    pub world: WorldChoice,
    /// Spawn origin.
    pub spawn: Vec3,
    /// Spawn view yaw.
    pub yaw: Scalar,
    /// The commands, expanded from their repeat counts.
    pub cmds: Vec<UserCmd>,
}

/// Parse the `straf3-headless` input format.
///
/// Directives: `rate <hz>`, `profile <cpm|vq3>`, `world <empty|flat <z>|arena>`,
/// `spawn <x> <y> <z>`, `yaw <deg>`, and
/// `cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> [roll]`.
/// Blank lines and `#` comments are ignored.
///
/// `world arena` is the one directive `straf3-headless` does not have, because
/// the arena lives above the line and the headless runner is below it.
pub fn parse(text: &str) -> Result<Fixture, String> {
    let mut rate: Option<TickRate> = None;
    let mut profile = PhysicsProfile::cpm();
    let mut profile_name = "cpm".to_owned();
    let mut world = WorldChoice::Flat;
    let mut spawn = vec3(s(0.0), s(0.0), s(64.0));
    let mut yaw = s(0.0);
    let mut cmds = Vec::new();

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let at = |e: String| format!("line {}: {e}", lineno + 1);

        match f[0] {
            "rate" => {
                let hz: u32 = num(&f, 1).map_err(at)?;
                rate = Some(
                    TickRate::from_hz(hz)
                        .ok_or_else(|| at(format!("rate {hz} Hz is outside 1..=1000")))?,
                );
            }
            "profile" => {
                profile_name = (*f.get(1).unwrap_or(&"")).to_owned();
                // One table, shared with `--profile` (see `crate::profile`):
                // a name the command line accepts and the fixture format does
                // not would replay a recording under different physics from
                // the ones it was made under.
                profile = crate::profile::by_name(&profile_name).ok_or_else(|| {
                    at(format!(
                        "unknown profile `{profile_name}` ({})",
                        crate::profile::NAMES
                    ))
                })?;
            }
            "world" => {
                world = match f.get(1) {
                    Some(&"empty") => WorldChoice::Empty,
                    Some(&"flat") => {
                        // The height is accepted and must be zero: this build's
                        // flat world is the one `WorldChoice::Flat` names, and
                        // silently ignoring a different height would produce a
                        // run that looks reproducible and is not.
                        let z: Scalar = num(&f, 2).unwrap_or(s(0.0));
                        if z != s(0.0) {
                            return Err(at(format!(
                                "`world flat {z}`: this build only has the plane at \
                                 z=0; running it at {z} would silently be a different \
                                 world"
                            )));
                        }
                        WorldChoice::Flat
                    }
                    // `map` names whichever map this process has installed, and
                    // that is all a fixture can say about it: the fixture format
                    // carries no map identity, so replaying a `world map`
                    // recording against a *different* map — or a recompiled one
                    // — is not detectable here. Binding a recording to the
                    // map's collision digest is C6's job and is not done.
                    Some(&"map") => WorldChoice::Map,
                    // `arena` is deliberately refused rather than silently
                    // mapped onto something: the hardcoded arena is retired, so
                    // a recording made in it has no world to replay against, and
                    // answering with different geometry would hand back a
                    // different trace with a straight face.
                    Some(&"arena") => {
                        return Err(at(
                            "`world arena`: the hardcoded arena was retired, so this \
                             recording cannot be replayed — the geometry it was made \
                             in no longer exists"
                                .to_string(),
                        ));
                    }
                    other => {
                        return Err(at(format!(
                            "unknown world `{}` (empty|flat <z>|map)",
                            other.unwrap_or(&"")
                        )));
                    }
                };
            }
            "spawn" => {
                spawn = vec3(
                    num(&f, 1).map_err(&at)?,
                    num(&f, 2).map_err(&at)?,
                    num(&f, 3).map_err(&at)?,
                );
            }
            "yaw" => yaw = num(&f, 1).map_err(&at)?,
            "cmd" => {
                let rate = rate.ok_or_else(|| {
                    at(
                        "`rate` must be declared before the first `cmd` — the command \
                        rate is part of the physics (spec D2)"
                            .to_owned(),
                    )
                })?;
                let repeat: u32 = num(&f, 1).map_err(&at)?;
                let cmd = UserCmd {
                    duration_ms: rate.command_millis(),
                    forward_move: axis(&f, 2).map_err(&at)?,
                    right_move: axis(&f, 3).map_err(&at)?,
                    up_move: axis(&f, 4).map_err(&at)?,
                    buttons: parse_buttons(f.get(5).copied().unwrap_or("-")).map_err(&at)?,
                    // Angles are written as degrees and quantised on the way
                    // in (contract item C3). A file that was written by the
                    // recorder already holds representable angles, so this is
                    // the identity for them; a hand-written one is snapped to
                    // the nearest representable angle rather than being run at
                    // a precision no command could have carried.
                    view: ViewAngles::from_degrees(
                        num(&f, 6).unwrap_or(s(0.0)),
                        num(&f, 7).unwrap_or(yaw),
                        num(&f, 8).unwrap_or(s(0.0)),
                    ),
                };
                cmds.reserve(repeat as usize);
                for _ in 0..repeat {
                    cmds.push(cmd);
                }
            }
            other => return Err(at(format!("unknown directive `{other}`"))),
        }
    }

    Ok(Fixture {
        rate: rate.ok_or(
            "no `rate` declared: the command rate is a required, recorded simulation \
             parameter (spec D2)",
        )?,
        profile,
        profile_name,
        world,
        spawn,
        yaw,
        cmds,
    })
}

/// How to run a replay.
#[derive(Debug, Clone, Default)]
pub struct ReplayOptions {
    /// Print one line per tick rather than only the final state.
    pub trace: bool,
    /// Print in the CSV form `straf3-headless --csv` uses.
    pub csv: bool,
    /// The frame schedule, in whole wall milliseconds, cycled until the
    /// commands run out.
    ///
    /// Empty means "one frame per tick", the regular schedule. Anything else
    /// is the hostile schedule criterion 5 is measured against — and the
    /// output must be identical either way.
    pub frame_ms: Vec<u64>,
}

/// One line of the trace, in whichever form was asked for.
#[must_use]
pub fn trace_line(state: &SimState, csv: bool) -> String {
    let v = state.player.velocity;
    let speed = (v.x * v.x + v.y * v.y).sqrt();
    if csv {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{:#018x}",
            state.tick,
            state.time_ms,
            state.player.origin.x,
            state.player.origin.y,
            state.player.origin.z,
            v.x,
            v.y,
            v.z,
            speed,
            u8::from(state.player.ground.is_grounded()),
            state.checksum()
        )
    } else {
        format!(
            "tick {:>6}  t {:>8}ms  pos {:>10.3} {:>10.3} {:>10.3}  vel {:>10.3} {:>10.3} {:>10.3}  ups {speed:>8.3}  {}",
            state.tick,
            state.time_ms,
            state.player.origin.x,
            state.player.origin.y,
            state.player.origin.z,
            v.x,
            v.y,
            v.z,
            if state.player.ground.is_grounded() {
                "ground"
            } else {
                "air"
            }
        )
    }
}

/// Run a fixture and return the state at every tick, starting with the state
/// *before* the first command.
///
/// Commands are pulled by the accumulator, not by a bare loop: the frame
/// schedule decides how many are consumed each frame, exactly as the window
/// does. That is what makes this a measurement of criterion 5 rather than an
/// assertion about it.
///
/// # Errors
///
/// A frame schedule that cannot advance — every entry zero — would loop
/// forever, so it is refused.
pub fn replay(fixture: &Fixture, options: &ReplayOptions) -> Result<Vec<SimState>, String> {
    let world = fixture.world.or_fallback();
    let mut game = Game::new(
        world.world(),
        fixture.profile,
        fixture.rate,
        fixture.spawn,
        fixture.yaw,
    );

    let schedule: Vec<u64> = if options.frame_ms.is_empty() {
        vec![u64::from(fixture.rate.command_millis())]
    } else {
        if options.frame_ms.iter().all(|ms| *ms == 0) {
            return Err(
                "every frame in the schedule is 0 ms, so no tick would ever come due".to_owned(),
            );
        }
        options.frame_ms.clone()
    };

    let mut trace = Vec::with_capacity(fixture.cmds.len() + 1);
    trace.push(*game.state());

    let mut step = FixedStep::new(fixture.rate);
    let mut next = 0;
    let mut frame = 0usize;
    while next < fixture.cmds.len() {
        let delta = schedule[frame % schedule.len()];
        frame += 1;
        let due = step.advance(delta);
        for _ in 0..due {
            let Some(cmd) = fixture.cmds.get(next) else {
                break;
            };
            game.apply(cmd);
            next += 1;
            trace.push(*game.state());
        }
    }

    Ok(trace)
}

fn num<T: std::str::FromStr>(f: &[&str], i: usize) -> Result<T, String> {
    f.get(i)
        .ok_or_else(|| format!("missing field {i}"))?
        .parse()
        .map_err(|_| format!("field {i} (`{}`) is not a number", f[i]))
}

/// A movement axis, clamped to Quake's signed-byte range.
fn axis(f: &[&str], i: usize) -> Result<i8, String> {
    let v: i32 = num(f, i)?;
    Ok(v.clamp(-127, 127) as i8)
}

fn parse_buttons(spec: &str) -> Result<Buttons, String> {
    if spec == "-" || spec.is_empty() {
        return Ok(Buttons::NONE);
    }
    let mut b = Buttons::NONE;
    for name in spec.split('+') {
        b = b.with(match name {
            "jump" => Buttons::JUMP,
            "crouch" => Buttons::CROUCH,
            "attack" => Buttons::ATTACK,
            "walk" => Buttons::WALK,
            other => return Err(format!("unknown button `{other}`")),
        });
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Recorder, WorldSpec};

    const FIXTURE: &str = "\
# a comment
rate 125
profile cpm
world flat 0
spawn 0 0 24
yaw 90

cmd 100 127 0 0 - 0 90
cmd 1 127 0 127 jump 0 90
cmd 50 127 64 0 - 0 92.5
";

    #[test]
    fn the_directives_parse_the_way_headless_reads_them() {
        let fixture = parse(FIXTURE).unwrap();
        assert_eq!(fixture.rate, TickRate::HZ_125);
        assert_eq!(fixture.profile_name, "cpm");
        assert_eq!(fixture.world, WorldChoice::Flat);
        assert_eq!(fixture.spawn, vec3(s(0.0), s(0.0), s(24.0)));
        assert_eq!(fixture.yaw, s(90.0));
        assert_eq!(fixture.cmds.len(), 151);
        assert_eq!(fixture.cmds[0].duration_ms, 8);
        assert_eq!(fixture.cmds[100].up_move, 127);
        assert!(fixture.cmds[100].buttons.contains(Buttons::JUMP));
        assert_eq!(fixture.cmds[150].right_move, 64);
        assert_eq!(
            fixture.cmds[150].view.yaw,
            straf3_sim::angle_to_short(s(92.5))
        );
    }

    #[test]
    fn a_file_with_no_rate_is_refused_rather_than_guessed() {
        assert!(parse("cmd 1 0 0 0 - 0 0").is_err());
        assert!(parse("rate 0\n").is_err());
        assert!(parse("rate 125\nworld moon\n").is_err());
        assert!(parse("rate 125\nprofile quake1\n").is_err());
        assert!(parse("rate 125\ncmd 1 0 0 0 nonsense 0 0").is_err());
    }

    #[test]
    fn a_flat_world_at_a_height_this_build_cannot_provide_is_refused() {
        // Silently ignoring the height would produce a run that claims to be
        // reproducible and is not.
        assert!(parse("rate 125\nworld flat 64\n").is_err());
        assert!(parse("rate 125\nworld flat 0\n").is_ok());
    }

    #[test]
    fn what_the_recorder_writes_is_what_the_parser_reads() {
        // The round trip that makes `--record` worth anything: every command
        // must come back bit-identical, view angles included.
        let mut recorder = Recorder::new(TickRate::HZ_125, vec3(s(0.0), s(0.0), s(24.0)), s(90.0));
        let mut cmd = UserCmd::still_at(TickRate::HZ_125);
        for i in 0..200 {
            cmd.forward_move = if i % 3 == 0 { 127 } else { 0 };
            cmd.right_move = if i % 5 == 0 { -127 } else { 127 };
            cmd.buttons = if i % 7 == 0 {
                Buttons::JUMP
            } else {
                Buttons::NONE
            };
            cmd.up_move = if i % 7 == 0 { 127 } else { 0 };
            cmd.view = ViewAngles::from_degrees(
                s(-1.0) / s((i + 1) as f32),
                s(90.0) + s(i as f32) / s(3.0),
                s(0.0),
            );
            recorder.push(cmd);
        }

        let text = recorder.to_fixture(WorldSpec::Flat(s(0.0)), "cpm");
        let fixture = parse(&text).unwrap();
        assert_eq!(fixture.cmds.len(), recorder.commands().len());
        for (parsed, original) in fixture.cmds.iter().zip(recorder.commands()) {
            assert_eq!(parsed, original);
        }
    }

    /// Contract item C3, end to end: a run recorded through the 16-bit view
    /// angle, written to a file and replayed from it reproduces the *same
    /// simulation*, not merely the same commands.
    ///
    /// Command equality (the test above) is the necessary half; this is the
    /// sufficient one. It compares the checksum at every tick rather than at
    /// the last, because a divergence can reconverge — a run has been measured
    /// matching on its final state while 29 of 1200 intermediate states
    /// differed, and an end-state comparison would have called that identical.
    ///
    /// The yaws are deliberately not representable angles: they are what an
    /// accumulating mouse would produce, quantised at the command boundary,
    /// and it is the quantised value the file has to carry back.
    #[test]
    fn a_recorded_run_replays_to_the_same_digest_at_every_tick() {
        let spawn = vec3(s(0.0), s(0.0), s(24.0));
        let spawn_yaw = s(90.0);
        let mut recorder = Recorder::new(TickRate::HZ_125, spawn, spawn_yaw);

        // A minute-and-a-half of strafejumping: turning continuously, jumping
        // on a rhythm, with a pitch that sweeps through level.
        let mut cmd = UserCmd::still_at(TickRate::HZ_125);
        for i in 0..1_000 {
            let t = s(i as f32);
            cmd.forward_move = 127;
            cmd.right_move = if (i / 40) % 2 == 0 { 127 } else { -127 };
            let jumping = i % 11 == 0;
            cmd.buttons = if jumping {
                Buttons::JUMP
            } else {
                Buttons::NONE
            };
            cmd.up_move = if jumping { 127 } else { 0 };
            cmd.view =
                ViewAngles::from_degrees(s(-20.0) + t * s(0.037), spawn_yaw + t * s(0.31), s(0.0));
            recorder.push(cmd);
        }

        let live = replay(
            &Fixture {
                rate: TickRate::HZ_125,
                profile: PhysicsProfile::cpm(),
                profile_name: "cpm".to_owned(),
                world: WorldChoice::Flat,
                spawn,
                yaw: spawn_yaw,
                cmds: recorder.commands().to_vec(),
            },
            &ReplayOptions::default(),
        )
        .unwrap();

        let text = recorder.to_fixture(WorldSpec::Flat(s(0.0)), "cpm");
        let from_file = replay(&parse(&text).unwrap(), &ReplayOptions::default()).unwrap();

        assert_eq!(live.len(), 1_001);
        assert_eq!(live.len(), from_file.len());
        for (tick, (a, b)) in live.iter().zip(&from_file).enumerate() {
            assert_eq!(a.checksum(), b.checksum(), "diverged at tick {tick}");
        }

        // And the run is a real one — a digest agreeing about nothing
        // happening would prove nothing. Past `max_speed` is the meaningful
        // threshold rather than an arbitrary number: ground movement cannot
        // exceed it, so the player is only there by strafejumping, which is
        // the code path where the view angle actually matters.
        let last = live.last().unwrap();
        let speed = (last.player.velocity.x * last.player.velocity.x
            + last.player.velocity.y * last.player.velocity.y)
            .sqrt();
        assert!(
            speed > PhysicsProfile::cpm().max_speed,
            "the reference run never left the ground speed cap: {speed}"
        );
        assert_ne!(live[0].checksum(), last.checksum());
    }

    #[test]
    fn a_map_recording_round_trips_to_the_map() {
        // The `world` directive is written for `Unrepresentable` worlds too
        // now (record.rs), and this parser understands `map` — so a recording
        // made on a compiled map, unlike a straf3-headless run, replays in the
        // world it was actually recorded in.
        let recorder = Recorder::new(TickRate::HZ_125, vec3(s(0.0), s(0.0), s(24.0)), s(90.0));
        let text = recorder.to_fixture(WorldSpec::Unrepresentable("map"), "cpm");
        let fixture = parse(&text).unwrap();
        assert_eq!(fixture.world, WorldChoice::Map);
    }

    #[test]
    fn an_arena_recording_is_refused_rather_than_replayed_elsewhere() {
        // The retirement's one externally visible consequence. A recording made
        // in the hardcoded arena must fail loudly: there is no geometry left to
        // reproduce it, and the failure mode this guards against is replaying it
        // against the map and calling the resulting trace a match.
        let recorder = Recorder::new(TickRate::HZ_125, vec3(s(0.0), s(0.0), s(24.0)), s(90.0));
        let text = recorder.to_fixture(WorldSpec::Unrepresentable("arena"), "cpm");
        let err = parse(&text).unwrap_err().to_string();
        assert!(
            err.contains("retired"),
            "expected the arena to be refused by name, got: {err}"
        );
    }

    #[test]
    fn a_hostile_frame_schedule_produces_the_identical_trace() {
        // Criterion 5, as an equality between two traces rather than a claim.
        let fixture = parse(FIXTURE).unwrap();

        let regular = replay(&fixture, &ReplayOptions::default()).unwrap();
        let hostile = replay(
            &fixture,
            &ReplayOptions {
                frame_ms: vec![1, 0, 200, 3, 37, 0, 0, 61, 8, 5],
                ..ReplayOptions::default()
            },
        )
        .unwrap();

        assert_eq!(regular.len(), fixture.cmds.len() + 1);
        assert_eq!(regular.len(), hostile.len());
        for (tick, (a, b)) in regular.iter().zip(&hostile).enumerate() {
            assert_eq!(a.checksum(), b.checksum(), "diverged at tick {tick}");
        }
    }

    #[test]
    fn a_schedule_that_can_never_advance_is_refused_rather_than_hanging() {
        let fixture = parse(FIXTURE).unwrap();
        let err = replay(
            &fixture,
            &ReplayOptions {
                frame_ms: vec![0, 0, 0],
                ..ReplayOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("0 ms"), "{err}");
    }

    #[test]
    fn the_csv_header_matches_the_columns_that_follow_it() {
        let fixture = parse(FIXTURE).unwrap();
        let trace = replay(&fixture, &ReplayOptions::default()).unwrap();
        let line = trace_line(&trace[1], true);
        assert_eq!(
            TRACE_HEADER.split(',').count(),
            line.split(',').count(),
            "{line}"
        );
        assert!(line.starts_with("1,8,"), "{line}");
    }

    #[test]
    fn replaying_the_same_fixture_twice_lands_on_the_same_bits() {
        let fixture = parse(FIXTURE).unwrap();
        let a = replay(&fixture, &ReplayOptions::default()).unwrap();
        let b = replay(&fixture, &ReplayOptions::default()).unwrap();
        assert_eq!(a.last().unwrap().checksum(), b.last().unwrap().checksum());
    }

    #[test]
    fn the_replay_path_is_the_step_function_and_nothing_else() {
        // Criterion 4 in one assertion: driving the commands through this
        // crate's loop must land exactly where calling `straf3_sim` directly
        // does, at every tick and not just the last.
        let fixture = parse(FIXTURE).unwrap();
        let ours = replay(&fixture, &ReplayOptions::default()).unwrap();

        let world = straf3_sim::world::FlatGround::at(s(0.0));
        let mut theirs = SimState::spawned_at(fixture.spawn, fixture.yaw);
        assert_eq!(ours[0].checksum(), theirs.checksum());
        for (i, cmd) in fixture.cmds.iter().enumerate() {
            straf3_sim::step_in_place(&mut theirs, cmd, &world, &fixture.profile);
            assert_eq!(ours[i + 1].checksum(), theirs.checksum(), "tick {}", i + 1);
        }
    }
}

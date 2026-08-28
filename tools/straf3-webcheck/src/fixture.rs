//! A fourth reader of `straf3-game`'s text recording format.
//!
//! There are three already — `straf3_game::replay`, `straf3-headless`, and the
//! `--frame-ms` path through the windowed build — and the point of each is the
//! same: a format read by one implementation cannot demonstrate that two
//! implementations agree.
//!
//! This one exists for a narrower reason. Before a browser-recorded `.s3d`
//! exists there is nothing to point [`crate`]'s comparison at, and a harness
//! first exercised on the artefact it was built to judge has never been shown
//! to work. `probes/coil-course/results/coil-run.txt` is a committed run of
//! `assets/maps/coil.map` — a real command stream, on the shipped collider,
//! whose replay checksum `PLAYING.md` publishes. Converting it to a `.s3d`
//! gives the harness a native subject with a known answer.
//!
//! It reads the subset the fixture uses and refuses everything else by name,
//! the way `straf3-headless` does. `world map` is accepted here and refused
//! there, because this reader is handed the `.map` file and that one is below
//! the seam and has no way to spell a compiled map.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::{Buttons, PhysicsProfile, TickRate, UserCmd, ViewAngles};

/// Which world a fixture declares.
#[derive(Debug, PartialEq)]
pub enum World {
    /// `world map` — the caller supplies the `.map`.
    Map,
    /// `world flat <z>`.
    Flat(Scalar),
    /// `world empty`.
    Empty,
}

/// A parsed text recording.
#[derive(Debug)]
pub struct Fixture {
    pub rate: TickRate,
    pub profile: PhysicsProfile,
    pub profile_name: String,
    pub world: World,
    pub spawn: Vec3,
    pub yaw: Scalar,
    pub commands: Vec<UserCmd>,
}

/// Parse the text format.
///
/// # Errors
///
/// Every malformed or unrecognised line, named with its line number. There is
/// no lenient mode and no default for a missing `rate`: the command rate is
/// part of the physics, so guessing it would produce a different run.
pub fn parse(text: &str) -> Result<Fixture, String> {
    let mut rate: Option<TickRate> = None;
    let mut profile = PhysicsProfile::cpm();
    let mut profile_name = "cpm".to_string();
    let mut world = World::Flat(s(0.0));
    let mut spawn = vec3(s(0.0), s(0.0), s(64.0));
    let mut yaw = s(0.0);
    let mut commands = Vec::new();

    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = |e: String| format!("line {}: {e}", n + 1);
        let f: Vec<&str> = line.split_whitespace().collect();

        match f[0] {
            "rate" => {
                let hz: u32 = num(&f, 1).map_err(&at)?;
                rate = Some(
                    TickRate::from_hz(hz)
                        .ok_or_else(|| at(format!("{hz} Hz is not expressible in whole ms")))?,
                );
            }
            "profile" => {
                profile_name = f.get(1).unwrap_or(&"").to_string();
                profile = profile_named(&profile_name).ok_or_else(|| {
                    at(format!("unknown profile `{profile_name}` (cpm|vq3)"))
                })?;
            }
            "world" => {
                world = match f.get(1) {
                    Some(&"map") => World::Map,
                    Some(&"empty") => World::Empty,
                    Some(&"flat") => World::Flat(num(&f, 2).unwrap_or(s(0.0))),
                    other => {
                        return Err(at(format!(
                            "unknown world `{}` (map|empty|flat <z>)",
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
                    at("`rate` must be declared before the first `cmd`".to_string())
                })?;
                let repeat: u32 = num(&f, 1).map_err(&at)?;
                let cmd = UserCmd {
                    duration_ms: rate.command_millis(),
                    forward_move: axis(&f, 2).map_err(&at)?,
                    right_move: axis(&f, 3).map_err(&at)?,
                    up_move: axis(&f, 4).map_err(&at)?,
                    buttons: buttons(f.get(5).copied().unwrap_or("-")).map_err(&at)?,
                    // Degrees in the file, Q3's 16-bit angles in the command
                    // (contract item C3). Quantising here is what makes the
                    // command in this file the command the simulation ran.
                    view: ViewAngles::from_degrees(
                        num(&f, 6).unwrap_or(s(0.0)),
                        num(&f, 7).unwrap_or(yaw),
                        num(&f, 8).unwrap_or(s(0.0)),
                    ),
                };
                for _ in 0..repeat {
                    commands.push(cmd);
                }
            }
            other => return Err(at(format!("unknown directive `{other}`"))),
        }
    }

    Ok(Fixture {
        rate: rate.ok_or("no `rate` declared: the command rate is part of the physics")?,
        profile,
        profile_name,
        world,
        spawn,
        yaw,
        commands,
    })
}

/// The two named profiles a recording can be made under.
///
/// Not a `From` impl and not exhaustive by accident: a name this does not know
/// is an error at the call site rather than a fallback to `cpm`, because
/// falling back would re-simulate a run under physics it was not recorded
/// under and report the disagreement as a divergence.
#[must_use]
pub fn profile_named(name: &str) -> Option<PhysicsProfile> {
    match name {
        "cpm" => Some(PhysicsProfile::cpm()),
        "vq3" => Some(PhysicsProfile::vq3()),
        _ => None,
    }
}

fn num<T: std::str::FromStr>(f: &[&str], i: usize) -> Result<T, String> {
    f.get(i)
        .ok_or_else(|| format!("missing field {i}"))?
        .parse()
        .map_err(|_| format!("field {i} (`{}`) is not a number", f[i]))
}

fn axis(f: &[&str], i: usize) -> Result<i8, String> {
    let v: i32 = num(f, i)?;
    Ok(v.clamp(-127, 127) as i8)
}

fn buttons(spec: &str) -> Result<Buttons, String> {
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

    #[test]
    fn a_repeat_count_expands_to_that_many_commands() {
        let f = parse("rate 125\ncmd 3 127 0 0 - 0.0 0.0 0.0\n").unwrap();
        assert_eq!(f.commands.len(), 3);
        assert_eq!(f.commands[0], f.commands[2]);
    }

    #[test]
    fn an_unknown_profile_is_refused_rather_than_defaulted() {
        // The failure this prevents: re-simulating under `cpm` a run that was
        // recorded under something else, and reporting the difference as a
        // divergence in the browser.
        let err = parse("rate 125\nprofile straf3\n").unwrap_err();
        assert!(err.contains("unknown profile") && err.contains("straf3"), "{err}");
    }

    #[test]
    fn a_command_before_the_rate_is_refused() {
        let err = parse("cmd 1 0 0 0 - 0 0 0\n").unwrap_err();
        assert!(err.contains("rate"), "{err}");
    }

    #[test]
    fn world_map_is_accepted_here_and_carries_no_geometry_of_its_own() {
        assert_eq!(parse("rate 125\nworld map\n").unwrap().world, World::Map);
    }
}

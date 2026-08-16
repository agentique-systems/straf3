//! `straf3-headless` — run a recorded input file with no GPU and no window.
//!
//! Spec criterion 7. This is the proof that the simulation is what it claims
//! to be: it reads a file of commands, runs them, and prints the final state.
//! No window opens, no adapter is enumerated, no clock is consulted. It runs
//! over SSH, in CI, and under WSL2 where — per spec section 2 — there is no
//! usable GPU and frame pacing is fiction.
//!
//! # Why this file is at `bin/` and not `src/bin/`
//!
//! Because it reads a file, and the library must never do that.
//! `cargo xtask check-seam` scans `crates/straf3-sim/src/**` for `std::fs`,
//! `std::env` and friends. Putting this under `src/bin/` would have forced
//! either a carve-out in that scan or a weaker scan — and the value of the
//! check is that it has no exceptions. Keeping the runner at `bin/` (declared
//! explicitly in `Cargo.toml`) means everything under `src/` really is bound
//! by the rule, and this file is visibly a *consumer* of the library rather
//! than part of it.
//!
//! Every I/O call in the crate is in this file. The library it calls is pure.
//!
//! Longer term this belongs in a `straf3-cli` or in `straf3-replay`, next to
//! the real demo format. It is here because Wave 1 owns only this crate.
//!
//! # Usage
//!
//! ```text
//! straf3-headless <input-file> [--trace] [--csv]
//! straf3-headless -                  # read the input from stdin
//! ```
//!
//! # Input format
//!
//! A deliberately boring line-oriented text format. It is a *test fixture
//! format*, not the replay format — `straf3-replay` owns that. Blank lines
//! and `#` comments are ignored.
//!
//! ```text
//! rate 125                 # command rate in Hz (spec D2). Required.
//! profile cpm              # cpm | vq3. Default cpm (spec D1).
//! world flat 0             # `flat <z>` or `empty`. Default `flat 0`.
//! spawn 0 0 64             # spawn origin. Default 0 0 64.
//! yaw 90                   # spawn view yaw. Default 0.
//!
//! # cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> [roll]
//! # buttons: `-` for none, or a +-joined list of jump/crouch/attack/walk.
//! cmd 100  127 0 0  -      0 90
//! cmd 1    127 0 0  jump   0 90
//! cmd 200  127 64 0 -      0 90
//! ```
//!
//! Command duration is not written per line: it comes from `rate`, because
//! the rate is the recorded simulation parameter D2 requires and a per-line
//! duration would let a fixture drift away from a rate it claims to run at.

use std::process::ExitCode;

use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::{EmptyWorld, FlatGround, World};
use straf3_sim::{Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, step_in_place};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "usage: straf3-headless <input-file|-> [--trace] [--csv]\n\n  \
             Runs a recorded input file through the simulation with no GPU and\n  \
             no window, and prints the final state (spec criterion 7).\n\n  \
             --trace   print every tick, not just the final state\n  \
             --csv     print the trace as CSV"
        );
        return ExitCode::FAILURE;
    }

    let path = &args[0];
    let trace = args.iter().any(|a| a == "--trace");
    let csv = args.iter().any(|a| a == "--csv");

    let text = match read_input(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("straf3-headless: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let input = match parse(&text) {
        Ok(input) => input,
        Err(e) => {
            eprintln!("straf3-headless: {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The only interesting line in this file: the world is chosen here, and
    // the simulation is handed it as an argument. It has no idea which one it
    // got, which is the point of the `World` trait.
    match &input.world {
        WorldChoice::Empty => execute(&input, &EmptyWorld, trace, csv),
        WorldChoice::Flat(height) => execute(&input, &FlatGround::at(*height), trace, csv),
    }

    ExitCode::SUCCESS
}

/// Read from a file, or from stdin when the path is `-`.
fn read_input(path: &str) -> std::io::Result<String> {
    if path == "-" {
        std::io::read_to_string(std::io::stdin())
    } else {
        std::fs::read_to_string(path)
    }
}

fn execute<W: World>(input: &Input, world: &W, trace: bool, csv: bool) {
    let mut state = SimState::spawned_at(input.spawn, input.yaw);

    if trace && csv {
        println!("tick,time_ms,x,y,z,vx,vy,vz,speed,grounded,checksum");
    }
    if trace {
        emit(&state, csv);
    }
    for cmd in &input.cmds {
        step_in_place(&mut state, cmd, world, &input.profile);
        if trace {
            emit(&state, csv);
        }
    }

    if csv && !trace {
        println!("tick,time_ms,x,y,z,vx,vy,vz,speed,grounded,checksum");
        emit(&state, true);
        return;
    }
    if csv {
        return;
    }

    let horizontal = (state.player.velocity.x * state.player.velocity.x
        + state.player.velocity.y * state.player.velocity.y)
        .sqrt();
    println!("straf3 headless run");
    println!("  commands      {}", input.cmds.len());
    println!(
        "  rate          {} Hz ({} ms per command)",
        input.rate.hz(),
        input.rate.command_millis()
    );
    println!("  profile       {}", input.profile_name);
    println!("  world         {}", input.world);
    println!("final state");
    println!("  tick          {}", state.tick);
    println!("  time          {} ms", state.time_ms);
    println!(
        "  origin        {:.6} {:.6} {:.6}",
        state.player.origin.x, state.player.origin.y, state.player.origin.z
    );
    println!(
        "  velocity      {:.6} {:.6} {:.6}",
        state.player.velocity.x, state.player.velocity.y, state.player.velocity.z
    );
    println!("  speed (ups)   {horizontal:.6}");
    println!(
        "  ground        {}",
        match state.player.ground.normal() {
            Some(n) => format!("grounded, normal {:.4} {:.4} {:.4}", n.x, n.y, n.z),
            None => "airborne".to_string(),
        }
    );
    println!("  run           {:?}", state.run);
    // The line a determinism check compares. Printed last so it is easy to
    // pull out of the output with `tail -1`.
    println!("  checksum      {:#018x}", state.checksum());
}

fn emit(state: &SimState, csv: bool) {
    let v = state.player.velocity;
    let speed = (v.x * v.x + v.y * v.y).sqrt();
    if csv {
        println!(
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
        );
    } else {
        println!(
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
        );
    }
}

/// Which test world to run against.
enum WorldChoice {
    /// No geometry at all.
    Empty,
    /// An infinite plane at this height.
    Flat(Scalar),
}

impl std::fmt::Display for WorldChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty"),
            Self::Flat(h) => write!(f, "flat ground at z={h}"),
        }
    }
}

/// A parsed input file.
struct Input {
    rate: TickRate,
    profile: PhysicsProfile,
    profile_name: String,
    world: WorldChoice,
    spawn: straf3_sim::num::Vec3,
    yaw: Scalar,
    cmds: Vec<UserCmd>,
}

fn parse(text: &str) -> Result<Input, String> {
    let mut rate: Option<TickRate> = None;
    let mut profile = PhysicsProfile::cpm();
    let mut profile_name = "cpm".to_string();
    let mut world = WorldChoice::Flat(s(0.0));
    let mut spawn = vec3(s(0.0), s(0.0), s(64.0));
    let mut yaw = s(0.0);
    let mut cmds = Vec::new();

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = |e: String| format!("line {}: {e}", lineno + 1);
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
                profile = match profile_name.as_str() {
                    "cpm" => PhysicsProfile::cpm(),
                    "vq3" => PhysicsProfile::vq3(),
                    other => return Err(at(format!("unknown profile `{other}` (cpm|vq3)"))),
                };
            }
            "world" => {
                world = match f.get(1) {
                    Some(&"empty") => WorldChoice::Empty,
                    Some(&"flat") => WorldChoice::Flat(num(&f, 2).unwrap_or(s(0.0))),
                    other => {
                        return Err(at(format!(
                            "unknown world `{}` (empty|flat <z>)",
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
                            .to_string(),
                    )
                })?;
                let repeat: u32 = num(&f, 1).map_err(&at)?;
                let cmd = UserCmd {
                    duration_ms: rate.command_millis(),
                    forward_move: axis(&f, 2).map_err(&at)?,
                    right_move: axis(&f, 3).map_err(&at)?,
                    up_move: axis(&f, 4).map_err(&at)?,
                    buttons: buttons(f.get(5).copied().unwrap_or("-")).map_err(&at)?,
                    view: ViewAngles {
                        pitch: num(&f, 6).unwrap_or(s(0.0)),
                        yaw: num(&f, 7).unwrap_or(yaw),
                        roll: num(&f, 8).unwrap_or(s(0.0)),
                    },
                };
                for _ in 0..repeat {
                    cmds.push(cmd);
                }
            }
            other => return Err(at(format!("unknown directive `{other}`"))),
        }
    }

    Ok(Input {
        rate: rate.ok_or("no `rate` declared: the command rate is a required, recorded simulation parameter (spec D2)")?,
        profile,
        profile_name,
        world,
        spawn,
        yaw,
        cmds,
    })
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

    /// `arena` falls through the catch-all `other =>` arm of the `world`
    /// directive, and that fallthrough is the only thing stopping this
    /// runner from silently defaulting to `WorldChoice::Flat` and running an
    /// arena recording in flat space. Nothing else pins that refusal — pin
    /// it here, and demand the message names the world, so this fails loudly
    /// if a future change routes `arena` to a different error (or to no
    /// error at all) rather than passing on an unrelated failure.
    #[test]
    fn arena_world_is_refused_by_name() {
        let err = match parse("rate 125\nworld arena\n") {
            Ok(_) => panic!("`arena` is not a known world; parse should have refused it"),
            Err(e) => e,
        };
        assert!(
            err.contains("unknown world") && err.contains("arena"),
            "expected an unknown-world error naming `arena`, got: {err}"
        );
    }
}

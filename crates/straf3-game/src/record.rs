//! Recording the commands a session produced, in `straf3-headless`'s own
//! input format.
//!
//! # What this is for
//!
//! Criterion 4: *a recorded input sequence replays to the same checksum
//! through the windowed build as through `straf3-headless`*. The cheapest
//! honest way to demonstrate that is to have the windowed build write a file
//! the headless runner already knows how to read, and compare the two
//! checksums — two binaries, two processes, one number.
//!
//! This module only *writes* that format; `straf3-sim`'s `bin/headless.rs`
//! owns the parser and stays the single definition of it.
//!
//! # The one thing it cannot do
//!
//! `straf3-headless` knows two worlds: `empty` and `flat <z>`. It does not
//! know the arena, and it must not be taught to — the arena lives above the
//! line, in `straf3-render`, and the headless runner is below it. So a session
//! recorded *in the arena* is replayable in-process (through
//! [`crate::Game::apply`], which is the same `step_in_place` call) but not
//! through `straf3-headless`. [`WorldSpec::Unrepresentable`] makes that
//! visible in the file itself rather than letting a fixture silently claim a
//! world it did not run in.

use straf3_sim::num::{Scalar, Vec3};
use straf3_sim::{Buttons, TickRate, UserCmd};

/// Which world a recording ran in, as `straf3-headless` spells it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorldSpec {
    /// No geometry at all.
    Empty,
    /// An infinite plane at this height.
    Flat(Scalar),
    /// A world `straf3-headless` has no spelling for — the arena.
    ///
    /// The name is written into the file as a comment, and no `world`
    /// directive is emitted, so feeding the file to `straf3-headless` produces
    /// a run in *its* default world rather than a silent claim to have
    /// reproduced this one.
    Unrepresentable(&'static str),
}

impl core::fmt::Display for WorldSpec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty"),
            Self::Flat(z) => write!(f, "flat {z:?}"),
            Self::Unrepresentable(name) => write!(f, "{name}"),
        }
    }
}

/// Commands as they were produced, in order.
#[derive(Debug, Clone)]
pub struct Recorder {
    rate: TickRate,
    spawn: Vec3,
    spawn_yaw: Scalar,
    commands: Vec<UserCmd>,
}

impl Recorder {
    /// Start recording a session at `rate`, spawned at `spawn` looking along
    /// `spawn_yaw`.
    #[must_use]
    pub const fn new(rate: TickRate, spawn: Vec3, spawn_yaw: Scalar) -> Self {
        Self {
            rate,
            spawn,
            spawn_yaw,
            commands: Vec::new(),
        }
    }

    /// Append one command.
    pub fn push(&mut self, cmd: UserCmd) {
        self.commands.push(cmd);
    }

    /// Every command recorded, in order.
    #[must_use]
    pub fn commands(&self) -> &[UserCmd] {
        &self.commands
    }

    /// How many commands are recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The recording as a `straf3-headless` input file.
    ///
    /// Identical consecutive commands are folded into one line with a repeat
    /// count — a minute of play is 7 500 commands, and standing still is one
    /// line either way.
    ///
    /// Angles are printed with `{:?}`, which is Rust's shortest
    /// round-trippable form for `f32`: the parsed value is *bit-identical* to
    /// the one recorded, which is the only reason the two checksums can be
    /// compared at all.
    #[must_use]
    pub fn to_fixture(&self, world: WorldSpec, profile_name: &str) -> String {
        let mut out = String::with_capacity(64 * self.commands.len() + 512);

        out.push_str("# Recorded by the straf3 windowed build.\n");
        out.push_str(
            "# Feed it to `straf3-headless` and compare the final checksum: the\n\
             # windowed build and the headless runner call the same step function\n\
             # with the same commands, so the two numbers must be equal.\n",
        );
        out.push_str(&format!("# world {world}\n"));
        if let WorldSpec::Unrepresentable(name) = world {
            out.push_str(&format!(
                "# NOTE: `straf3-headless` has no spelling for the `{name}` world, so no\n\
                 # `world` directive is written here and this file will NOT reproduce\n\
                 # the recorded run under it. Replay it in-process instead.\n"
            ));
        }
        out.push('\n');

        out.push_str(&format!("rate {}\n", self.rate.hz()));
        out.push_str(&format!("profile {profile_name}\n"));
        match world {
            WorldSpec::Empty | WorldSpec::Flat(_) => {
                out.push_str(&format!("world {world}\n"));
            }
            WorldSpec::Unrepresentable(_) => {}
        }
        out.push_str(&format!(
            "spawn {:?} {:?} {:?}\n",
            self.spawn.x, self.spawn.y, self.spawn.z
        ));
        out.push_str(&format!("yaw {:?}\n\n", self.spawn_yaw));
        out.push_str("# cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> <roll>\n");

        let mut index = 0;
        while index < self.commands.len() {
            let cmd = self.commands[index];
            let mut repeat = 1;
            while index + repeat < self.commands.len() && self.commands[index + repeat] == cmd {
                repeat += 1;
            }
            out.push_str(&format!(
                "cmd {} {} {} {} {} {:?} {:?} {:?}\n",
                repeat,
                cmd.forward_move,
                cmd.right_move,
                cmd.up_move,
                button_spec(cmd.buttons),
                cmd.view.pitch,
                cmd.view.yaw,
                cmd.view.roll,
            ));
            index += repeat;
        }

        out
    }
}

/// Buttons as `straf3-headless` spells them: `-`, or `+`-joined names.
fn button_spec(buttons: Buttons) -> String {
    let mut names = Vec::new();
    for (bit, name) in [
        (Buttons::JUMP, "jump"),
        (Buttons::CROUCH, "crouch"),
        (Buttons::ATTACK, "attack"),
        (Buttons::WALK, "walk"),
    ] {
        if buttons.contains(bit) {
            names.push(name);
        }
    }
    if names.is_empty() {
        "-".to_owned()
    } else {
        names.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::ViewAngles;
    use straf3_sim::num::{s, vec3};

    fn recorder() -> Recorder {
        Recorder::new(TickRate::HZ_125, vec3(s(0.0), s(0.0), s(24.0)), s(90.0))
    }

    #[test]
    fn an_empty_recording_is_still_a_valid_fixture() {
        let text = recorder().to_fixture(WorldSpec::Flat(s(0.0)), "cpm");
        assert!(text.contains("rate 125\n"));
        assert!(text.contains("world flat 0.0\n"));
        assert!(text.contains("profile cpm\n"));
        assert!(!text.contains("\ncmd "));
    }

    #[test]
    fn identical_commands_fold_into_a_repeat_count() {
        let mut rec = recorder();
        for _ in 0..100 {
            rec.push(UserCmd::still_at(TickRate::HZ_125));
        }
        let text = rec.to_fixture(WorldSpec::Flat(s(0.0)), "cpm");
        assert!(text.contains("cmd 100 0 0 0 - 0.0 0.0 0.0\n"), "{text}");
    }

    #[test]
    fn a_change_breaks_the_run_rather_than_being_swallowed() {
        let mut rec = recorder();
        let mut cmd = UserCmd::still_at(TickRate::HZ_125);
        cmd.forward_move = 127;
        rec.push(cmd);
        rec.push(cmd);
        cmd.buttons = Buttons::JUMP;
        cmd.up_move = 127;
        rec.push(cmd);
        rec.push(cmd);

        let text = rec.to_fixture(WorldSpec::Flat(s(0.0)), "cpm");
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("cmd ")).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "cmd 2 127 0 0 - 0.0 0.0 0.0");
        assert_eq!(lines[1], "cmd 2 127 0 127 jump 0.0 0.0 0.0");
    }

    #[test]
    fn angles_round_trip_bit_for_bit() {
        // The recording is only worth anything if the parsed angle is the same
        // bits as the recorded one — a truncated decimal would replay to a
        // different checksum and the whole exercise would prove nothing.
        let mut rec = recorder();
        let awkward = [
            s(1.0 / 3.0),
            s(-89.999_99),
            s(179.999_98),
            s(0.1) + s(0.2),
            s(1.234_567_9e-7),
        ];
        for yaw in awkward {
            rec.push(UserCmd {
                view: ViewAngles {
                    pitch: s(0.0),
                    yaw,
                    roll: s(0.0),
                },
                ..UserCmd::still_at(TickRate::HZ_125)
            });
        }
        let text = rec.to_fixture(WorldSpec::Flat(s(0.0)), "cpm");
        for (line, expected) in text
            .lines()
            .filter(|l| l.starts_with("cmd "))
            .zip(awkward.iter())
        {
            let field: f32 = line.split_whitespace().nth(7).unwrap().parse().unwrap();
            assert_eq!(field.to_bits(), expected.to_bits(), "{line}");
        }
    }

    #[test]
    fn a_world_headless_cannot_spell_is_flagged_rather_than_faked() {
        let text = recorder().to_fixture(WorldSpec::Unrepresentable("arena"), "cpm");
        assert!(text.contains("# world arena"));
        assert!(text.contains("will NOT reproduce"));
        // The crucial bit: no `world` directive, so headless cannot be fooled
        // into thinking it ran the same geometry.
        assert!(!text.lines().any(|l| l.starts_with("world ")));
    }

    #[test]
    fn every_button_has_a_spelling_headless_accepts() {
        assert_eq!(button_spec(Buttons::NONE), "-");
        assert_eq!(button_spec(Buttons::JUMP), "jump");
        assert_eq!(
            button_spec(Buttons::JUMP.with(Buttons::CROUCH).with(Buttons::WALK)),
            "jump+crouch+walk"
        );
    }
}

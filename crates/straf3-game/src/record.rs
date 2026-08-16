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
//! know a compiled map, and it must not be taught to — the map is *chosen*
//! above the line, in `straf3-game`'s `scene`, and the headless runner is below
//! it with no way to be told which `.map` to load. So a session recorded *on a
//! map* is replayable in-process, through `straf3 --replay` (which parses
//! `world map`), but not through
//! `straf3-headless`. [`WorldSpec::Unrepresentable`] writes the world's name
//! into the file regardless — omitting it would make `straf3-headless` (and
//! any other reader defaulting on a missing directive) silently run the
//! wrong world instead of refusing a name it does not know.

use straf3_sim::num::{Scalar, Vec3};
use straf3_sim::{Buttons, TickRate, UserCmd};

/// Which world a recording ran in, as `straf3-headless` spells it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorldSpec {
    /// No geometry at all.
    Empty,
    /// An infinite plane at this height.
    Flat(Scalar),
    /// A world `straf3-headless` has no spelling for — a compiled map.
    ///
    /// The name is written into the file as the `world` directive, same as
    /// any other world. `straf3-headless` does not recognise it and refuses
    /// the file by name rather than defaulting to a different world it never
    /// ran — that refusal is `straf3-headless`'s, not a fact this recorder
    /// can enforce by omission.
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

    /// The state this recording began from, as the `.s3d` format states it.
    ///
    /// The same three values [`Recorder::new`] was given. A `.s3d` carries the
    /// spawn itself rather than looking it up from the map, because moving a
    /// spawn marker deliberately does not change a map's collision identity —
    /// so the recording has to say where the run actually began.
    #[must_use]
    pub const fn start(&self) -> straf3_replay::RunStart {
        straf3_replay::RunStart {
            rate: self.rate,
            spawn: self.spawn,
            yaw: self.spawn_yaw,
        }
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
    /// # Angles
    ///
    /// View angles are 16-bit in the command (contract item C3) and are
    /// written here as the degrees they stand for, with `{:?}` — Rust's
    /// shortest round-trippable form for `f32`. The file therefore stays
    /// human-readable and diffable, and the round trip is still exact: the
    /// printed value is one of the 65 536 representable angles, it parses back
    /// to the identical `f32`, and re-quantising it recovers the identical
    /// short (`straf3_sim::cmd`'s exhaustive round-trip test is the guarantee).
    ///
    /// The alternative — printing the raw shorts — would be smaller but would
    /// silently change the meaning of every `cmd` line already written: an old
    /// `90` means 90°, and a reader expecting shorts would take it for 0.49°
    /// and run a different session while looking like it had run the recorded
    /// one. Compactness belongs in the binary replay format, not here.
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
                "# NOTE: `straf3-headless` has no spelling for the `{name}` world. It will\n\
                 # refuse this file by name rather than silently run it in a different\n\
                 # one. Replay it with `straf3 --replay` instead.\n"
            ));
        }
        out.push('\n');

        out.push_str(&format!("rate {}\n", self.rate.hz()));
        out.push_str(&format!("profile {profile_name}\n"));
        out.push_str(&format!("world {world}\n"));
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
                cmd.view.pitch_degrees(),
                cmd.view.yaw_degrees(),
                cmd.view.roll_degrees(),
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
        //
        // Since C3 the recorded angle is a 16-bit short, so the property has
        // two halves and both are checked here: the printed degrees parse back
        // to the identical `f32`, and that `f32` re-quantises to the identical
        // short. Awkward *inputs* are used on purpose — they are quantised on
        // the way into the command, and it is the quantised value, not the
        // input, that has to survive the file.
        let mut rec = recorder();
        let awkward = [
            s(1.0 / 3.0),
            s(-89.999_99),
            s(179.999_98),
            s(0.1) + s(0.2),
            s(1.234_567_9e-7),
            s(359.999_97),
        ];
        for yaw in awkward {
            rec.push(UserCmd {
                view: ViewAngles::looking_along(yaw),
                ..UserCmd::still_at(TickRate::HZ_125)
            });
        }
        let text = rec.to_fixture(WorldSpec::Flat(s(0.0)), "cpm");
        let recorded: Vec<UserCmd> = rec.commands().to_vec();
        for (line, original) in text
            .lines()
            .filter(|l| l.starts_with("cmd "))
            .zip(recorded.iter())
        {
            let field: f32 = line.split_whitespace().nth(7).unwrap().parse().unwrap();
            assert_eq!(
                field.to_bits(),
                original.view.yaw_degrees().to_bits(),
                "printed degrees are not the recorded ones: {line}"
            );
            assert_eq!(
                straf3_sim::angle_to_short(field),
                original.view.yaw,
                "re-quantising the printed degrees lost the short: {line}"
            );
        }
    }

    #[test]
    fn a_world_headless_cannot_spell_is_written_by_name_not_omitted() {
        // Superseded design: this directive used to be omitted so that
        // `straf3-headless` would silently fall back to its default `Flat`
        // world instead of erroring — which just made the wrong answer
        // silent in every reader, including `straf3 --replay`, which
        // already understands `map` and was needlessly blinded by the
        // omission. Now the name is written like any other world, and it is
        // `straf3-headless`'s own unknown-world error that refuses it.
        let text = recorder().to_fixture(WorldSpec::Unrepresentable("map"), "cpm");
        assert!(text.contains("# world map"));
        assert!(text.contains("refuse this file by name"));
        // The crucial bit: the `world` directive IS written, so a reader
        // that understands `map` (`straf3 --replay`) can use it, and a
        // reader that does not (`straf3-headless`) sees the real name and
        // can refuse it rather than silently defaulting.
        assert!(text.lines().any(|l| l == "world map"));
    }

    #[test]
    fn a_map_recordings_note_says_headless_refuses_it() {
        let text = recorder().to_fixture(WorldSpec::Unrepresentable("map"), "cpm");
        assert!(text.contains("straf3-headless"));
        assert!(text.contains("refuse"));
        assert!(text.contains("straf3 --replay"));
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

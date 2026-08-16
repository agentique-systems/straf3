//! The format's own tests: round trip, refusal, and every way a file can lie.

use straf3_sim::num::{Vec3, s, vec3};
use straf3_sim::world::{EmptyWorld, FlatGround};
use straf3_sim::{
    Buttons, PhysicsProfile, TickRate, UserCmd, ViewAngles, angle_to_short, step_in_place,
};

use crate::codec::{FLAG_TRACE, FORMAT_VERSION, MAGIC, MAX_NAME_BYTES};
use crate::digest::fold_all;
use crate::{LoadError, Mismatch, Outcome, PhysicsId, Recording, RunStart, VerifyError, WorldId};

fn start() -> RunStart {
    RunStart {
        rate: TickRate::HZ_125,
        spawn: vec3(s(0.0), s(0.0), s(64.0)),
        yaw: s(90.0),
    }
}

/// A short strafing run: jumps, turns, crouches, and uses every command field.
fn commands(n: u32) -> Vec<UserCmd> {
    let world = FlatGround::at(s(0.0));
    let profile = PhysicsProfile::cpm();
    let mut state = start().state();
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut cmd = UserCmd::still_at(TickRate::HZ_125);
        cmd.view.yaw = angle_to_short(s(i as f32) * s(0.37));
        cmd.view.pitch = angle_to_short(s(-20.0) + s((i % 60) as f32));
        cmd.view.roll = angle_to_short(s(i as f32) * s(-0.11));
        cmd.forward_move = 127;
        cmd.right_move = if (i / 8).is_multiple_of(2) { 127 } else { -127 };
        if state.player.ground.is_grounded() {
            cmd.buttons = Buttons::JUMP;
        }
        if (30..40).contains(&i) {
            cmd.buttons = cmd.buttons.with(Buttons::CROUCH).with(Buttons::WALK);
            cmd.up_move = -127;
        }
        step_in_place(&mut state, &cmd, &world, &profile);
        out.push(cmd);
    }
    out
}

fn map_id() -> WorldId {
    WorldId::map("coil", 0x0123_4567_89ab_cdef)
}

fn recorded(n: u32) -> Recording {
    Recording::record(
        start(),
        commands(n),
        &FlatGround::at(s(0.0)),
        map_id(),
        &PhysicsProfile::cpm(),
        "cpm",
    )
}

// ── round trip ──────────────────────────────────────────────────────────────

#[test]
fn a_recording_survives_the_bytes_exactly() {
    let rec = recorded(120);
    for bytes in [rec.to_bytes(), rec.to_bytes_with_checksums().unwrap()] {
        let back = Recording::from_bytes(&bytes).expect("decodes");
        assert_eq!(back.start(), rec.start());
        assert_eq!(back.world(), rec.world());
        assert_eq!(back.physics(), rec.physics());
        assert_eq!(back.claimed(), rec.claimed());
        assert_eq!(back.commands_unchecked(), rec.commands_unchecked());
    }
}

#[test]
fn encoding_is_the_inverse_of_decoding_both_ways_round() {
    let rec = recorded(64);
    let compact = rec.to_bytes();
    let traced = rec.to_bytes_with_checksums().unwrap();
    assert_eq!(Recording::from_bytes(&compact).unwrap().to_bytes(), compact);
    assert_eq!(
        Recording::from_bytes(&traced)
            .unwrap()
            .to_bytes_with_checksums()
            .unwrap(),
        traced
    );
    // A recording loaded from either form writes the same compact bytes.
    assert_eq!(Recording::from_bytes(&traced).unwrap().to_bytes(), compact);
    // ...and the traced form is exactly eight bytes per command longer.
    assert_eq!(traced.len(), compact.len() + 8 * rec.command_count());
}

#[test]
fn a_recording_loaded_from_a_compact_file_cannot_pretend_to_have_evidence() {
    let rec = recorded(16);
    let back = Recording::from_bytes(&rec.to_bytes()).unwrap();
    assert!(back.trace().is_none());
    assert!(back.to_bytes_with_checksums().is_none());
}

#[test]
fn every_command_field_survives_including_the_awkward_ones() {
    // The fields most likely to be lost by a lazy encoder: the sign of the
    // signed axes, the high button bits, and the full 16-bit range of each
    // angle. `i8::MIN` is included because `-128` is not `-127` and a
    // negate-and-cast would turn it into itself.
    let awkward = vec![
        UserCmd {
            duration_ms: u16::MAX,
            forward_move: i8::MIN,
            right_move: i8::MAX,
            up_move: -1,
            buttons: Buttons(u16::MAX),
            view: ViewAngles {
                pitch: u16::MAX,
                yaw: 0,
                roll: 32_768,
            },
        },
        UserCmd {
            duration_ms: 0,
            forward_move: 0,
            right_move: i8::MIN,
            up_move: i8::MAX,
            buttons: Buttons::NONE,
            view: ViewAngles {
                pitch: 1,
                yaw: 65_535,
                roll: 1,
            },
        },
    ];
    let rec = Recording::record(
        start(),
        awkward.clone(),
        &EmptyWorld,
        WorldId::Empty,
        &PhysicsProfile::vq3(),
        "vq3",
    );
    let back = Recording::from_bytes(&rec.to_bytes()).unwrap();
    assert_eq!(back.commands_unchecked(), awkward.as_slice());
}

#[test]
fn the_spawn_survives_bit_for_bit() {
    // Written as bits, so a negative zero and a value no decimal round-trips
    // both come back identical. Text would need a proof; bits need none.
    let odd = RunStart {
        rate: TickRate::HZ_76,
        spawn: vec3(s(-0.0), s(1.0 / 3.0), s(-12345.678)),
        yaw: s(-89.999_99),
    };
    let rec = Recording::record(
        odd,
        commands(4),
        &FlatGround::at(s(0.0)),
        WorldId::flat(s(-0.0)),
        &PhysicsProfile::cpm(),
        "cpm",
    );
    let back = Recording::from_bytes(&rec.to_bytes()).unwrap();
    let bits = |v: Vec3| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
    assert_eq!(bits(back.start().spawn), bits(odd.spawn));
    assert_eq!(back.start().yaw.to_bits(), odd.yaw.to_bits());
    assert_eq!(back.start().rate, TickRate::HZ_76);
    assert_eq!(back.world(), &WorldId::flat(s(-0.0)));
    assert_ne!(back.world(), &WorldId::flat(s(0.0)));
}

#[test]
fn an_empty_recording_is_still_a_valid_file() {
    let rec = Recording::record(
        start(),
        Vec::new(),
        &EmptyWorld,
        WorldId::Empty,
        &PhysicsProfile::cpm(),
        "cpm",
    );
    let back = Recording::from_bytes(&rec.to_bytes()).unwrap();
    assert_eq!(back.command_count(), 0);
    assert_eq!(back.claimed().sim_time_ms, 0);
    assert_eq!(back.claimed().run_time_ms, None);
    assert_eq!(
        back.verify(&EmptyWorld, &WorldId::Empty, &PhysicsProfile::cpm()),
        Ok(back.claimed())
    );
}

#[test]
fn a_non_ascii_name_is_counted_in_bytes() {
    let rec = Recording::record(
        start(),
        commands(4),
        &FlatGround::at(s(0.0)),
        WorldId::map("κοίλο-ø", 7),
        &PhysicsProfile::cpm(),
        "cpm-ü",
    );
    let back = Recording::from_bytes(&rec.to_bytes()).unwrap();
    assert_eq!(back.world().map_name(), Some("κοίλο-ø"));
    assert_eq!(back.physics().name, "cpm-ü");
}

#[test]
fn the_file_starts_with_the_magic_and_the_version() {
    let bytes = recorded(4).to_bytes();
    assert_eq!(&bytes[..4], &MAGIC);
    assert_eq!(
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        FORMAT_VERSION
    );
    assert_eq!(
        u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        0,
        "the compact form sets no flags"
    );
    let traced = recorded(4).to_bytes_with_checksums().unwrap();
    assert_eq!(
        u32::from_le_bytes([traced[8], traced[9], traced[10], traced[11]]),
        FLAG_TRACE
    );
}

// ── re-simulation ───────────────────────────────────────────────────────────

#[test]
fn a_decoded_recording_re_simulates_to_what_it_claims() {
    let rec = recorded(200);
    let back = Recording::from_bytes(&rec.to_bytes_with_checksums().unwrap()).unwrap();
    let outcome = back
        .verify(&FlatGround::at(s(0.0)), &map_id(), &PhysicsProfile::cpm())
        .expect("verifies");
    assert_eq!(outcome, rec.claimed());
    assert!(outcome.digest != 0);
    assert_eq!(outcome.sim_time_ms, 200 * 8);
}

#[test]
fn replaying_visits_every_command_and_ends_where_verify_says() {
    let rec = recorded(50);
    let mut seen = Vec::new();
    let out = rec
        .replay(
            &FlatGround::at(s(0.0)),
            &map_id(),
            &PhysicsProfile::cpm(),
            |i, state| seen.push((i, state.time_ms, state.checksum())),
        )
        .expect("replays");
    assert_eq!(seen.len(), 50);
    assert_eq!(seen[0].0, 0);
    assert_eq!(seen[49].0, 49);
    assert_eq!(out, rec.claimed());
    // The states handed to the observer are the ones the digest folded.
    assert_eq!(
        fold_all(seen.iter().map(|(_, _, c)| *c)),
        rec.claimed().digest
    );
    // ...and those are exactly the recorded trace.
    assert_eq!(
        seen.iter().map(|(_, _, c)| *c).collect::<Vec<_>>(),
        rec.trace().unwrap()
    );
}

#[test]
fn the_run_digest_is_the_fold_of_the_per_command_checksums() {
    let rec = recorded(37);
    assert_eq!(
        fold_all(rec.trace().unwrap().iter().copied()),
        rec.claimed().digest
    );
}

#[test]
fn simulating_the_same_recording_twice_gives_the_same_answer() {
    let rec = recorded(80);
    let a = rec
        .resimulate(&FlatGround::at(s(0.0)), &map_id(), &PhysicsProfile::cpm())
        .unwrap();
    let b = rec
        .resimulate(&FlatGround::at(s(0.0)), &map_id(), &PhysicsProfile::cpm())
        .unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_run_that_never_finished_reports_no_time() {
    // `FlatGround` has no trigger volumes, so the clock never starts. The
    // recording is valid and simply is not a time — the distinction a
    // leaderboard depends on.
    let rec = recorded(20);
    assert_eq!(rec.claimed().run_time_ms, None);
    assert_eq!(rec.claimed().sim_time_ms, 160);
}

// ── C6: the binding is not optional ─────────────────────────────────────────

#[test]
fn a_recompiled_map_is_refused_and_named_as_such() {
    let rec = recorded(20);
    let recompiled = WorldId::map("coil", 0x0123_4567_89ab_cdee);
    let err = rec
        .commands_for(&recompiled, &PhysicsProfile::cpm())
        .expect_err("stale geometry must be refused");
    assert!(err.is_stale_geometry());
    let message = err.to_string();
    assert!(
        message.contains("different compilation of the same map"),
        "{message}"
    );
    assert!(message.contains("different time"), "{message}");

    // ...and every other door into the commands is shut too.
    assert!(
        rec.resimulate(&FlatGround::at(s(0.0)), &recompiled, &PhysicsProfile::cpm())
            .is_err()
    );
    assert!(
        rec.replay(
            &FlatGround::at(s(0.0)),
            &recompiled,
            &PhysicsProfile::cpm(),
            |_, _| {}
        )
        .is_err()
    );
    assert_eq!(
        rec.verify(&FlatGround::at(s(0.0)), &recompiled, &PhysicsProfile::cpm()),
        Err(VerifyError::Mismatch(Mismatch::World {
            recorded: map_id(),
            actual: recompiled,
        }))
    );
}

#[test]
fn a_different_map_is_refused_but_is_not_stale_geometry() {
    let rec = recorded(8);
    let err = rec
        .commands_for(&WorldId::map("other", 42), &PhysicsProfile::cpm())
        .unwrap_err();
    assert!(!err.is_stale_geometry());
    assert!(err.to_string().contains("other"));
}

#[test]
fn renaming_a_map_does_not_invalidate_a_run() {
    let rec = recorded(8);
    assert!(
        rec.commands_for(
            &WorldId::map("coil-final-FINAL", 0x0123_4567_89ab_cdef),
            &PhysicsProfile::cpm()
        )
        .is_ok()
    );
}

#[test]
fn a_changed_movement_constant_is_refused() {
    let rec = recorded(8);
    let mut tweaked = PhysicsProfile::cpm();
    tweaked.air_accelerate = f32::from_bits(tweaked.air_accelerate.to_bits() + 1);
    let err = rec.commands_for(&map_id(), &tweaked).unwrap_err();
    assert!(matches!(err, Mismatch::Physics { .. }));
    assert!(err.to_string().contains("movement constants"));
    // And the obvious one: the other profile entirely.
    assert!(rec.commands_for(&map_id(), &PhysicsProfile::vq3()).is_err());
}

#[test]
fn the_physics_binding_is_taken_from_the_profile_that_will_be_used() {
    // There is no argument through which a caller can assert a physics
    // identity: `commands_for` derives it from the profile it is handed, so a
    // caller cannot claim `cpm` and then simulate with `vq3`.
    let rec = recorded(8);
    assert_eq!(rec.physics(), &PhysicsId::of("cpm", &PhysicsProfile::cpm()));
    assert!(rec.commands_for(&map_id(), &PhysicsProfile::cpm()).is_ok());
}

#[test]
fn the_analytic_worlds_do_not_substitute_for_each_other() {
    let rec = Recording::record(
        start(),
        commands(8),
        &FlatGround::at(s(0.0)),
        WorldId::flat(s(0.0)),
        &PhysicsProfile::cpm(),
        "cpm",
    );
    assert!(
        rec.commands_for(&WorldId::flat(s(64.0)), &PhysicsProfile::cpm())
            .is_err()
    );
    assert!(
        rec.commands_for(&WorldId::Empty, &PhysicsProfile::cpm())
            .is_err()
    );
    assert!(
        rec.commands_for(&WorldId::flat(s(0.0)), &PhysicsProfile::cpm())
            .is_ok()
    );
}

#[test]
fn a_divergence_is_reported_with_the_command_it_started_on() {
    // Fake the divergence by verifying against a world the recording was
    // (wrongly) told it ran in: the binding passes, the physics passes, and
    // the simulation is different. This is the shape a real cross-target
    // divergence would take.
    let honest = recorded(60);
    let mislabelled = Recording::record(
        start(),
        honest.commands_unchecked().to_vec(),
        &FlatGround::at(s(0.0)),
        map_id(),
        &PhysicsProfile::cpm(),
        "cpm",
    );
    let err = mislabelled
        .verify(&FlatGround::at(s(-32.0)), &map_id(), &PhysicsProfile::cpm())
        .expect_err("a different floor is a different run");
    match err {
        VerifyError::Diverged {
            first_diverging_command,
            claimed,
            actual,
        } => {
            assert_ne!(claimed.digest, actual.digest);
            // Not command 0: the player spawns airborne and falls, and the two
            // runs are bit-identical until the higher floor stops one of them.
            // That is worth asserting rather than merely tolerating — it is
            // the trace earning its keep, naming the command where physics
            // first differed instead of the command where the file was opened.
            let at = first_diverging_command.expect("a traced recording localises it");
            assert!(at > 0, "diverged at command {at}, before either run landed");
            let recorded_trace = mislabelled.trace().unwrap();
            assert!(
                recorded_trace[..at as usize]
                    .iter()
                    .zip(honest.trace().unwrap())
                    .all(|(a, b)| a == b),
                "the commands before {at} were reported as agreeing and do not"
            );
        }
        other => panic!("expected a divergence, got {other}"),
    }
}

#[test]
fn a_divergence_without_a_trace_says_so_rather_than_guessing() {
    // Long enough to reach the floor: the player spawns at z=64 and two
    // different floors are the same run until one of them is landed on.
    let rec = recorded(60);
    let compact = Recording::from_bytes(&rec.to_bytes()).unwrap();
    let err = compact
        .verify(&FlatGround::at(s(-32.0)), &map_id(), &PhysicsProfile::cpm())
        .unwrap_err();
    assert!(matches!(
        err,
        VerifyError::Diverged {
            first_diverging_command: None,
            ..
        }
    ));
    assert!(err.to_string().contains("no checksum trace"));
}

// ── every way a file can lie ────────────────────────────────────────────────

#[test]
fn a_file_that_is_not_one_is_refused() {
    assert!(matches!(
        Recording::from_bytes(&[0u8; 64]),
        Err(LoadError::NotAnS3d { .. })
    ));
    assert!(matches!(
        Recording::from_bytes(b"S3D"),
        Err(LoadError::Truncated { .. })
    ));
    assert!(matches!(
        Recording::from_bytes(&[]),
        Err(LoadError::Truncated { .. })
    ));
}

#[test]
fn a_future_version_is_refused_by_version_and_not_called_corrupt() {
    let mut bytes = recorded(4).to_bytes();
    bytes[4] = 2;
    assert_eq!(
        Recording::from_bytes(&bytes),
        Err(LoadError::UnsupportedVersion {
            found: 2,
            supported: FORMAT_VERSION
        })
    );
}

#[test]
fn an_unknown_flag_bit_is_refused() {
    let mut bytes = recorded(4).to_bytes();
    bytes[11] = 0x80; // top byte of the little-endian flags word
    // The content digest is checked first, so fix it up: the point of this
    // test is the flag, not the corruption.
    let fixed = reseal(bytes);
    assert!(matches!(
        Recording::from_bytes(&fixed),
        Err(LoadError::UnknownFlags { .. })
    ));
}

#[test]
fn every_single_byte_of_a_recording_is_covered_by_the_content_digest() {
    // Exhaustive over the file: flip one bit in each byte in turn, and the
    // load must fail. A digest that covered "most of" the file would let a
    // corrupted command through as a valid recording.
    let bytes = recorded(12).to_bytes();
    assert!(Recording::from_bytes(&bytes).is_ok());
    for i in 0..bytes.len() {
        let mut broken = bytes.clone();
        broken[i] ^= 0x01;
        assert!(
            Recording::from_bytes(&broken).is_err(),
            "flipping a bit of byte {i} of {} produced a loadable recording",
            bytes.len()
        );
    }
}

#[test]
fn a_truncated_file_is_refused_at_every_length() {
    let bytes = recorded(12).to_bytes();
    for cut in 0..bytes.len() {
        assert!(
            Recording::from_bytes(&bytes[..cut]).is_err(),
            "the first {cut} bytes loaded as a recording"
        );
    }
}

#[test]
fn appended_bytes_are_refused() {
    let mut bytes = recorded(6).to_bytes();
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    assert!(Recording::from_bytes(&bytes).is_err());
}

#[test]
fn a_digest_spliced_in_from_another_run_is_caught_at_load() {
    // Criterion 2's rule, applied to a saved run: a file's digest must be
    // derived from that file's own per-command checksums. Here the header's
    // digest is replaced with a plausible one from a different run, and the
    // trace is left alone.
    let mine = recorded(60);
    let theirs = Recording::record(
        start(),
        commands(60),
        &FlatGround::at(s(-8.0)),
        map_id(),
        &PhysicsProfile::cpm(),
        "cpm",
    );
    assert_ne!(mine.claimed().digest, theirs.claimed().digest);

    let mut bytes = mine.to_bytes_with_checksums().unwrap();
    let at = find(&bytes, &mine.claimed().digest.to_le_bytes()).expect("digest is in the header");
    bytes[at..at + 8].copy_from_slice(&theirs.claimed().digest.to_le_bytes());
    let resealed = reseal(bytes);

    assert!(matches!(
        Recording::from_bytes(&resealed),
        Err(LoadError::DigestNotDerivedFromTrace { .. })
    ));
}

#[test]
fn a_spliced_digest_in_a_compact_file_survives_load_and_dies_at_verification() {
    // The honest limit of the compact form, stated as a test rather than left
    // to be discovered: with no trace there is nothing at load time to check
    // the digest against, so the lie is caught by re-simulating — which is the
    // only thing that could ever have caught it, and is what `verify` is for.
    let mine = recorded(60);
    let mut bytes = mine.to_bytes();
    let at = find(&bytes, &mine.claimed().digest.to_le_bytes()).expect("digest is in the header");
    bytes[at..at + 8].copy_from_slice(&0xdead_beef_dead_beef_u64.to_le_bytes());
    let resealed = reseal(bytes);

    let loaded = Recording::from_bytes(&resealed).expect("load cannot see the lie");
    assert_eq!(loaded.claimed().digest, 0xdead_beef_dead_beef);
    assert!(matches!(
        loaded.verify(&FlatGround::at(s(0.0)), &map_id(), &PhysicsProfile::cpm()),
        Err(VerifyError::Diverged { .. })
    ));
}

#[test]
fn an_impossible_command_rate_is_refused() {
    let rec = recorded(4);
    let mut bytes = rec.to_bytes();
    // `rate_hz` is the first field of the header, at offset 16.
    bytes[16..20].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        Recording::from_bytes(&reseal(bytes)),
        Err(LoadError::BadRate { hz: 0 })
    ));
}

#[test]
fn a_command_count_larger_than_the_file_costs_a_comparison_not_an_allocation() {
    // The field is attacker-controlled in the only sense that matters — it
    // came out of a file — so it is checked against what is actually left
    // before anything is reserved for it.
    let mut bytes = recorded(4).to_bytes();
    bytes[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Recording::from_bytes(&reseal(bytes)),
        Err(LoadError::Truncated { .. })
    ));
}

#[test]
fn a_run_finished_byte_that_is_neither_is_refused() {
    let mut bytes = recorded(4).to_bytes();
    // rate, count, sim_time, run_time = 16 bytes of header, then the flag.
    bytes[16 + 16] = 2;
    assert!(matches!(
        Recording::from_bytes(&reseal(bytes)),
        Err(LoadError::BadBool { .. })
    ));
}

#[test]
fn an_unknown_world_tag_is_refused() {
    let mut bytes = recorded(4).to_bytes();
    bytes[16 + 17] = 9;
    assert!(matches!(
        Recording::from_bytes(&reseal(bytes)),
        Err(LoadError::BadWorldTag { tag: 9 })
    ));
}

#[test]
fn an_absurd_name_length_is_refused_before_it_is_allocated() {
    let rec = recorded(4);
    let mut bytes = rec.to_bytes();
    // Located by the name's own bytes rather than by arithmetic on the
    // layout: the length prefix is the four bytes immediately before it.
    let name = find(&bytes, b"coil").expect("the map name is in the file");
    bytes[name - 4..name].copy_from_slice(&(MAX_NAME_BYTES + 1).to_le_bytes());
    assert!(matches!(
        Recording::from_bytes(&reseal(bytes)),
        Err(LoadError::NameTooLong { .. })
    ));
}

#[test]
fn a_name_that_is_not_utf8_is_refused() {
    let rec = Recording::record(
        start(),
        commands(2),
        &FlatGround::at(s(0.0)),
        WorldId::map("abcd", 1),
        &PhysicsProfile::cpm(),
        "cpm",
    );
    let mut bytes = rec.to_bytes();
    let at = find(&bytes, b"abcd").expect("the name is in the file");
    bytes[at] = 0xff;
    assert!(matches!(
        Recording::from_bytes(&reseal(bytes)),
        Err(LoadError::BadUtf8 { .. })
    ));
}

#[test]
fn a_declared_header_length_that_is_wrong_is_refused() {
    let mut bytes = recorded(4).to_bytes();
    let declared = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    bytes[12..16].copy_from_slice(&(declared - 1).to_le_bytes());
    assert!(matches!(
        Recording::from_bytes(&reseal(bytes)),
        Err(LoadError::HeaderLength { .. })
    ));
}

// ── outcome semantics ───────────────────────────────────────────────────────

#[test]
fn an_unfinished_run_and_a_zero_millisecond_run_are_different_files() {
    // `run_time_ms: None` and `Some(0)` must not encode the same, or a run
    // that never started would load as an unbeatable personal best.
    let base = recorded(4);
    let unfinished = base.to_bytes();
    let zero = crate::recording::Recording::from_parts(
        *base.start(),
        base.commands_unchecked().to_vec(),
        base.world().clone(),
        base.physics().clone(),
        Outcome {
            run_time_ms: Some(0),
            ..base.claimed()
        },
        None,
    )
    .to_bytes();
    assert_ne!(unfinished, zero);
    assert_eq!(
        Recording::from_bytes(&zero).unwrap().claimed().run_time_ms,
        Some(0)
    );
    assert_eq!(
        Recording::from_bytes(&unfinished)
            .unwrap()
            .claimed()
            .run_time_ms,
        None
    );
}

#[test]
fn the_claimed_outcome_is_a_claim_until_it_is_verified() {
    let rec = recorded(10);
    let claimed: Outcome = rec.claimed();
    assert_eq!(
        rec.verify(&FlatGround::at(s(0.0)), &map_id(), &PhysicsProfile::cpm()),
        Ok(claimed)
    );
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// Recompute the trailing content digest after a test has edited a file, so
/// the test exercises the check it is aiming at rather than the corruption
/// check that guards everything.
fn reseal(mut bytes: Vec<u8>) -> Vec<u8> {
    let body_len = bytes.len() - 8;
    let mut h = crate::digest::Fnv1a::new();
    h.bytes(&bytes[..body_len]);
    bytes[body_len..].copy_from_slice(&h.finish().to_le_bytes());
    bytes
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

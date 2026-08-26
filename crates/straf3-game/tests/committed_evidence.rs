//! The guard for the committed competitive-loop evidence — designed, not yet armed.
//!
//! # Why this file has no tests in it yet
//!
//! The guard it describes cannot compile until two things exist: the canonical
//! profile (`straf3_game::profile::straf3()`, absent as of this commit — see
//! `crates/straf3-sim/src/profile.rs`, which has `vq3()`, `cpm()` and
//! `experimental()` and nothing else) and the artefact itself. Writing it now
//! and leaving it broken would break every other seat's build, so the design
//! lands here instead of in a conversation, and the "Arming it" checklist below
//! is the whole of the work.
//!
//! **This file does not satisfy the requirement it describes.** It is the
//! design, in the tree, so that whoever captures the evidence executes it
//! rather than re-deriving it.
//!
//! # The problem it exists to solve
//!
//! `runs/coil.cpm.s3d` was committed as evidence of a personal best. It was
//! invalidated the same day by `a604820` — which added eight fields to
//! `PhysicsProfile` and changed no existing constant — and nobody found out for
//! nine days. Not through inattention: **nothing in the tree loaded it**, so
//! there was no moment at which the breakage could announce itself. It was
//! discovered only when a measurement session tried to seed a personal best
//! from it by accident. See `crates/straf3-replay/src/identity.rs`,
//! "What that costs: this digest identifies the representation, not the
//! behaviour".
//!
//! A committed artefact that nothing loads is a claim with an expiry date
//! nobody can see. This repository's README already refuses that argument in
//! prose about checksums pasted into documents; this is the same object in
//! binary form.
//!
//! # Why the client cannot be where this fails
//!
//! Because the client is deliberately built not to fail there. A digest
//! mismatch reaches the player as `log::warn!` — *"the personal best at {path}
//! cannot be raced here: {e}"* — and the session **continues without a ghost**.
//! That is the right behaviour for someone who wants to play: a stale record
//! should not stop a game. It also means a stale artefact produces a session
//! that looks entirely normal, so "I played it and it was fine" is not
//! evidence. The loud failure has to live in the test suite.
//!
//! # Why a test and not an xtask check
//!
//! `cargo test --workspace` is the standing gate and runs by default.
//! `cargo xtask check-seam` is a command someone has to remember. The point is
//! that the failure reaches whoever changed the physics *at the moment they
//! change it*, without them opting in.
//!
//! # Why the digest and not the run time
//!
//! `windowed_playback.rs` already guards the committed *command stream* by
//! asserting `COIL_RUN_MS`, and that catches **behavioural** drift. It cannot
//! catch what killed `runs/coil.cpm.s3d`: `PhysicsId` folds an exhaustive
//! destructure with no `..`, so a behaviourally neutral field still moves the
//! digest while every run still takes exactly 5096 ms. Both checks are needed
//! and they catch different things.
//!
//! # Arming it
//!
//! 1. Bind the artefact at compile time, next to the map:
//!
//!    ```ignore
//!    const CANON_PB: &[u8] = include_bytes!("../../../runs/coil.<canon>.s3d");
//!    const COIL_MAP: &str = include_str!("../../../assets/maps/coil.map");
//!    /// The time the artefact records, asserted so a *swapped* artefact is
//!    /// caught and not only a stale one — the job `COIL_RUN_MS` does.
//!    const CANON_PB_MS: u32 = /* from the run */;
//!    ```
//!
//!    `include_bytes!` rather than a runtime read, for the reason
//!    `windowed_playback.rs` gives for its own fixture: "a test cannot pass
//!    against a file that is not actually committed." Deleting or renaming the
//!    artefact must be a *build* failure, not a silently skipped test.
//!
//! 2. The load test. Install the map from source so the `WorldId` is recomputed
//!    by the *current* compiler — this catches map-compiler drift as well as
//!    physics drift, which are the two bindings `identity.rs` describes:
//!
//!    ```ignore
//!    straf3_game::scene::install("coil", COIL_MAP).expect("coil.map must compile");
//!    let world = straf3_game::WorldChoice::Map;
//!    let world_id = world.world_id().expect("the coil map is installed above");
//!    let recording = straf3_replay::Recording::from_bytes(CANON_PB).unwrap();
//!    // Mirrors app.rs's own call, including the `&world.world()` shape.
//!    match straf3_game::ghost::Ghost::from_recording(
//!        &recording, &world.world(), &world_id, &straf3_game::profile::straf3(),
//!    ) { /* see below */ }
//!    ```
//!
//!    Match `NeverStarted` and `NeverFinished` **separately** from `Mismatch`.
//!    A truncated artefact reported as "the physics changed" sends the reader
//!    hunting a movement regression that never happened.
//!
//! 3. Assert `recording.physics().name` is the canon name and
//!    `ghost.run_time_ms() == CANON_PB_MS`.
//!
//! 4. Delete this paragraph and the "no tests in it yet" section above.
//!
//! # The panic text, written out
//!
//! This is the deliverable, not a sketch: the person who trips this guard is
//! mid-way through a physics change and wants it green, and whatever the
//! message does not tell them, they will not go and look up.
//!
//! ```text
//! The committed personal best no longer loads under this build's physics.
//!
//!   {the Mismatch error, which names the recorded and current digests}
//!
//! THIS FAILURE IS CORRECT. It is not a bug in the artefact and it is
//! probably not a bug in your change.
//!
//! READ THIS BEFORE HUNTING A MOVEMENT REGRESSION: the physics may not have
//! changed at all. PhysicsId folds an exhaustive destructure of
//! PhysicsProfile, so *adding a field* moves every profile's digest even at a
//! disabling value, even when no constant changed and every run still takes
//! the same time. That is exactly what a604820 did. See
//! crates/straf3-replay/src/identity.rs, "this digest identifies the
//! representation, not the behaviour".
//!
//! TO FIX: re-record the run on the real GPU and re-commit the artefact,
//! together with its screenshot. PLAYTEST.md section 1 has the route.
//!
//! DO NOT regenerate it headlessly to make this green. A .s3d can be produced
//! by the test harness with no window and no GPU, and that will pass this
//! test — but the artefact is committed as evidence of a run on real
//! hardware, and PLAYING.md describes it that way. Regenerating it headlessly
//! silently turns evidence into a fixture and leaves the documentation
//! claiming something untrue. If you do it anyway, change the documentation
//! in the same commit.
//! ```
//!
//! That last paragraph is the one this file most needs. The fix for "an
//! artefact can rot unnoticed" introduces "an artefact can be quietly
//! downgraded from evidence to fixture, with the guard green and the
//! documentation unchanged" — the same failure one turn further on. The
//! remediation has to sit where the failing person is looking, and where they
//! are looking is the panic.
//!
//! # The maintenance cost, stated rather than discovered
//!
//! Arming this makes a GPU session the price of any change that widens
//! `PhysicsProfile`. That is a real cost falling on a real person, and it is
//! the correct trade only because the alternative is an artefact that lies.
//! It belongs in the published caveats as well as here.

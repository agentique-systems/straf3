//! The `/v1` surface, against a real Postgres.
//!
//! These are the tests that carry requirements r7, r8, r9 and r10. Each one
//! names the property it is about in its own name, so a failure reads as
//! "pinned categories stopped meaning what they meant" rather than as
//! "test_17 failed".

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use straf3_records::{digest16, profiles};
use support::Harness;
use support::fixture::{self, TestCourse};
use uuid::Uuid;

// ── the surface exists and answers ──────────────────────────────────────────

#[tokio::test]
async fn health_is_green_only_because_the_database_actually_round_trips() {
    let _url = require_database!();
    let harness = Harness::new("health").await;

    let (status, body) = harness.get("/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
    // The build this service *is*, derived by running `crosstarget`.
    assert_eq!(body["native_verifier_ok"], true);

    harness.cleanup().await;
}

#[tokio::test]
async fn meta_names_the_physics_the_client_should_be_running() {
    let _url = require_database!();
    let harness = Harness::new("meta").await;
    harness.seed().await;

    let (status, body) = harness.get("/v1/meta").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["default_family"], profiles::DEFAULT_FAMILY);
    // Null, not a plausible number: this service does not build the bundle.
    assert!(body["sim_build"]["wasm_hash"].is_null());

    let families: Vec<&str> = body["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["family"].as_str().unwrap())
        .collect();
    assert!(families.contains(&"cpm") && families.contains(&"vq3"));

    for profile in body["profiles"].as_array().unwrap() {
        let digest = profile["digest"].as_str().unwrap();
        assert_eq!(digest.len(), 16, "every digest is 16 hex characters");
        assert_eq!(digest, digest.to_lowercase(), "and lowercase");
    }

    harness.cleanup().await;
}

/// The tripwire `coordinator` asked for: if `straf3-map` or the map text moves,
/// every stored run's world identity silently stops matching, and the symptom
/// would otherwise be an unexplained wave of `rejected` verdicts.
///
/// The number is asserted here and derived by the seed. It is *not* in the
/// migration.
#[tokio::test]
async fn coil_seeds_to_the_collision_digest_five_builds_agreed_on() {
    let _url = require_database!();
    let harness = Harness::new("coildigest").await;
    let report = harness.seed().await;

    let coil = report
        .maps
        .iter()
        .find(|(slug, _)| slug == "coil")
        .expect("coil is in assets/maps");
    assert_eq!(
        coil.1, 0x4726_3b88_45d8_bb4b,
        "coil's derived collision digest moved; a recording made against the old geometry will \
         now be refused, which is correct but needs explaining"
    );

    let (status, body) = harness.get("/v1/maps/coil").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["collision_digest"], "47263b8845d8bb4b");
    assert_eq!(body["source_url"], "/assets/maps/coil.map");
    assert_eq!(body["has_timing"], true);

    harness.cleanup().await;
}

// ── r9: the two kinds of "no rows" ──────────────────────────────────────────

/// Requirement r9, the "nobody has set a time" half.
#[tokio::test]
async fn an_empty_board_is_a_200_whose_body_says_it_is_empty() {
    let _url = require_database!();
    let harness = Harness::new("emptyboard").await;
    harness.seed().await;

    let (status, body) = harness.get("/v1/maps/coil/leaderboard?profile=cpm").await;

    assert_eq!(status, StatusCode::OK, "an empty board is a success");
    assert_eq!(body["entries"], json!([]));
    assert_eq!(body["total"], 0);
    assert_eq!(body["category"]["map"], "coil");
    assert_eq!(body["category"]["family"], "cpm");
    assert_eq!(body["category"]["pinned"], false);
    // Not a bare array, and not a 204: the body has to be able to say *which*
    // board is empty, or the site cannot render "nobody has set a time on the
    // coil CPM board yet".
    assert!(body.is_object());
    assert!(body["error"].is_null());

    harness.cleanup().await;
}

/// Requirement r9, the "could not answer" half — the one that matters, because
/// this is the case an implementation flattens into an empty array by accident.
#[tokio::test]
async fn a_board_the_service_cannot_answer_for_is_not_an_empty_board() {
    let _url = require_database!();

    // A pool pointed at nothing. Everything else about the service is real.
    let harness = Harness::unreachable_database();

    let (status, body) = harness.get("/v1/maps/coil/leaderboard?profile=cpm").await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "an unanswerable board must not be a 200"
    );
    assert_eq!(body["error"], "database_unavailable");
    assert!(
        body["detail"].as_str().is_some_and(|d| !d.is_empty()),
        "the envelope carries a sentence a person can act on"
    );
    assert!(
        body["entries"].is_null(),
        "there is no entries array at all — an empty one would be a claim nobody can make"
    );

    // ...and health says the same thing, which is what makes the 503 honest.
    let (status, body) = harness.get("/v1/health").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "database_unavailable");
}

// ── r8: the category key ────────────────────────────────────────────────────

/// Requirement r8's refusal case, and URLS.md §3's third bullet: a pinned board
/// whose digest is unknown renders as **unknown** — not as empty, and not as
/// the current board.
#[tokio::test]
async fn an_unknown_pinned_digest_is_unknown_and_not_the_current_board() {
    let _url = require_database!();
    let harness = Harness::new("unknownpin").await;
    harness.seed().await;

    let (status, body) = harness
        .get("/v1/maps/coil/leaderboard?profile=cpm@ffffffffffffffff")
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_physics_digest");
    assert!(body["entries"].is_null(), "not an empty board");
    assert!(body["category"].is_null(), "and not the current board");

    // An uppercase pin is a refusal too, not a redirect (URLS.md §2).
    let (status, body) = harness
        .get("/v1/maps/coil/leaderboard?profile=cpm@FFFFFFFFFFFFFFFF")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_category");

    // A family that does not exist is its own code, so the site can say
    // something different about it.
    let (status, body) = harness.get("/v1/maps/coil/leaderboard?profile=quake").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_physics_family");

    harness.cleanup().await;
}

/// **Requirement r8, whole.** A category key pinned to a physics digest still
/// means what it meant after the physics changes.
///
/// Physics is not being frozen or tuned this wave, so this does not move a
/// movement constant — moving one would change the physics digest, which r12
/// forbids. It inserts a *second* immutable `physics_profiles` row for the same
/// family, which is exactly what a tuning pass does (§5.4: "tuning inserts a
/// row, it never updates one"), and then checks that the two URLs URLS.md §3
/// distinguishes really do resolve differently.
#[tokio::test]
async fn a_pinned_category_still_means_what_it_meant_after_the_physics_changes() {
    let _url = require_database!();
    let harness = Harness::new("pinnedcat").await;
    harness.seed().await;

    let original: i64 = sqlx::query_scalar(
        "select digest from physics_profiles where kind = 'cpm' order by created_at asc limit 1",
    )
    .fetch_one(&harness.pool)
    .await
    .unwrap();
    let original_hex = digest16::format(digest16::from_sql(original));

    // What a tuning pass does: a new row, with different constants, for the
    // same family, timestamped later. The bits differ from the real cpm
    // profile's, so this is genuinely different physics and not a relabelling.
    let tuned_digest: i64 = digest16::to_sql(0x1234_5678_9abc_def0);
    sqlx::query(
        "insert into physics_profiles (kind, label, digest, profile_bits, layout_version, \
                                       created_at) \
         values ('cpm', 'CPM (tuned, later)', $1, $2, 1, now() + interval '1 hour')",
    )
    .bind(tuned_digest)
    .bind(vec![0u8; 8])
    .execute(&harness.pool)
    .await
    .unwrap();

    // The mechanism r8 rests on, enforced by the database rather than by
    // habit: §5.4's "tuning inserts a row, it never updates one" is a trigger,
    // and it refuses even this test.
    let edit = sqlx::query("update physics_profiles set label = 'rewritten' where digest = $1")
        .bind(original)
        .execute(&harness.pool)
        .await;
    assert!(edit.is_err(), "a physics_profiles row must not be editable");
    let erase = sqlx::query("delete from physics_profiles where digest = $1")
        .bind(original)
        .execute(&harness.pool)
        .await;
    assert!(
        erase.is_err(),
        "nor deletable — a pinned board would lose its meaning"
    );

    // The unpinned key follows the family forward.
    let (status, body) = harness.get("/v1/maps/coil/leaderboard?profile=cpm").await;
    assert_eq!(status, StatusCode::OK);
    let current = body["category"]["digest"].as_str().unwrap().to_string();
    assert_eq!(
        current, "123456789abcdef0",
        "`/m/coil/cpm` means the *current* cpm board, and cpm was just tuned"
    );

    // The pinned key does not. This is r8.
    let (status, body) = harness
        .get(&format!(
            "/v1/maps/coil/leaderboard?profile=cpm@{original_hex}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["category"]["digest"], original_hex,
        "a board pinned to a digest must keep resolving to exactly that row"
    );
    assert_eq!(body["category"]["pinned"], true);
    assert_eq!(body["entries"], json!([]));
    assert_eq!(body["total"], 0);

    // And a pin naming a digest from the wrong family is refused rather than
    // helpfully resolved.
    let vq3: i64 =
        sqlx::query_scalar("select digest from physics_profiles where kind = 'vq3' limit 1")
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    let (status, body) = harness
        .get(&format!(
            "/v1/maps/coil/leaderboard?profile=cpm@{}",
            digest16::format(digest16::from_sql(vq3))
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_physics_digest");

    harness.cleanup().await;
}

// ── r7 / w3: nothing ranks until it has been re-simulated ───────────────────

/// **Requirement r7, whole.** A submitted run is ranked only after the service
/// re-simulates it and agrees.
///
/// The run is made on a fixture course rather than on `coil`: `coil` is a real
/// Defrag map whose finish trigger sits above a jump, and synthesising a
/// finishing run on it would be a test about movement rather than about the
/// service. The fixture is compiled through the same `straf3-map` and produces
/// a `.s3d` through the same `straf3-replay`, so the path under test is
/// identical.
#[tokio::test]
async fn a_run_is_pending_until_the_verifier_agrees_and_only_then_is_it_ranked() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("ranking").await;
    harness.seed().await;
    let map_id = course.insert(&harness.pool).await;

    let profile_id: i32 = sqlx::query_scalar("select id from physics_profiles where kind = 'cpm'")
        .fetch_one(&harness.pool)
        .await
        .unwrap();

    let recording = course.a_finishing_run();
    assert!(
        recording.claimed().run_time_ms.is_some(),
        "the fixture course must actually be finishable, or this test proves nothing"
    );
    let honest_time = recording.claimed().run_time_ms.unwrap();
    let bytes = recording.to_bytes_with_checksums().unwrap();

    // A ticket first: §7.2 step 1.
    let (status, ticket) = harness
        .post_json("/v1/attempts", &token, json!({"map": "fixture-course"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{ticket}");
    let ticket_id: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();

    let (status, submitted) = harness.post_demo(&token, ticket_id, bytes.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{submitted}");
    assert_eq!(
        submitted["status"], "pending",
        "intake never says verified: nothing has re-simulated it yet"
    );
    assert!(
        submitted["run_digest"].is_null(),
        "and it has no durable name yet: that digest is the one the service folds for itself"
    );
    let run_id = submitted["run_id"].as_str().unwrap().to_string();
    let run_digest = submitted["claimed_digest"].as_str().unwrap().to_string();

    // Before verification: no time, and the board is still empty.
    let (_, run) = harness.get(&format!("/v1/runs/{run_id}")).await;
    assert!(run["time_ms"].is_null(), "an unverified run has no time");
    assert!(run["demo"].is_null(), "and its bytes are not published yet");
    let (_, board) = harness
        .get("/v1/maps/fixture-course/leaderboard?profile=cpm")
        .await;
    assert_eq!(board["total"], 0, "nothing ranks before re-simulation");

    // Re-simulate, exactly as the verifier binary does.
    fixture::run_verifier(&harness.pool, &course, map_id, profile_id).await;

    let (status, run) = harness.get(&format!("/v1/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(run["status"], "verified");
    assert_eq!(
        run["time_ms"].as_i64().unwrap() as u32,
        honest_time,
        "the ranked time is the one this machine computed"
    );
    assert_eq!(
        run["run_digest"], run_digest,
        "an honest run's durable name is the digest it claimed, because the two agree"
    );
    assert_eq!(
        run["diagnostics"]["client_rolling_digest"], run_digest,
        "and the digests agree"
    );
    assert_eq!(run["diagnostics"]["server_rolling_digest"], run_digest);
    assert!(run["diagnostics"]["divergence_at"].is_null());
    assert_eq!(run["demo"], json!(format!("/v1/runs/{run_id}/demo")));

    // Now it is on the board.
    let (_, board) = harness
        .get("/v1/maps/fixture-course/leaderboard?profile=cpm")
        .await;
    assert_eq!(board["total"], 1);
    assert_eq!(board["entries"][0]["rank"], 1);
    assert_eq!(
        board["entries"][0]["time_ms"].as_i64().unwrap() as u32,
        honest_time
    );
    assert_eq!(board["entries"][0]["run_digest"], run_digest);
    // r10: the display name came from the verified token subject and from
    // nothing the client sent.
    assert_eq!(board["entries"][0]["player"], "Nova Tester");

    // And the recording is now downloadable, byte-identical.
    let (status, served) = harness
        .raw(
            Request::get(format!("/v1/runs/{run_id}/demo"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        served, bytes,
        "a ghost gets exactly the bytes that were submitted"
    );

    harness.cleanup().await;
}

/// The claimed time is carried and never ranked.
///
/// The header's `run_time_ms` is written independently of the commands, so a
/// file can claim any time it likes. The service stores the claim in
/// `client_time_ms` and ranks its own number — ARCHITECTURE §8.1: "we computed
/// the time; theirs was never an input."
#[tokio::test]
async fn a_lie_in_the_header_is_recorded_as_a_claim_and_never_as_the_time() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("claimedtime").await;
    harness.seed().await;
    let map_id = course.insert(&harness.pool).await;
    let profile_id: i32 = sqlx::query_scalar("select id from physics_profiles where kind = 'cpm'")
        .fetch_one(&harness.pool)
        .await
        .unwrap();

    let recording = course.a_finishing_run();
    let honest_time = recording.claimed().run_time_ms.unwrap();
    let bytes = fixture::claim_a_faster_time(&recording.to_bytes(), 1);
    // It still decodes, and it still carries the true rolling digest.
    let reread = straf3_replay::Recording::from_bytes(&bytes).unwrap();
    assert_eq!(reread.claimed().run_time_ms, Some(1));
    assert_eq!(reread.claimed().digest, recording.claimed().digest);

    let (_, ticket) = harness
        .post_json("/v1/attempts", &token, json!({"map": "fixture-course"}))
        .await;
    let ticket_id: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();
    let (status, submitted) = harness.post_demo(&token, ticket_id, bytes).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{submitted}");
    let run_id = submitted["run_id"].as_str().unwrap().to_string();

    fixture::run_verifier(&harness.pool, &course, map_id, profile_id).await;

    let (_, run) = harness.get(&format!("/v1/runs/{run_id}")).await;
    assert_eq!(run["status"], "verified");
    assert_eq!(
        run["diagnostics"]["client_time_ms"], 1,
        "the claim is kept, as a diagnostic"
    );
    assert_eq!(
        run["time_ms"].as_i64().unwrap() as u32,
        honest_time,
        "and the ranked time is the computed one, not the claimed one"
    );

    let (_, board) = harness
        .get("/v1/maps/fixture-course/leaderboard?profile=cpm")
        .await;
    assert_eq!(
        board["entries"][0]["time_ms"].as_i64().unwrap() as u32,
        honest_time
    );

    harness.cleanup().await;
}

/// The consequence of `runs.run_digest bigint` that the migration header
/// discloses, asserted rather than argued.
///
/// Rewriting the header digest produces a *different* `run_digest`, so the
/// global unique index does not stop the row being created. What stops it
/// mattering is that the verifier folds the digest from the commands
/// themselves: the forgery is `divergent`, `time_ms` stays null, and nothing
/// reaches the board.
#[tokio::test]
async fn a_forged_header_digest_can_make_a_row_but_can_never_make_a_ranked_time() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("forged").await;
    harness.seed().await;
    let map_id = course.insert(&harness.pool).await;
    let profile_id: i32 = sqlx::query_scalar("select id from physics_profiles where kind = 'cpm'")
        .fetch_one(&harness.pool)
        .await
        .unwrap();

    let honest = course.a_finishing_run();
    let forged = fixture::forge_run_digest(&honest.to_bytes(), honest.claimed().digest ^ 1);
    let reread = straf3_replay::Recording::from_bytes(&forged).expect("it still decodes");
    assert_ne!(
        reread.claimed().digest,
        honest.claimed().digest,
        "the claim is a different number, so nothing at intake rejects it"
    );

    let (_, ticket) = harness
        .post_json("/v1/attempts", &token, json!({"map": "fixture-course"}))
        .await;
    let ticket_id: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();
    let (status, submitted) = harness.post_demo(&token, ticket_id, forged).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "the row is created: {submitted}"
    );
    let run_id = submitted["run_id"].as_str().unwrap().to_string();

    fixture::run_verifier(&harness.pool, &course, map_id, profile_id).await;

    let (_, run) = harness.get(&format!("/v1/runs/{run_id}")).await;
    assert_eq!(
        run["status"], "divergent",
        "the rolling digest is recomputed from the commands, so the forgery is caught"
    );
    assert!(run["time_ms"].is_null(), "and it never gets a time");
    assert!(
        run["run_digest"].is_null(),
        "and it owns no digest, so it cannot squat the record it was aimed at"
    );
    assert!(run["demo"].is_null(), "nor are its bytes published");
    assert_ne!(
        run["diagnostics"]["client_rolling_digest"],
        run["diagnostics"]["server_rolling_digest"]
    );

    let (_, board) = harness
        .get("/v1/maps/fixture-course/leaderboard?profile=cpm")
        .await;
    assert_eq!(board["total"], 0, "and it never reaches a board");

    // The point of the partial index: the honest run this forgery was aimed at
    // must still be rankable afterwards. Under a plain global unique index on a
    // client-supplied digest it would collide forever.
    let (honest_token, _) = fixture::sign_in_as(&harness, "honest-subject", "Honest").await;
    let (_, ticket) = harness
        .post_json(
            "/v1/attempts",
            &honest_token,
            json!({"map": "fixture-course"}),
        )
        .await;
    let ticket_id: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();
    let (status, submitted) = harness
        .post_demo(&honest_token, ticket_id, honest.to_bytes())
        .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "the squat did not block it: {submitted}"
    );

    fixture::run_verifier(&harness.pool, &course, map_id, profile_id).await;

    let (_, board) = harness
        .get("/v1/maps/fixture-course/leaderboard?profile=cpm")
        .await;
    assert_eq!(board["total"], 1, "and it ranks");
    assert_eq!(board["entries"][0]["player"], "Honest");

    harness.cleanup().await;
}

/// §7.2 step 3 and §8.3: idempotency for the owner, refusal for anyone else.
///
/// Ownership is settled at **verification**, not at intake, and that is the
/// change the second migration makes. Intake only ever sees the digest the
/// submitter wrote in the header, so refusing there would refuse on a number
/// the submitter chose — which is a squat, not a protection. Refusing on the
/// digest this service folded is a protection.
#[tokio::test]
async fn a_run_belongs_to_whoever_was_verified_with_it_first() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("ownership").await;
    harness.seed().await;
    let map_id = course.insert(&harness.pool).await;
    let profile_id: i32 = sqlx::query_scalar("select id from physics_profiles where kind = 'cpm'")
        .fetch_one(&harness.pool)
        .await
        .unwrap();

    let bytes = course.a_finishing_run().to_bytes_with_checksums().unwrap();

    let ticket_for = async |token: &str| -> Uuid {
        let (_, t) = harness
            .post_json("/v1/attempts", token, json!({"map": "fixture-course"}))
            .await;
        t["ticket"].as_str().unwrap().parse().unwrap()
    };

    let first = ticket_for(&token).await;
    let (status, a) = harness.post_demo(&token, first, bytes.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // The same player, the same run: idempotent, and it returns the original
    // rather than queueing the work twice.
    let second = ticket_for(&token).await;
    let (status, b) = harness.post_demo(&token, second, bytes.clone()).await;
    assert_eq!(status, StatusCode::OK, "a retried upload is not a new run");
    assert_eq!(a["run_id"], b["run_id"]);

    fixture::run_verifier(&harness.pool, &course, map_id, profile_id).await;

    // Now the digest is owned, by a number the server folded. This is §8.3's
    // case: downloading a ranked demo and re-posting it as your own.
    let (thief_token, _) = fixture::sign_in_as(&harness, "thief-subject", "Thief").await;
    let stolen = ticket_for(&thief_token).await;
    let (status, body) = harness.post_demo(&thief_token, stolen, bytes.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "run_already_submitted");

    // ...and the board still shows exactly one entry, the original player's.
    let (_, board) = harness
        .get("/v1/maps/fixture-course/leaderboard?profile=cpm")
        .await;
    assert_eq!(board["total"], 1);
    assert_eq!(board["entries"][0]["player"], "Nova Tester");

    harness.cleanup().await;
}

/// Two players race the same run past intake before either is verified. Both
/// rows exist; the partial unique index decides at verification, and the loser
/// is refused rather than crashing the verifier.
#[tokio::test]
async fn two_pending_copies_of_one_run_are_resolved_by_verification_not_by_intake() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("racecopies").await;
    harness.seed().await;
    let map_id = course.insert(&harness.pool).await;
    let profile_id: i32 = sqlx::query_scalar("select id from physics_profiles where kind = 'cpm'")
        .fetch_one(&harness.pool)
        .await
        .unwrap();
    let bytes = course.a_finishing_run().to_bytes_with_checksums().unwrap();

    let ticket_for = async |token: &str| -> Uuid {
        let (_, t) = harness
            .post_json("/v1/attempts", token, json!({"map": "fixture-course"}))
            .await;
        t["ticket"].as_str().unwrap().parse().unwrap()
    };

    let (other_token, _) = fixture::sign_in_as(&harness, "rival-subject", "Rival").await;
    let mine = ticket_for(&token).await;
    let theirs = ticket_for(&other_token).await;

    // Neither is verified yet, so intake has nothing to refuse on.
    let (status, _) = harness.post_demo(&token, mine, bytes.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, second) = harness.post_demo(&other_token, theirs, bytes).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    let loser = second["run_id"].as_str().unwrap().to_string();

    fixture::run_verifier(&harness.pool, &course, map_id, profile_id).await;

    let (_, run) = harness.get(&format!("/v1/runs/{loser}")).await;
    assert_eq!(
        run["status"], "rejected",
        "the second one through the verifier loses on the partial unique index"
    );
    assert!(
        run["reject_reason"]
            .as_str()
            .unwrap()
            .contains("verified with it first"),
        "and is told why: {}",
        run["reject_reason"]
    );
    assert!(run["run_digest"].is_null(), "the loser owns no digest");

    let (_, board) = harness
        .get("/v1/maps/fixture-course/leaderboard?profile=cpm")
        .await;
    assert_eq!(board["total"], 1, "one run, one board row");
    assert_eq!(board["entries"][0]["player"], "Nova Tester");

    harness.cleanup().await;
}

/// §7.2 step 2, at intake: a recording naming geometry or physics this service
/// has no row for is refused with the mismatch named.
#[tokio::test]
async fn a_recording_this_service_cannot_honour_is_refused_and_not_substituted() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("unhonourable").await;
    harness.seed().await;
    course.insert(&harness.pool).await;

    let (_, ticket) = harness
        .post_json("/v1/attempts", &token, json!({"map": "fixture-course"}))
        .await;
    let ticket_id: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();

    // A run made in a world whose collision digest this service has never seen.
    let stranger = course.a_run_in_another_world();
    let (status, body) = harness
        .post_demo(&token, ticket_id, stranger.to_bytes())
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "unknown_map");
    assert!(
        body["detail"].as_str().unwrap().contains("similar"),
        "the refusal says it is not matching a map that looks close: {}",
        body["detail"]
    );

    // A run made under physics this service has no profile for.
    let (_, ticket) = harness
        .post_json("/v1/attempts", &token, json!({"map": "fixture-course"}))
        .await;
    let ticket_id: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();
    let experimental = course.a_run_under_experimental_physics();
    let (status, body) = harness
        .post_demo(&token, ticket_id, experimental.to_bytes())
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "unknown_physics_digest");
    assert!(
        body["detail"].as_str().unwrap().contains("nearest"),
        "and this one says it is not ranking under the nearest profile: {}",
        body["detail"]
    );

    harness.cleanup().await;
}

// ── URLS.md §5: the two spellings of a run's name ───────────────────────────

#[tokio::test]
async fn a_run_answers_to_its_uuid_and_to_its_digest_with_the_same_body() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("bydigest").await;
    harness.seed().await;
    let map_id = course.insert(&harness.pool).await;

    let (_, ticket) = harness
        .post_json("/v1/attempts", &token, json!({"map": "fixture-course"}))
        .await;
    let ticket_id: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();
    let (_, submitted) = harness
        .post_demo(
            &token,
            ticket_id,
            course.a_finishing_run().to_bytes_with_checksums().unwrap(),
        )
        .await;
    let run_id = submitted["run_id"].as_str().unwrap().to_string();

    // `by-digest` resolves against the digest this service folded, so it finds
    // nothing until the run has been verified — a squatted claim resolves to
    // nothing rather than to somebody else's garbage.
    let claimed = submitted["claimed_digest"].as_str().unwrap().to_string();
    let (status, body) = harness.get(&format!("/v1/runs/by-digest/{claimed}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let profile_id: i32 = sqlx::query_scalar("select id from physics_profiles where kind = 'cpm'")
        .fetch_one(&harness.pool)
        .await
        .unwrap();
    fixture::run_verifier(&harness.pool, &course, map_id, profile_id).await;

    let (_, run) = harness.get(&format!("/v1/runs/{run_id}")).await;
    let digest = run["run_digest"].as_str().unwrap().to_string();
    assert_eq!(digest.len(), 16);

    let (status_a, by_id) = harness.get(&format!("/v1/runs/{run_id}")).await;
    let (status_b, by_digest) = harness.get(&format!("/v1/runs/by-digest/{digest}")).await;
    assert_eq!(status_a, StatusCode::OK);
    assert_eq!(status_b, StatusCode::OK);
    assert_eq!(by_id, by_digest, "URLS.md §5: the same body, both ways");

    // Uppercase is a 404, not a redirect (URLS.md §2).
    let (status, body) = harness
        .get(&format!("/v1/runs/by-digest/{}", digest.to_uppercase()))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_run");

    // As is a digest nobody has set.
    let (status, body) = harness.get("/v1/runs/by-digest/0000000000000000").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_run");

    harness.cleanup().await;
}

// ── r10 / w4: the token is the only thing that names a player ───────────────

/// **Requirement r10's mechanism.** A real Ed25519 JWT, signed by a key this
/// test generates and publishes as a JWK, verifies — and the `players` row it
/// resolves to is created from the token's own subject.
#[tokio::test]
async fn a_real_ed25519_token_verifies_and_names_a_player_created_from_its_subject() {
    let _url = require_database!();
    let (harness, token, subject) = fixture::signed_in("ed25519").await;

    // The route refuses without one...
    let (status, body) = harness.get("/v1/maps/coil/leaderboard/me").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "not_authenticated");

    harness.seed().await;

    // ...and answers with one, naming a player nothing in the request named.
    let (status, body) = harness
        .get_auth("/v1/maps/coil/leaderboard/me", &token)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["player"], "Nova Tester");
    assert!(
        body["entry"].is_null(),
        "no time here yet — null, which is not an error and not an empty board"
    );

    let stored: String = sqlx::query_scalar(
        "select p.display_name from players p join identities i on i.player_id = p.id \
          where i.provider = 'neon-auth' and i.provider_user_id = $1",
    )
    .bind(&subject)
    .fetch_one(&harness.pool)
    .await
    .expect("the verified subject created a players row");
    assert_eq!(stored, "Nova Tester");

    harness.cleanup().await;
}

/// Every way a token can be wrong, and the refusal naming which.
#[tokio::test]
async fn a_token_that_is_wrong_is_refused_with_the_reason_named() {
    let _url = require_database!();
    let keys = fixture::TestKeys::new();
    let harness =
        Harness::with_jwks("badtokens", keys.jwks(Some("https://issuer.test".into()))).await;

    let cases: Vec<(&str, String, &str)> = vec![
        ("expired", keys.token_expired_an_hour_ago("s"), "expired"),
        (
            "wrong issuer",
            keys.token_from_another_issuer("s"),
            "authority",
        ),
        (
            "bad signature",
            keys.token_with_a_broken_signature("s"),
            "signature",
        ),
        (
            "unknown kid",
            keys.token_from_an_unpublished_key("s"),
            "key",
        ),
    ];

    for (name, token, expected) in cases {
        let (status, body) = harness
            .get_auth("/v1/maps/coil/leaderboard/me", &token)
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{name}: {body}");
        assert_eq!(body["error"], "not_authenticated", "{name}");
        let detail = body["detail"].as_str().unwrap().to_lowercase();
        assert!(
            detail.contains(expected),
            "{name}: the detail should say which check failed, got `{detail}`"
        );
    }

    harness.cleanup().await;
}

/// §7.2 step 1 and §7.3: a submission needs a live, unconsumed ticket of this
/// player's, for this category.
#[tokio::test]
async fn a_submission_without_a_live_ticket_of_your_own_is_refused() {
    let _url = require_database!();
    let course = TestCourse::new();
    let (harness, token, _player) = fixture::signed_in("tickets").await;
    harness.seed().await;
    course.insert(&harness.pool).await;
    let bytes = course.a_finishing_run().to_bytes_with_checksums().unwrap();

    // No ticket header at all.
    let (status, body) = harness
        .raw(
            Request::post("/v1/runs")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(bytes.clone()))
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"], "missing_ticket");

    // A ticket nobody issued.
    let (status, body) = harness
        .post_demo(&token, Uuid::new_v4(), bytes.clone())
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "bad_ticket");

    // Somebody else's ticket.
    let (other_token, _) = fixture::sign_in_as(&harness, "other-subject", "Other").await;
    let (_, ticket) = harness
        .post_json(
            "/v1/attempts",
            &other_token,
            json!({"map": "fixture-course"}),
        )
        .await;
    let theirs: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();
    let (status, body) = harness.post_demo(&token, theirs, bytes.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "bad_ticket");
    assert!(body["detail"].as_str().unwrap().contains("issued to you"));

    // A ticket already spent.
    let (_, ticket) = harness
        .post_json("/v1/attempts", &token, json!({"map": "fixture-course"}))
        .await;
    let mine: Uuid = ticket["ticket"].as_str().unwrap().parse().unwrap();
    let (status, _) = harness.post_demo(&token, mine, bytes.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, body) = harness.post_demo(&token, mine, bytes).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "bad_ticket");
    assert!(body["detail"].as_str().unwrap().contains("single use"));

    harness.cleanup().await;
}

/// An unknown path under `/v1` is a `404` with the envelope, not an HTML page
/// and not a silent empty body. URLS.md §6: `/v1` is data, and the site's
/// SPA fallback must never be what answers here.
#[tokio::test]
async fn an_unknown_v1_path_is_a_404_carrying_the_envelope() {
    let _url = require_database!();
    let harness = Harness::new("fallback").await;

    let (status, body) = harness.get("/v1/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_endpoint");

    harness.cleanup().await;
}

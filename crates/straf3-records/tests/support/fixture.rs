//! The two things the integration tests need that the repository does not
//! already have: a course a run can actually finish on, and a real Ed25519
//! signing key.
//!
//! # Why a fixture course rather than `coil`
//!
//! `coil` is a real Defrag map. Its finish trigger sits at `z >= 64`, past a
//! jump, so synthesising a finishing run on it would be a test about the
//! movement model — and if the movement changed, the *service* tests would go
//! red for a reason that has nothing to do with the service.
//!
//! This course is a flat floor with a start line and a finish line, so "hold
//! forward" finishes. It is compiled through the same `straf3-map` and recorded
//! through the same `straf3-replay`, so every byte on the path under test is
//! the real one. `coil` still gets its own assertion — its derived collision
//! digest is pinned in `tests/api.rs`.
//!
//! # Why a real key rather than a stub verifier
//!
//! The Neon Auth JWKS publishes one `{"kty":"OKP","crv":"Ed25519"}` key. A test
//! that stubbed verification would prove that the stub returns what it was told
//! to. These tests generate an Ed25519 keypair, publish it as a JWK, sign a
//! real JWT with it, and make the service verify it through `jsonwebtoken` —
//! the same call, the same `Validation`, the same algorithm, as against Neon.
//! The key is generated from a fixed seed so the tests are deterministic; it is
//! a test key and it protects nothing.

use base64::Engine as _;
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::EncodePrivateKey;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::json;
use sqlx::PgPool;
use straf3_map::CompiledMap;
use straf3_records::auth::Jwks;
use straf3_records::verify::{RunStatus, verify_against};
use straf3_records::{digest16, simbuild};
use straf3_replay::{Recording, RunStart, WorldId};
use straf3_sim::{PhysicsProfile, TickRate, UserCmd, angle_to_short};
use uuid::Uuid;

use super::Harness;

/// The issuer the fixture tokens claim and the harness trusts.
pub const ISSUER: &str = "https://issuer.test";

// ── a course a run can finish on ────────────────────────────────────────────

/// A flat course with a start line and a finish line.
pub struct TestCourse {
    /// The compiled map, hulls and triggers.
    pub compiled: CompiledMap,
    /// The `.map` text it was compiled from.
    pub source: String,
}

impl Default for TestCourse {
    fn default() -> Self {
        Self::new()
    }
}

impl TestCourse {
    /// The slug it is seeded under.
    pub const SLUG: &'static str = "fixture-course";

    /// Compile it.
    pub fn new() -> Self {
        let source = course_source();
        let compiled = straf3_map::compile(&source).expect("the fixture course compiles");
        assert!(
            compiled.has_timing(),
            "the fixture course must have both a start and a finish trigger"
        );
        Self { compiled, source }
    }

    /// The world identity a recording made here carries.
    pub fn world_id(&self) -> WorldId {
        WorldId::map(Self::SLUG, self.compiled.collision_digest())
    }

    /// Insert the `maps` row, with the collision digest derived by compiling —
    /// the same rule the seed follows.
    pub async fn insert(&self, pool: &PgPool) -> i32 {
        use sha2::{Digest, Sha256};
        sqlx::query_scalar(
            "insert into maps (slug, name, author, source_sha256, source_key, collision_digest, \
                               map_compiler_version, has_start_trigger, has_finish_trigger) \
             values ($1, $2, null, $3, $4, $5, $6, true, true) returning id",
        )
        .bind(Self::SLUG)
        .bind("Fixture Course")
        .bind(Sha256::digest(self.source.as_bytes()).to_vec())
        .bind(format!("/assets/maps/{}.map", Self::SLUG))
        .bind(digest16::to_sql(self.compiled.collision_digest()))
        .bind(simbuild::map_compiler_version())
        .fetch_one(pool)
        .await
        .expect("insert fixture map")
    }

    /// A run that crosses both timing triggers.
    pub fn a_finishing_run(&self) -> Recording {
        self.record_with(&PhysicsProfile::cpm(), "cpm", self.world_id())
    }

    /// The same inputs, declared to have been made somewhere this service has
    /// never heard of.
    pub fn a_run_in_another_world(&self) -> Recording {
        // A collision digest no `maps` row carries. Recorded against the real
        // world so the file is well-formed; only its declared identity is
        // foreign, which is exactly the case §7.2 step 2 is about.
        self.record_with(
            &PhysicsProfile::cpm(),
            "cpm",
            WorldId::map("somewhere-else", 0xdead_beef_dead_beef),
        )
    }

    /// A run under physics this service deliberately has no profile row for.
    pub fn a_run_under_experimental_physics(&self) -> Recording {
        self.record_with(
            &PhysicsProfile::experimental(),
            "experimental",
            self.world_id(),
        )
    }

    fn record_with(
        &self,
        profile: &PhysicsProfile,
        name: &str,
        declared: WorldId,
    ) -> Recording {
        let world = self.compiled.collider();
        let start = RunStart {
            rate: TickRate::HZ_125,
            spawn: self.compiled.spawn,
            yaw: self.compiled.spawn_yaw,
        };

        // Hold forward, facing the way the spawn faces. On a flat floor that
        // crosses the start line and then the finish line.
        let view_yaw = angle_to_short(self.compiled.spawn_yaw);
        let commands: Vec<UserCmd> = (0..1400)
            .map(|_| {
                let mut cmd = UserCmd::still_at(TickRate::HZ_125);
                cmd.forward_move = 127;
                cmd.view.yaw = view_yaw;
                cmd
            })
            .collect();

        Recording::record(start, commands, &world, declared, profile, name)
    }
}

/// The `.map` text. Valve 220, the same dialect `assets/maps/coil.map` is in.
fn course_source() -> String {
    let floor = brush(-256.0, -512.0, -32.0, 256.0, 2048.0, 0.0, "straf3/floor");
    let start = brush(-256.0, 0.0, -32.0, 256.0, 32.0, 512.0, "common/trigger");
    let finish = brush(-256.0, 1024.0, -32.0, 256.0, 1056.0, 512.0, "common/trigger");

    format!(
        "// straf3-records test fixture: a straight line with a clock at each end.\n\
         {{\n\
         \"classname\" \"worldspawn\"\n\
         \"message\" \"Fixture Course\"\n\
         {floor}\
         }}\n\
         {{\n\
         \"classname\" \"info_player_start\"\n\
         \"origin\" \"0 -256 24\"\n\
         \"angle\" \"90\"\n\
         }}\n\
         {{\n\
         \"classname\" \"trigger_multiple\"\n\
         \"target\" \"t_start\"\n\
         {start}\
         }}\n\
         {{\n\
         \"classname\" \"trigger_multiple\"\n\
         \"target\" \"t_finish\"\n\
         {finish}\
         }}\n\
         {{\n\
         \"classname\" \"target_startTimer\"\n\
         \"targetname\" \"t_start\"\n\
         \"origin\" \"0 16 64\"\n\
         }}\n\
         {{\n\
         \"classname\" \"target_stopTimer\"\n\
         \"targetname\" \"t_finish\"\n\
         \"origin\" \"0 1040 64\"\n\
         }}\n"
    )
}

/// One axis-aligned box, six faces, in the winding `coil.map` uses.
fn brush(x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32, tex: &str) -> String {
    let uv = "[ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1";
    let uv_side = "[ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1";
    let uv_end = "[ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1";
    format!(
        "{{\n\
         ( {x0} {y0} {z0} ) ( {x1} {y0} {z0} ) ( {x1} {y1} {z0} ) {tex} {uv}\n\
         ( {x0} {y0} {z1} ) ( {x1} {y1} {z1} ) ( {x1} {y0} {z1} ) {tex} {uv}\n\
         ( {x0} {y0} {z0} ) ( {x1} {y0} {z1} ) ( {x1} {y0} {z0} ) {tex} {uv_side}\n\
         ( {x0} {y1} {z0} ) ( {x1} {y1} {z0} ) ( {x1} {y1} {z1} ) {tex} {uv_side}\n\
         ( {x0} {y0} {z0} ) ( {x0} {y1} {z1} ) ( {x0} {y0} {z1} ) {tex} {uv_end}\n\
         ( {x1} {y0} {z0} ) ( {x1} {y1} {z1} ) ( {x1} {y1} {z0} ) {tex} {uv_end}\n\
         }}\n"
    )
}

// ── driving the verifier ────────────────────────────────────────────────────

/// Do what `straf3-records-verifier` does, against the fixture course.
///
/// The same `verify_against` and the same SQL the binary runs — this exists so
/// a test does not have to spawn a second process, not so it can take a
/// shortcut through the decision.
pub async fn run_verifier(pool: &PgPool, course: &TestCourse, map_id: i32, profile_id: i32) {
    let world_id = course.world_id();

    loop {
        let mut tx = pool.begin().await.expect("begin");
        let claimed = sqlx::query(
            "select id, demo_bytes_blob from runs where status = 'pending' \
              order by submitted_at asc for update skip locked limit 1",
        )
        .fetch_optional(&mut *tx)
        .await
        .expect("claim");

        let Some(row) = claimed else {
            tx.rollback().await.ok();
            return;
        };

        use sqlx::Row as _;
        let id: Uuid = row.try_get("id").unwrap();
        let bytes: Vec<u8> = row.try_get("demo_bytes_blob").unwrap();

        let recording = Recording::from_bytes(&bytes).expect("stored bytes decode");
        let verdict = verify_against(&recording, &course.compiled, &world_id);

        sqlx::query(
            "update runs set status = $2::run_status, time_ms = $3, client_time_ms = $4, \
                    client_rolling_digest = $5, server_rolling_digest = $6, \
                    divergence_at = $7, reject_reason = $8, verified_at = now() where id = $1",
        )
        .bind(id)
        .bind(verdict.status.as_str())
        .bind(verdict.time_ms)
        .bind(verdict.client_time_ms)
        .bind(digest16::to_sql(verdict.client_rolling_digest))
        .bind(verdict.server_rolling_digest.map(digest16::to_sql))
        .bind(verdict.divergence_at)
        .bind(verdict.reject_reason.as_deref())
        .execute(&mut *tx)
        .await
        .expect("write verdict");

        if let (RunStatus::Verified, Some(time_ms)) = (verdict.status, verdict.time_ms) {
            let player_id: Uuid = sqlx::query_scalar("select player_id from runs where id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            sqlx::query(
                "insert into leaderboard_entries \
                     (map_id, profile_id, player_id, run_id, time_ms, set_at) \
                 values ($1, $2, $3, $4, $5, now()) \
                 on conflict (map_id, profile_id, player_id) do update set \
                     run_id = excluded.run_id, time_ms = excluded.time_ms, \
                     set_at = excluded.set_at \
                 where excluded.time_ms < leaderboard_entries.time_ms",
            )
            .bind(map_id)
            .bind(profile_id)
            .bind(player_id)
            .bind(id)
            .bind(time_ms)
            .execute(&mut *tx)
            .await
            .expect("upsert board");
        }

        tx.commit().await.expect("commit");
    }
}

// ── editing a `.s3d` header, the way a forger would ─────────────────────────

/// Offset of the header's `run_digest`, from the layout in
/// `straf3_replay::codec`: magic 4, version 4, flags 4, header_len 4, then
/// rate 4, count 4, sim_time 4, run_time 4, finished 1, world_tag 1,
/// spawn 12, yaw 4.
const RUN_DIGEST_OFFSET: usize = 50;
/// `run_time_ms` sits at 28.
const RUN_TIME_OFFSET: usize = 28;

/// Rewrite the stored rolling digest and re-seal the file.
pub fn forge_run_digest(bytes: &[u8], digest: u64) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[RUN_DIGEST_OFFSET..RUN_DIGEST_OFFSET + 8].copy_from_slice(&digest.to_le_bytes());
    reseal(out)
}

/// Rewrite the claimed run time and re-seal the file.
pub fn claim_a_faster_time(bytes: &[u8], time_ms: u32) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[RUN_TIME_OFFSET..RUN_TIME_OFFSET + 4].copy_from_slice(&time_ms.to_le_bytes());
    reseal(out)
}

/// Recompute the trailing content digest, so the edit passes the corruption
/// check exactly as a real forgery would. The content digest is not a security
/// boundary and `straf3-replay` says so — an attacker recomputes it trivially,
/// which is the point of this helper.
fn reseal(mut bytes: Vec<u8>) -> Vec<u8> {
    let body_len = bytes.len() - 8;
    let mut h = straf3_replay::digest::Fnv1a::new();
    h.bytes(&bytes[..body_len]);
    let content = h.finish();
    bytes[body_len..].copy_from_slice(&content.to_le_bytes());
    bytes
}

// ── a real Ed25519 key, published as a JWK ──────────────────────────────────

/// A signing key and the JWK that verifies it.
pub struct TestKeys {
    signing: SigningKey,
    kid: String,
}

impl Default for TestKeys {
    fn default() -> Self {
        Self::new()
    }
}

impl TestKeys {
    /// Deterministic, so two calls in different tests agree about the key.
    pub fn new() -> Self {
        Self {
            signing: SigningKey::from_bytes(&[7u8; 32]),
            kid: "straf3-records-test-key".to_string(),
        }
    }

    /// The key set the service would have fetched from the auth endpoint.
    ///
    /// The same shape Neon Auth publishes: `kty: OKP`, `crv: Ed25519`,
    /// `alg: EdDSA`, verified against the live endpoint before this was
    /// written.
    pub fn jwks(&self, _issuer: Option<String>) -> Jwks {
        Jwks::with_keys(self.jwk_set(), Some(ISSUER.to_string()))
    }

    fn jwk_set(&self) -> JwkSet {
        let x = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(self.signing.verifying_key().to_bytes());
        serde_json::from_value(json!({
            "keys": [{
                "kty": "OKP", "crv": "Ed25519", "alg": "EdDSA",
                "use": "sig", "kid": self.kid, "x": x,
            }]
        }))
        .expect("a well-formed OKP JWK set")
    }

    fn encoding_key(&self) -> EncodingKey {
        let der = self
            .signing
            .to_pkcs8_der()
            .expect("the test key encodes to PKCS#8");
        EncodingKey::from_ed_der(der.as_bytes())
    }

    fn header(&self, kid: &str) -> Header {
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(kid.to_string());
        header
    }

    fn sign(&self, header: &Header, claims: serde_json::Value) -> String {
        jsonwebtoken::encode(header, &claims, &self.encoding_key()).expect("sign")
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    /// A token the service should accept.
    pub fn token(&self, subject: &str, name: &str) -> String {
        self.sign(
            &self.header(&self.kid),
            json!({
                "sub": subject, "iss": ISSUER, "aud": "straf3",
                "iat": Self::now(), "exp": Self::now() + 3600,
                "name": name, "email": format!("{subject}@example.test"),
            }),
        )
    }

    /// Expired an hour ago.
    pub fn token_expired_an_hour_ago(&self, subject: &str) -> String {
        self.sign(
            &self.header(&self.kid),
            json!({
                "sub": subject, "iss": ISSUER,
                "iat": Self::now() - 7200, "exp": Self::now() - 3600, "name": "Stale",
            }),
        )
    }

    /// Correctly signed, wrong `iss`.
    pub fn token_from_another_issuer(&self, subject: &str) -> String {
        self.sign(
            &self.header(&self.kid),
            json!({
                "sub": subject, "iss": "https://not-our-auth.test",
                "iat": Self::now(), "exp": Self::now() + 3600, "name": "Elsewhere",
            }),
        )
    }

    /// Right shape, right `kid`, signature bytes flipped.
    pub fn token_with_a_broken_signature(&self, subject: &str) -> String {
        let good = self.token(subject, "Tamper");
        let (head, sig) = good.rsplit_once('.').expect("a JWT has three parts");
        let mut raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig)
            .expect("the signature is base64url");
        raw[0] ^= 0xff;
        format!(
            "{head}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
        )
    }

    /// Signed by a key the endpoint never published.
    pub fn token_from_an_unpublished_key(&self, subject: &str) -> String {
        let rogue = Self {
            signing: SigningKey::from_bytes(&[9u8; 32]),
            kid: "a-key-nobody-published".to_string(),
        };
        rogue.token(subject, "Rogue")
    }
}

// ── signed-in harnesses ─────────────────────────────────────────────────────

/// A harness that trusts the fixture key, plus a token for "Nova Tester".
///
/// Returns `(harness, token, subject)`.
pub async fn signed_in(label: &str) -> (Harness, String, String) {
    let keys = TestKeys::new();
    let harness = Harness::with_jwks(label, keys.jwks(None)).await;
    let subject = "nova-subject";
    let token = keys.token(subject, "Nova Tester");
    (harness, token, subject.to_string())
}

/// Another token the same harness trusts.
pub async fn sign_in_as(_harness: &Harness, subject: &str, name: &str) -> (String, String) {
    let keys = TestKeys::new();
    (keys.token(subject, name), subject.to_string())
}

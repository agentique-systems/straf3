//! Neon Auth bearer tokens, verified against the published JWKS.
//!
//! # This supersedes ARCHITECTURE §6 entirely
//!
//! The operator provisioned Neon Auth (Better Auth) on the Neon `straf3`
//! project, so nothing here implements OAuth providers,
//! `/auth/:provider/start`, `s3_session` cookies or CSRF machinery. The site
//! signs the player in and holds the JWT; every authenticated `/v1` call
//! carries `Authorization: Bearer <jwt>`; this module decides whether that
//! token is real and which `players` row it names.
//!
//! # Ed25519, and only Ed25519
//!
//! The JWKS at `NEON_AUTH_JWKS_URL` was fetched and read rather than assumed:
//! it serves exactly one key, `{"kty":"OKP","crv":"Ed25519","alg":"EdDSA"}`.
//! So [`Validation`] is constructed with `Algorithm::EdDSA` and never with a
//! permissive list. That matters more than it looks: an `alg` list is how a
//! verifier ends up accepting an `HS256` token signed with the *public* key it
//! published, and an `alg: none` token is the same bug with the pretence
//! removed. The header's `alg` is checked before the signature is, so a token
//! offering a different algorithm is refused with that named rather than
//! failing somewhere inside the crypto.
//!
//! # Rotation is a live path, not a hypothetical
//!
//! There is one key today. When it rotates, tokens signed by the new one arrive
//! carrying a `kid` this cache has never seen, and rejecting them outright
//! would sign every player out until a restart. So an unknown `kid` triggers a
//! re-fetch — **rate limited**, by [`config::JWKS_MIN_REFETCH_INTERVAL`], so a
//! stream of forged `kid`s cannot turn this service into a request amplifier
//! pointed at the auth endpoint.
//!
//! # The attribution rule r10 rests on
//!
//! The subject comes out of the *verified* token and from nowhere else. No
//! request body, header or query parameter can name a player. That is why
//! [`authenticate`] returns the `players` row rather than a claim about one,
//! and why every handler that writes takes an [`AuthedPlayer`] by value.

use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use jsonwebtoken::jwk::{AlgorithmParameters, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use sqlx::{PgPool, Row};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config;
use crate::error::{ApiError, ApiResult};

/// A player the service is willing to attribute a time to.
///
/// Constructed only by [`authenticate`], only from a verified token.
#[derive(Debug, Clone)]
pub struct AuthedPlayer {
    /// `players.id`.
    pub id: Uuid,
    /// What a board shows.
    pub display_name: String,
    /// The Neon Auth `sub`.
    pub subject: String,
}

/// The claims this service reads. Everything else in the token is ignored.
#[derive(Debug, Deserialize)]
pub struct Claims {
    /// The subject — a Neon Auth user id.
    pub sub: String,
    /// Expiry. `Validation` enforces it; it is deserialized so a handler can
    /// say when.
    #[serde(default)]
    pub exp: Option<i64>,
    /// The issuer, when the token carries one.
    #[serde(default)]
    pub iss: Option<String>,
    /// A display name, when Better Auth put one in.
    #[serde(default)]
    pub name: Option<String>,
    /// An email, stored on `identities` for the operator's benefit only.
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Default)]
struct Cached {
    keys: Option<JwkSet>,
    fetched_at: Option<Instant>,
    last_attempt: Option<Instant>,
}

/// The JWKS, cached with a bounded TTL and re-fetched on rotation.
pub struct Jwks {
    url: Option<String>,
    issuer: Option<String>,
    http: reqwest::Client,
    cache: RwLock<Cached>,
}

impl std::fmt::Debug for Jwks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the URL: it is a credential-adjacent value from `.env`.
        f.debug_struct("Jwks")
            .field("configured", &self.url.is_some())
            .field("issuer_pinned", &self.issuer.is_some())
            .finish()
    }
}

impl Jwks {
    /// Build the cache. Fetches nothing yet.
    #[must_use]
    pub fn new(url: Option<String>, issuer: Option<String>) -> Self {
        Self {
            url,
            issuer,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            cache: RwLock::new(Cached::default()),
        }
    }

    /// A cache pre-loaded with a key set and no endpoint behind it.
    ///
    /// For tests that need the *verification* path — signature, `alg`, `exp`,
    /// `iss` — without a network. It cannot re-fetch, so it also pins down the
    /// unknown-`kid` behaviour: an unknown key id has nowhere to go and the
    /// token is refused rather than accepted.
    #[must_use]
    pub fn with_keys(keys: JwkSet, issuer: Option<String>) -> Self {
        let jwks = Self::new(None, issuer);
        {
            // `try_write` on a freshly constructed lock cannot fail.
            let mut cache = jwks.cache.try_write().expect("a new lock is uncontended");
            cache.keys = Some(keys);
            cache.fetched_at = Some(Instant::now());
        }
        jwks
    }

    /// Whether a JWKS endpoint is configured at all.
    #[must_use]
    pub const fn is_configured(&self) -> bool {
        self.url.is_some()
    }

    /// Warm the cache, so the first signed-in request does not pay for it.
    ///
    /// # Errors
    ///
    /// Whatever the fetch said. Startup logs it and carries on — a records
    /// service that refuses to boot because the auth endpoint is briefly down
    /// would take the anonymous half of the site with it.
    pub async fn warm(&self) -> Result<usize, String> {
        let set = self.fetch().await?;
        let count = set.keys.len();
        let mut cache = self.cache.write().await;
        cache.keys = Some(set);
        cache.fetched_at = Some(Instant::now());
        Ok(count)
    }

    async fn fetch(&self) -> Result<JwkSet, String> {
        let url = self
            .url
            .as_deref()
            .ok_or_else(|| "no JWKS endpoint is configured".to_string())?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            // The URL is deliberately not in the message: it comes from `.env`.
            .map_err(|e| format!("the JWKS endpoint could not be reached: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "the JWKS endpoint answered {}",
                response.status().as_u16()
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("the JWKS response could not be read: {e}"))?;
        serde_json::from_slice::<JwkSet>(&bytes)
            .map_err(|e| format!("the JWKS response is not a JWK set: {e}"))
    }

    /// The decoding key for `kid`, re-fetching once if it is unknown.
    async fn key_for(&self, kid: &str) -> ApiResult<DecodingKey> {
        if let Some(key) = self.cached_key(kid).await? {
            return Ok(key);
        }

        // Nowhere to re-fetch from. Say what is actually wrong — the key id is
        // not one the service holds — rather than reporting a network failure
        // for a request it was never going to make.
        if self.url.is_none() {
            return Err(ApiError::not_authenticated(format!(
                "the token names signing key `{kid}`, which is not one this service holds."
            )));
        }

        // Unknown `kid`, or a stale cache. Re-fetch, but not more often than
        // the floor: a forged `kid` must not become an amplifier.
        {
            let mut cache = self.cache.write().await;
            let too_soon = cache
                .last_attempt
                .is_some_and(|at| at.elapsed() < config::JWKS_MIN_REFETCH_INTERVAL);
            if too_soon {
                return Err(ApiError::not_authenticated(format!(
                    "the token names signing key `{kid}`, which is not in the cached key set."
                )));
            }
            cache.last_attempt = Some(Instant::now());
        }

        let set = self
            .fetch()
            .await
            .map_err(|e| ApiError::not_authenticated(format!("token not verified: {e}")))?;
        {
            let mut cache = self.cache.write().await;
            cache.keys = Some(set);
            cache.fetched_at = Some(Instant::now());
        }

        self.cached_key(kid).await?.ok_or_else(|| {
            ApiError::not_authenticated(format!(
                "the token names signing key `{kid}`, which the auth endpoint does not publish."
            ))
        })
    }

    async fn cached_key(&self, kid: &str) -> ApiResult<Option<DecodingKey>> {
        let cache = self.cache.read().await;
        let fresh = cache
            .fetched_at
            .is_some_and(|at| at.elapsed() < config::JWKS_TTL);
        if !fresh {
            return Ok(None);
        }
        let Some(jwk) = cache.keys.as_ref().and_then(|set| set.find(kid)) else {
            return Ok(None);
        };
        // The published key is OKP/Ed25519. Anything else is a key type this
        // service will not verify with, and saying so beats a signature failure
        // three layers down.
        match &jwk.algorithm {
            AlgorithmParameters::OctetKeyPair(_) => {
                DecodingKey::from_jwk(jwk).map(Some).map_err(|e| {
                    ApiError::not_authenticated(format!(
                        "signing key `{kid}` could not be read: {e}"
                    ))
                })
            }
            other => Err(ApiError::not_authenticated(format!(
                "signing key `{kid}` is a {} key; this service verifies Ed25519 (OKP) only.",
                key_type_name(other)
            ))),
        }
    }

    /// Verify a bearer token and return its claims.
    ///
    /// # Errors
    ///
    /// [`ApiError::not_authenticated`], with a `detail` that names which of
    /// signature, `alg`, `exp` or `iss` was the problem.
    pub async fn verify(&self, token: &str) -> ApiResult<Claims> {
        // No endpoint *and* no keys means nothing here can verify anything.
        // Refusing is the honest answer; accepting an unverified token because
        // the service cannot check it is not.
        if self.url.is_none() && self.cache.read().await.keys.is_none() {
            return Err(ApiError::not_authenticated(
                "this service has no auth endpoint configured, so it cannot verify a token. \
                 Set NEON_AUTH_JWKS_URL."
                    .to_string(),
            ));
        }

        let header = decode_header(token).map_err(|e| {
            ApiError::not_authenticated(format!("the bearer token is not a JWT: {e}"))
        })?;

        // Checked before the signature so the refusal names the real cause. An
        // `alg` this service does not implement is the shape of both the
        // `alg: none` attack and the "verify an HS256 token with the published
        // public key" attack.
        if header.alg != Algorithm::EdDSA {
            return Err(ApiError::not_authenticated(format!(
                "the token is signed with {:?}; this service accepts EdDSA (Ed25519) only.",
                header.alg
            )));
        }

        let kid = header.kid.ok_or_else(|| {
            ApiError::not_authenticated("the token carries no `kid`, so no key can be chosen.")
        })?;
        let key = self.key_for(&kid).await?;

        let mut validation = Validation::new(Algorithm::EdDSA);
        debug_assert_eq!(validation.algorithms, vec![Algorithm::EdDSA]);
        validation.validate_exp = true;
        // Better Auth sets `aud`, but there is no second audience to confuse
        // this service with on a one-origin deployment, and requiring one we
        // have not been told would refuse every real token.
        validation.validate_aud = false;
        validation.set_required_spec_claims(&["exp", "sub"]);
        if let Some(issuer) = &self.issuer {
            validation.set_issuer(&[issuer]);
        }

        decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|e| {
                use jsonwebtoken::errors::ErrorKind;
                let detail = match e.kind() {
                    ErrorKind::ExpiredSignature => {
                        "the token has expired; sign in again.".to_string()
                    }
                    ErrorKind::InvalidIssuer => {
                        "the token was issued by a different authority than this service trusts."
                            .to_string()
                    }
                    ErrorKind::InvalidSignature => {
                        "the token's signature does not verify against the published key."
                            .to_string()
                    }
                    ErrorKind::MissingRequiredClaim(claim) => {
                        format!("the token is missing the `{claim}` claim.")
                    }
                    other => format!("the token did not verify: {other:?}"),
                };
                ApiError::not_authenticated(detail)
            })
    }
}

fn key_type_name(params: &AlgorithmParameters) -> &'static str {
    match params {
        AlgorithmParameters::EllipticCurve(_) => "EC",
        AlgorithmParameters::RSA(_) => "RSA",
        AlgorithmParameters::OctetKey(_) => "oct",
        AlgorithmParameters::OctetKeyPair(_) => "OKP",
    }
}

/// Pull the bearer token out of the request headers.
///
/// # Errors
///
/// [`ApiError::not_authenticated`] when there is no `Authorization: Bearer`.
pub fn bearer_token(headers: &HeaderMap) -> ApiResult<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or_else(|| {
            ApiError::not_authenticated(
                "this route needs a signed-in player; send `Authorization: Bearer <token>`."
                    .to_string(),
            )
        })?
        .to_str()
        .map_err(|_| {
            ApiError::not_authenticated("the Authorization header is not text.".to_string())
        })?;

    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            ApiError::not_authenticated(
                "the Authorization header is not a bearer token.".to_string(),
            )
        })
}

/// Verify the request's token and resolve it to a `players` row, creating one
/// on first sight.
///
/// # Errors
///
/// [`ApiError::not_authenticated`] for anything about the token; a database
/// error otherwise.
pub async fn authenticate(
    pool: &PgPool,
    jwks: &Jwks,
    headers: &HeaderMap,
) -> ApiResult<AuthedPlayer> {
    let token = bearer_token(headers)?;
    let claims = jwks.verify(token).await?;
    player_for_subject(pool, &claims).await
}

/// Find or create the player behind a verified subject.
///
/// # Errors
///
/// A database failure, or a banned player.
pub async fn player_for_subject(pool: &PgPool, claims: &Claims) -> ApiResult<AuthedPlayer> {
    if let Some(found) = lookup_subject(pool, &claims.sub).await? {
        return Ok(found);
    }

    // First sight. Everything about the new row comes from the verified token.
    let id = Uuid::new_v4();
    let display_name = allocate_display_name(pool, claims, id).await?;

    let mut tx = pool.begin().await?;
    sqlx::query("insert into players (id, display_name) values ($1, $2)")
        .bind(id)
        .bind(&display_name)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "insert into identities (provider, provider_user_id, player_id, handle, email) \
         values ('neon-auth', $1, $2, $3, $4) on conflict (provider, provider_user_id) do nothing",
    )
    .bind(&claims.sub)
    .bind(id)
    .bind(claims.name.as_deref())
    .bind(claims.email.as_deref())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // Two concurrent first sights of the same subject both insert a player;
    // the identity insert is what decides which one wins, so read it back
    // rather than assuming this request's row won.
    lookup_subject(pool, &claims.sub)
        .await?
        .ok_or_else(|| ApiError::internal("the player row vanished immediately after insert"))
}

async fn lookup_subject(pool: &PgPool, subject: &str) -> ApiResult<Option<AuthedPlayer>> {
    let row = sqlx::query(
        "select p.id, p.display_name, p.banned_at, p.ban_reason \
         from identities i join players p on p.id = i.player_id \
         where i.provider = 'neon-auth' and i.provider_user_id = $1",
    )
    .bind(subject)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else { return Ok(None) };

    let banned_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("banned_at")?;
    if banned_at.is_some() {
        let reason: Option<String> = row.try_get("ban_reason")?;
        return Err(ApiError {
            status: axum::http::StatusCode::FORBIDDEN,
            code: "player_banned",
            detail: reason.unwrap_or_else(|| "this account cannot submit runs.".to_string()),
        });
    }

    Ok(Some(AuthedPlayer {
        id: row.try_get("id")?,
        display_name: row.try_get("display_name")?,
        subject: subject.to_string(),
    }))
}

/// Pick a display name that is free.
///
/// `players` is unique on `lower(display_name)` (§5.1), so a second player
/// called `nova` needs a different name rather than an error the site cannot
/// act on. The last candidate is unique by construction, so this terminates.
async fn allocate_display_name(pool: &PgPool, claims: &Claims, id: Uuid) -> ApiResult<String> {
    let base = claims
        .name
        .as_deref()
        .map(sanitize_display_name)
        .filter(|n| !n.is_empty())
        .or_else(|| {
            claims
                .email
                .as_deref()
                .and_then(|e| e.split('@').next())
                .map(sanitize_display_name)
                .filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| format!("player-{}", &id.simple().to_string()[..8]));

    for suffix in 0..64u32 {
        let candidate = if suffix == 0 {
            base.clone()
        } else {
            let stem: String = base.chars().take(28).collect();
            format!("{stem}-{suffix}")
        };
        let taken: Option<i32> =
            sqlx::query_scalar("select 1 from players where lower(display_name) = lower($1)")
                .bind(&candidate)
                .fetch_optional(pool)
                .await?;
        if taken.is_none() {
            return Ok(candidate);
        }
    }

    Ok(format!("player-{}", id.simple()))
}

/// Trim a token-supplied name down to something a board and a URL can carry.
///
/// Deliberately conservative: this string ends up in `GET /v1/players/:name`
/// and on every leaderboard row, and a name containing a slash or a control
/// character is a name that breaks one of them.
#[must_use]
pub fn sanitize_display_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim_matches(['-', '.', ' '])
        .chars()
        .take(32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with(auth: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_str(auth).unwrap(),
        );
        headers
    }

    #[test]
    fn a_bearer_token_is_read_and_nothing_else_is() {
        assert_eq!(
            bearer_token(&headers_with("Bearer abc.def.ghi")).unwrap(),
            "abc.def.ghi"
        );
        assert_eq!(bearer_token(&headers_with("bearer abc")).unwrap(), "abc");
        assert!(bearer_token(&HeaderMap::new()).is_err());
        assert!(bearer_token(&headers_with("Basic dXNlcjpwdw==")).is_err());
        assert!(bearer_token(&headers_with("Bearer ")).is_err());
    }

    #[tokio::test]
    async fn a_token_signed_with_anything_but_ed25519_is_refused_by_algorithm() {
        // The `alg: none` and "HS256 against the published public key" attacks
        // both arrive as a header this service must refuse *before* it reaches
        // the signature check. No JWKS is needed to prove that, which is the
        // point — the refusal happens above the key lookup.
        let jwks = Jwks::new(Some("https://example.invalid/jwks".to_string()), None);

        // header {"alg":"none","typ":"JWT"} . payload {"sub":"x"} . (no sig)
        let none_alg = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiJ4In0.";
        let err = jwks.verify(none_alg).await.unwrap_err();
        assert_eq!(err.code, "not_authenticated");
        assert!(
            err.detail.contains("EdDSA"),
            "the refusal should name the algorithm: {}",
            err.detail
        );

        // header {"alg":"HS256","typ":"JWT"}
        let hs256 = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ4In0.c2ln";
        let err = jwks.verify(hs256).await.unwrap_err();
        assert!(err.detail.contains("HS256"), "{}", err.detail);
    }

    #[tokio::test]
    async fn with_no_endpoint_configured_every_token_is_refused() {
        // Not "allowed through because we cannot check". The honest failure.
        let jwks = Jwks::new(None, None);
        let err = jwks.verify("eyJhbGciOiJFZERTQSJ9.e30.x").await.unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn display_names_cannot_break_a_url_or_a_board_row() {
        assert_eq!(sanitize_display_name("Nova"), "Nova");
        assert_eq!(sanitize_display_name("  spaced   out  "), "spaced out");
        assert_eq!(sanitize_display_name("a/b\\c?d#e"), "abcde");
        assert_eq!(sanitize_display_name("\u{7}\u{0}ctrl"), "ctrl");
        assert_eq!(sanitize_display_name("-.-"), "");
        assert_eq!(sanitize_display_name(&"x".repeat(64)).len(), 32);
    }
}

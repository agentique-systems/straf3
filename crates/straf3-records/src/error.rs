//! The one failure envelope, and the codes the site renders differently.
//!
//! # Why this file matters more than its size suggests
//!
//! `web/site/app/api.js` distinguishes four outcomes, and the reason it can is
//! that this service never flattens two of them together. Requirement r9 is
//! precisely that distinction:
//!
//! - **A board with nobody on it** is a `200` whose body says so —
//!   `{"category": …, "entries": [], "total": 0}`. Never a bare `[]`, never a
//!   `204`, never a `404`.
//! - **A board the service could not answer for** is a non-2xx carrying
//!   `{"error": "<snake_case_code>", "detail": "<a sentence a person can act
//!   on>"}`.
//!
//! An empty array returned because Postgres was unreachable would make those
//! two indistinguishable at the only place the difference can still be
//! detected, which is why every read path here propagates a database failure
//! rather than defaulting to an empty result.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// A refusal, in the shape the site already parses.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// The HTTP status. Always non-2xx.
    pub status: StatusCode,
    /// The `snake_case` code. Stable; the site branches on it.
    pub code: &'static str,
    /// A sentence a person can act on. Never contains a credential.
    pub detail: String,
}

/// The result type every handler returns.
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            status,
            code,
            detail: detail.into(),
        }
    }

    /// No `maps` row for that slug.
    pub fn unknown_map(slug: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "unknown_map",
            format!("no map is named `{slug}`."),
        )
    }

    /// A pinned `@digest16` with no `physics_profiles` row.
    ///
    /// Not an empty board and not the current one. URLS.md §3: silently
    /// substituting the current profile is the failure ARCHITECTURE §7.2 step 2
    /// forbids the verifier from making, and the API does not get to make it
    /// either.
    pub fn unknown_physics_digest(digest: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "unknown_physics_digest",
            format!(
                "no physics profile has digest `{digest}`. This board is pinned to constants this \
                 service has never seen, so it is unknown — not empty, and not the current board."
            ),
        )
    }

    /// A category family that is not a `physics_profiles.kind` here.
    pub fn unknown_physics_family(family: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "unknown_physics_family",
            format!("no physics profile family is called `{family}`."),
        )
    }

    /// A `<family>[@<digest16>]` that does not parse.
    pub fn invalid_category(spec: &str) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_category",
            format!(
                "`{spec}` is not a category key. The grammar is `<family>` or \
                 `<family>@<digest16>`, all lowercase (URLS.md §2)."
            ),
        )
    }

    /// No run by that uuid or digest.
    pub fn unknown_run(name: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "unknown_run",
            format!("no run is named `{name}`."),
        )
    }

    /// `POST /v1/runs` without an `X-Straf3-Ticket`.
    pub fn missing_ticket() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "missing_ticket",
            "a submission must carry the `X-Straf3-Ticket` from `POST /v1/attempts`."
                .to_string(),
        )
    }

    /// A ticket that is expired, already consumed, someone else's, or for a
    /// different category than the recording claims.
    pub fn bad_ticket(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "bad_ticket", detail)
    }

    /// No bearer token, or one that did not verify.
    pub fn not_authenticated(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "not_authenticated", detail)
    }

    /// The global unique index on `runs.run_digest` says another player got
    /// there first (§7.2 step 3, §8.3).
    pub fn run_already_submitted(digest: &str) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "run_already_submitted",
            format!(
                "run `{digest}` has already been submitted by another player. A run belongs to \
                 whoever submitted it first."
            ),
        )
    }

    /// The service is up and Postgres is not.
    pub fn database_unavailable(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            detail,
        )
    }

    /// The submitted body broke one of ARCHITECTURE §7.3's bounds.
    pub fn payload_too_large(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "demo_too_large", detail)
    }

    /// The bytes are not a `.s3d` this build can read.
    pub fn malformed_demo(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "malformed_demo", detail)
    }

    /// The recording names a world or physics identity this service cannot
    /// honour. Refused with the mismatch named — never ranked under the
    /// nearest profile (ARCHITECTURE §7.2 step 2, §7.4).
    pub fn unhonourable_recording(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, detail)
    }

    /// Too many submissions or tickets, per §7.3.
    pub fn rate_limited(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", detail)
    }

    /// A bug here, not a condition the caller can fix.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            detail,
        )
    }

    /// A malformed request that is none of the above.
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", detail)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}): {}", self.code, self.status, self.detail)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": self.code, "detail": self.detail })),
        )
            .into_response()
    }
}

/// Classify a `sqlx` failure into "Postgres is not there" versus "this service
/// has a bug".
///
/// Both are non-2xx with the envelope, so the site renders both as
/// *unanswerable* — but they are not the same thing and saying so keeps the
/// `503` honest. A `503` that also covered a broken query would make
/// `GET /v1/health` meaningless.
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::Protocol(_)
            | sqlx::Error::Configuration(_) => Self::database_unavailable(format!(
                "the records database did not answer: {error}"
            )),
            _ => Self::internal(format!("database error: {error}")),
        }
    }
}

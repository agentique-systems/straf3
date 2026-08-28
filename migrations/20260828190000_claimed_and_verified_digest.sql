-- Split the run digest in two, and move uniqueness onto the half this service
-- computed for itself.
--
-- # The hole this closes
--
-- The first migration put a single global unique index on `run_digest`, exactly
-- as ARCHITECTURE §5.1 specifies, and §8.3 rests first-submitter ownership on
-- it. That works only if the indexed value is derived by the server. It is not,
-- and it cannot be: the rolling digest is a fold over every command's simulated
-- state (§3.2), so computing it *is* the verification, and §7.2 step 2 requires
-- intake to decode without simulating. At intake the digest can only be **read
-- out of the file's header**, which is a value the submitter chose.
--
-- That makes a plain global unique index a weapon rather than a protection.
-- Anyone can take the digest of a run they did not perform, put it in the header
-- of any file at all, and submit it. Their row is nonsense and will never rank —
-- but it owns the digest, and the player who actually performs that run is
-- refused with `409 run_already_submitted` forever. A squat, costing one
-- request, against a specific record.
--
-- # The shape that fixes it
--
-- Two columns, because they are two different claims:
--
--   * `claimed_digest` — what the submitted file said. Never rewritten, so the
--     evidence of what was claimed survives the verdict.
--   * `verified_digest` — what this service folded from the commands itself.
--     Null until it has re-simulated the run and agreed.
--
-- Uniqueness moves to a partial index on `verified_digest`, which only exists
-- for runs this service actually re-simulated. So:
--
--   * a run that never verifies never owns a digest, and cannot block anyone;
--   * two players who submit the same genuine run collide at *verification*, on
--     a number neither of them supplied, and the first one through owns it;
--   * `GET /v1/runs/by-digest/:digest16` resolves against `verified_digest`
--     alone, so a squatted digest resolves to nothing rather than to garbage.
--
-- This supersedes the disclosure in the previous migration's header, which
-- described the weaker property that shape could offer. It is a new file rather
-- than an edit because the first migration is applied and `_sqlx_migrations`
-- holds its checksum.
--
-- Intake idempotency is now a separate question from ownership, and is answered
-- separately: the same player re-uploading the same run — retried, or re-encoded
-- from the compact form into the traced one — matches on `(player_id,
-- claimed_digest)` and gets the original row back with a `200`, per §7.2 step 3.

alter table runs rename column run_digest to claimed_digest;
alter table runs add column verified_digest bigint;

-- The global unique index §5.1 asked for, dropped for the reason above. What
-- replaces it is not weaker: it is unique on a number the server derived rather
-- than on one the client supplied.
drop index runs_run_digest_key;

-- Still indexed: intake looks a player's own prior submissions up by it.
create index runs_claimed_digest_idx on runs (claimed_digest);

-- Ownership. Partial, so an unranked run owns nothing.
create unique index runs_verified_digest_key
    on runs (verified_digest) where verified_digest is not null;

-- Intake idempotency, per player. Not unique: two concurrent uploads of the same
-- run by the same player may both insert, and verification resolves it — one is
-- verified, the other is rejected as a duplicate of it.
create index runs_player_claimed_digest_idx on runs (player_id, claimed_digest);

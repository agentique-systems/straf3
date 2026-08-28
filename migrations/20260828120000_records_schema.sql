-- The records service's schema. ARCHITECTURE §5.1, in `public`.
--
-- `neon_auth` is Better Auth's and is never touched from here: nothing below
-- creates, alters or reads across into it. A player is bound to a Neon Auth
-- subject through `identities`, which stores the subject the service read out
-- of a *verified* token — not a join into someone else's tables.
--
-- Three deliberate departures from §5.1, each a scope decision recorded in the
-- wave contracts rather than an oversight:
--
--   1. There is no object store on a one-local-origin target, so the `.s3d`
--      bytes live in `runs.demo_bytes_blob`. §5.1's `demo_key` stays, nullable
--      and unused, so the column is there the day an object store is.
--   2. `runs.run_digest` is `bigint`, not `bytea`. §5.1 wrote `bytea` for a
--      `canonical_digest()` that was going to be a wide hash; what
--      `straf3-replay` actually folds, and what `docs/web/URLS.md` §5 fixes as
--      the `<digest16>` in a `/watch/` link, is a `u64` — the same width as
--      `physics_profiles.digest` and `sim_builds.build_hash`, which §5.1
--      already stores as `bigint`. Postgres has no unsigned integer, so the
--      value is stored as the two's-complement reinterpretation of the u64 and
--      is only ever rendered as 16 lowercase hex digits.
--   3. `identities` carries one row per Neon Auth subject. The table shape is
--      §5.1's unchanged; `provider` is always 'neon-auth' and
--      `provider_user_id` is the token's `sub`. Neon Auth supersedes
--      ARCHITECTURE §6, so no provider column ever holds 'discord'.

-- Identity ------------------------------------------------------------------

create table players (
    id           uuid primary key,
    display_name text        not null,
    created_at   timestamptz not null default now(),
    country      char(2),
    banned_at    timestamptz,
    ban_reason   text
);
create unique index players_display_name_lower_key on players (lower(display_name));

-- One row per Neon Auth subject. `provider` is retained from §5.1 so a second
-- identity source is an insert rather than a migration.
create table identities (
    provider         text not null,
    provider_user_id text not null,
    player_id        uuid not null references players (id),
    handle           text,
    avatar_url       text,
    email            text,
    linked_at        timestamptz not null default now(),
    primary key (provider, provider_user_id)
);
create index identities_player_id_idx on identities (player_id);

-- Immutable physics facts ---------------------------------------------------

-- Every row is immutable. Tuning CPM inserts a row; it never updates one.
-- That is what makes `/m/coil/cpm@<digest16>` mean the same board forever
-- while `/m/coil/cpm` moves (§5.4, URLS.md §3), and the trigger below is what
-- makes "immutable" a fact about the database rather than a habit of the code.
create table physics_profiles (
    id             int primary key generated always as identity,
    kind           text        not null check (kind in ('vq3', 'cpm')),
    label          text        not null,
    digest         bigint      not null unique,
    profile_bits   bytea       not null,
    layout_version smallint    not null,
    created_at     timestamptz not null default now()
);
create index physics_profiles_kind_created_idx on physics_profiles (kind, created_at desc);

create function physics_profiles_are_immutable() returns trigger as $$
begin
    raise exception
        'physics_profiles rows are immutable (ARCHITECTURE §5.4): tuning inserts a row, it never updates one';
end;
$$ language plpgsql;

create trigger physics_profiles_no_update
    before update or delete on physics_profiles
    for each row execute function physics_profiles_are_immutable();

create table sim_builds (
    id                 int primary key generated always as identity,
    sim_version        text        not null,
    git_sha            text        not null,
    build_hash         bigint      not null unique,
    -- Set from actually running `straf3_replay::crosstarget`, never asserted.
    native_verifier_ok boolean     not null default false,
    -- The artifact the browser is served. Null here on purpose: this service
    -- does not build the wasm bundle and will not invent a hash for it.
    wasm_hash          bigint,
    retired_at         timestamptz,
    created_at         timestamptz not null default now()
);

create table maps (
    id                   int primary key generated always as identity,
    slug                 text        not null unique,
    name                 text        not null,
    author               text,
    source_sha256        bytea       not null,
    -- No object store: this is the path the one origin serves the `.map` from.
    source_key           text        not null,
    collision_digest     bigint      not null,
    map_compiler_version text        not null,
    has_start_trigger    boolean     not null,
    has_finish_trigger   boolean     not null,
    added_at             timestamptz not null default now()
);
create unique index maps_source_collision_compiler_key
    on maps (source_sha256, collision_digest, map_compiler_version);
create index maps_collision_digest_idx on maps (collision_digest);

-- Runs ----------------------------------------------------------------------

create type run_status as enum
    ('pending', 'verified', 'did_not_finish', 'rejected', 'divergent', 'error');

-- One live attempt per ticket. Issued on request, consumed by a submission.
-- The ticket *is* `id`: a v4 uuid is 122 bits from the OS CSPRNG, and it is
-- checked against the authenticated player, so holding someone else's ticket
-- buys nothing even if one could be guessed.
create table attempts (
    id          uuid primary key,
    player_id   uuid not null references players (id),
    map_id      int  not null references maps (id),
    profile_id  int  not null references physics_profiles (id),
    issued_at   timestamptz not null default now(),
    expires_at  timestamptz not null,
    consumed_at timestamptz,
    consumed_by uuid
);
create index attempts_player_issued_idx on attempts (player_id, issued_at desc);
create index attempts_live_idx on attempts (expires_at) where consumed_at is null;

-- Append-only. A run is never edited after verification; a re-verification
-- under different physics creates a new row (§5.4).
create table runs (
    id           uuid primary key,
    player_id    uuid     not null references players (id),
    map_id       int      not null references maps (id),
    profile_id   int      not null references physics_profiles (id),
    sim_build_id int      not null references sim_builds (id),
    tick_rate_hz smallint not null,

    status   run_status not null default 'pending',
    -- SERVER-COMPUTED. Null unless the verifier re-simulated the run and it
    -- crossed both timing triggers. The client's number never lands here.
    time_ms  integer,
    commands integer not null,

    demo_sha256      bytea   not null,
    -- The rolling digest folded over every command (§3.2, §1.3). Global unique
    -- index below; this is the identity of the run and the `<digest16>` a
    -- `/watch/` or `/r/` link carries.
    run_digest       bigint  not null,
    -- Object-store key; null everywhere this wave, see the header.
    demo_key         text,
    demo_bytes_blob  bytea   not null,
    demo_bytes       integer not null,
    attempt_id       uuid references attempts (id),

    -- Diagnostics. `client_time_ms` is what the recording claimed; it is never
    -- ranked, never compared against, and never shown as authoritative.
    client_time_ms        integer,
    client_rolling_digest bigint,
    server_rolling_digest bigint,
    divergence_at         integer,

    submitted_at  timestamptz not null default now(),
    verified_at   timestamptz,
    reject_reason text
);
create index runs_board_idx on runs (map_id, profile_id, time_ms) where status = 'verified';
create index runs_player_submitted_idx on runs (player_id, submitted_at desc);

-- GLOBAL, not per-player. This is the constraint that makes a run belong to
-- whoever submitted it first (§8.3). A per-player index here would be an
-- idempotency key and nothing more, and would let anyone re-post a demo they
-- downloaded from the leaderboard and have it rank as their own.
create unique index runs_run_digest_key on runs (run_digest);
create index runs_demo_sha256_idx on runs (demo_sha256);
create index runs_pending_idx on runs (submitted_at) where status = 'pending';

alter table attempts
    add constraint attempts_consumed_by_fkey foreign key (consumed_by) references runs (id);

-- Current personal best per category. Derived; rebuildable from `runs` alone.
create table leaderboard_entries (
    map_id     int  not null references maps (id),
    profile_id int  not null references physics_profiles (id),
    player_id  uuid not null references players (id),
    run_id     uuid not null references runs (id),
    time_ms    integer     not null,
    set_at     timestamptz not null,
    primary key (map_id, profile_id, player_id)
);
create index leaderboard_entries_rank_idx
    on leaderboard_entries (map_id, profile_id, time_ms, set_at);

-- Records that ever held first place, so history survives being beaten.
create table record_history (
    map_id     int  not null references maps (id),
    profile_id int  not null references physics_profiles (id),
    run_id     uuid not null references runs (id),
    time_ms    integer     not null,
    held_from  timestamptz not null,
    held_until timestamptz,
    primary key (map_id, profile_id, run_id)
);
create index record_history_current_idx
    on record_history (map_id, profile_id, held_from desc);

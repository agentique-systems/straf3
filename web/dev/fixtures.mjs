// @ts-check
/**
 * Canned `/v1` answers, for looking at the site's four kinds of nothing.
 *
 * **This is not the records service and must never be mistaken for it.** It
 * exists for one reason: requirement r9 says the site must render "nobody has
 * set a time here yet", "the records service could not answer", and a populated
 * board as three visibly different things — and a fourth, a pinned physics
 * digest the service does not know, rendering as *unknown*. Three of those four
 * cannot be produced from a correctly-working seeded database, because a
 * correctly-working seeded database has no failures in it and no times in it.
 *
 * So the four states are produced here, on demand, and the *real* page code
 * renders them. That matters more than it sounds: a gallery page that drew four
 * example boxes would prove the boxes exist, not that the leaderboard route
 * reaches them. Every response below goes through `web/site/app/api.js` and
 * `web/site/app/board.js` exactly as a real one would.
 *
 * Every value here is **fabricated**. The digests are not derived from any
 * `PhysicsProfile` and the times were not set by anyone; they are shaped like
 * the real thing so the renderer is exercised, and they are deliberately not
 * plausible as records. Nothing in this file is ever served by the real API,
 * and `--fixtures` prints a banner saying so.
 *
 * The routes that produce each of the four states:
 *
 *   /m/coil/cpm                          a populated board
 *   /m/coil/vq3                          200 with entries: [] — nobody has set a time
 *   /m/coil/cpm@ffffffffffffffff         404 unknown_physics_digest — unknown, not empty
 *   /m/void/cpm                          503 database_unavailable — could not answer
 *   /m/coil/nope                         404 unknown_physics_family
 *
 * and starting the dev server with `--no-api` gives the fifth flavour of "could
 * not answer": 503 `no_records_service`.
 */

/** Fabricated. Not a `PhysicsProfile::digest()` of anything. */
const CPM_DIGEST = 'c0ffee11c0ffee11';
const VQ3_DIGEST = 'decafbad0decafba';
const COLLISION_DIGEST = 'a11ce5a11ce5a11c';

const CPM = { family: 'cpm', digest: CPM_DIGEST, label: 'CPM (fixture)', key: `cpm@${CPM_DIGEST}` };
const VQ3 = { family: 'vq3', digest: VQ3_DIGEST, label: 'VQ3 (fixture)', key: `vq3@${VQ3_DIGEST}` };

const RUN_DIGEST = '0123456789abcdef';

const ENTRIES = [
  { rank: 1, player: 'nova', time_ms: 24_318, set_at: '2026-08-21T19:04:11Z',
    run_id: '11111111-1111-4111-8111-111111111111', run_digest: RUN_DIGEST, watch: `/watch/${RUN_DIGEST}` },
  { rank: 2, player: 'kestrel', time_ms: 24_902, set_at: '2026-08-19T08:55:02Z',
    run_id: '22222222-2222-4222-8222-222222222222', run_digest: 'fedcba9876543210', watch: '/watch/fedcba9876543210' },
  // A tie. `rank() over (order by time_ms, set_at)` gives ties the same rank,
  // and the site must render them as tied rather than inventing an order.
  { rank: 3, player: 'aster', time_ms: 25_555, set_at: '2026-08-11T12:00:00Z',
    run_id: '33333333-3333-4333-8333-333333333333', run_digest: '00ff00ff00ff00ff', watch: '/watch/00ff00ff00ff00ff' },
  { rank: 3, player: 'quill', time_ms: 25_555, set_at: '2026-08-12T12:00:00Z',
    run_id: '44444444-4444-4444-8444-444444444444', run_digest: '1234abcd1234abcd', watch: '/watch/1234abcd1234abcd' },
];

const COIL = {
  slug: 'coil',
  name: 'coil',
  author: null,
  collision_digest: COLLISION_DIGEST,
  source_sha256: 'f'.repeat(64),
  source_url: '/assets/maps/coil.map',
  map_compiler_version: 'straf3-map 0.1.0 (fixture)',
  has_start_trigger: true,
  has_finish_trigger: true,
  has_timing: true,
  play: '/play/coil',
};

/** @param {string} error @param {string} detail */
const err = (error, detail) => ({ error, detail });

/**
 * Answer one `/v1` request.
 *
 * @param {URL} url
 * @returns {{status: number, body: unknown}}
 */
export function answer(url) {
  const path = url.pathname;
  const q = url.searchParams;

  if (path === '/v1/health') {
    return { status: 200, body: { status: 'ok', database: 'fixtures', sim_build: null, native_verifier_ok: false } };
  }

  if (path === '/v1/meta') {
    return {
      status: 200,
      body: {
        sim_build: { sim_version: '0.0.0-fixture', git_sha: 'fixture', build_hash: null,
                     native_verifier_ok: false, wasm_hash: null },
        demo_format_version: 1,
        default_family: 'cpm',
        profiles: [
          { ...CPM, layout_version: 1, created_at: '2026-08-01T00:00:00Z', current: true },
          { ...VQ3, layout_version: 1, created_at: '2026-08-01T00:00:00Z', current: true },
        ],
        limits: { max_commands: 150_000, max_compressed_bytes: 1_048_576,
                  max_decompressed_bytes: 8_388_608, attempt_ttl_ms: 1_800_000 },
        fixture: true,
      },
    };
  }

  if (path === '/v1/maps') {
    return {
      status: 200,
      body: {
        maps: [{
          ...COIL,
          categories: [
            { ...CPM, entries: ENTRIES.length, record: {
                time_ms: ENTRIES[0].time_ms, run_id: ENTRIES[0].run_id,
                run_digest: ENTRIES[0].run_digest, player: ENTRIES[0].player } },
            // Nobody has set a vq3 time. The index says so with a count and a
            // null record, so even here "no record" is a fact and not a gap.
            { ...VQ3, entries: 0, record: null },
          ],
        }],
        total: 1,
      },
    };
  }

  const leaderboard = path.match(/^\/v1\/maps\/([^/]+)\/leaderboard$/);
  const mapDetail = path.match(/^\/v1\/maps\/([^/]+)$/);

  if (mapDetail) {
    const slug = mapDetail[1];
    if (slug === 'void') return { status: 503, body: err('database_unavailable', 'The service is up and Postgres is not.') };
    if (slug !== 'coil') return { status: 404, body: err('unknown_map', `No map called "${slug}".`) };
    return {
      status: 200,
      body: {
        ...COIL,
        default_category: 'cpm',
        leaderboard: '/v1/maps/coil/leaderboard',
        categories: [{ ...CPM, entries: ENTRIES.length }, { ...VQ3, entries: 0 }],
      },
    };
  }

  if (leaderboard) {
    const slug = leaderboard[1];

    // "The service is up and the database is not." A board here is unknown,
    // and rendering it as empty would be a claim nobody made.
    if (slug === 'void') {
      return { status: 503, body: err('database_unavailable', 'select 1 did not round-trip to Postgres.') };
    }
    if (slug !== 'coil') return { status: 404, body: err('unknown_map', `No map called "${slug}".`) };

    const profile = q.get('profile') ?? 'cpm';
    const at = profile.indexOf('@');
    const family = at < 0 ? profile : profile.slice(0, at);
    const pinned = at < 0 ? null : profile.slice(at + 1);

    if (at >= 0 && !/^[0-9a-f]{16}$/.test(pinned ?? '')) {
      return { status: 400, body: err('invalid_category', `"${profile}" is not <family> or <family>@<digest16>.`) };
    }
    if (family !== 'cpm' && family !== 'vq3') {
      return { status: 404, body: err('unknown_physics_family', `No physics profile of kind "${family}".`) };
    }

    const known = family === 'cpm' ? CPM : VQ3;
    if (pinned && pinned !== known.digest) {
      // The one that must never render as empty and never as the current board.
      return {
        status: 404,
        body: err('unknown_physics_digest',
          `No physics profile has digest ${pinned}. The board frozen to those constants is not ` +
          'one this service has ever seen, so it cannot say whether it is empty or full.'),
      };
    }

    const rows = family === 'cpm' ? ENTRIES : [];
    return {
      status: 200,
      body: {
        category: { map: 'coil', ...known, pinned: pinned !== null },
        entries: rows,
        total: rows.length,
        limit: Number(q.get('limit') ?? 50),
        offset: Number(q.get('offset') ?? 0),
      },
    };
  }

  const byDigest = path.match(/^\/v1\/runs\/by-digest\/([0-9a-f]{16})$/);
  if (byDigest) {
    if (byDigest[1] !== RUN_DIGEST) {
      return { status: 404, body: err('unknown_run', `No run with digest ${byDigest[1]}.`) };
    }
    return {
      status: 200,
      body: {
        run_id: ENTRIES[0].run_id,
        run_digest: RUN_DIGEST,
        status: 'verified',
        time_ms: ENTRIES[0].time_ms,
        commands: 3040,
        tick_rate_hz: 125,
        map: { slug: 'coil', name: 'coil', collision_digest: COLLISION_DIGEST },
        category: { map: 'coil', ...CPM },
        player: { display_name: 'nova' },
        submitted_at: ENTRIES[0].set_at,
        verified_at: ENTRIES[0].set_at,
        reject_reason: null,
        demo_bytes: 17_408,
        // Null: this fixture has no bytes, and inventing a URL that 404s would
        // turn "there is no recording here" into a wasm error three layers away.
        demo: null,
        watch: `/watch/${RUN_DIGEST}`,
        diagnostics: {
          client_time_ms: ENTRIES[0].time_ms,
          client_rolling_digest: RUN_DIGEST,
          server_rolling_digest: RUN_DIGEST,
          divergence_at: null,
          sim_build: 'fixture',
          native_verifier_ok: false,
        },
      },
    };
  }

  return { status: 404, body: err('unknown_endpoint', `${path} is not a /v1 endpoint in the fixture set.`) };
}

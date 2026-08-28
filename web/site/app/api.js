// @ts-check
/**
 * The records service, as the site sees it.
 *
 * Every call returns a discriminated result rather than throwing, because the
 * site has to distinguish four outcomes that a thrown exception flattens into
 * one:
 *
 *   ok          — the service answered, here is the data
 *   empty       — the service answered, and the answer is "there is nothing"
 *   absent      — there is no records service to ask
 *   failed      — there is one and it did not answer, or answered badly
 *
 * `empty` and `absent` are the pair that matters. A leaderboard with no rows
 * because nobody has set a time, and a leaderboard with no rows because the
 * service is down, look identical in the DOM and are completely different
 * facts. Conflating them is how a site ends up quietly claiming a map has no
 * records when it has a hundred.
 *
 * The endpoints are `docs/web/ARCHITECTURE.md` §7.5, plus the one addition
 * `docs/web/URLS.md` §5 asks for: `GET /v1/runs/by-digest/:digest16`.
 */

/**
 * @template T
 * @typedef {{status:'ok', data:T, source:'service'}
 *         | {status:'absent', detail:string}
 *         | {status:'failed', code:number, error:string|null, detail:string}} ApiResult
 */

/**
 * `failed` carries the machine-readable `error` code alongside the sentence,
 * because one of them is rendered differently from all the others.
 *
 * `unknown_physics_digest` — a pinned `@digest16` the service has no profile
 * row for — is not "the board failed to load". URLS.md §3 says such a board
 * renders as *unknown*, never as empty and never as the current board, and the
 * only thing that distinguishes it from an ordinary 404 is this code. Dropping
 * it and keeping just the prose would leave the site matching on a sentence.
 */

import { categoryText } from './router.js';

const BASE = '/v1';

/**
 * The Neon Auth bearer token, when the visitor has signed in.
 *
 * Held in memory and in `sessionStorage`, never in a cookie: the service
 * verifies a bearer JWT against the Neon Auth JWKS and there is no cookie
 * session to forge a request against. `null` means anonymous, which is a
 * perfectly good state — anonymous play records locally and is claimed
 * afterwards (ARCHITECTURE §6.4's behaviour, kept).
 *
 * @returns {string|null}
 */
export function token() {
  try {
    return sessionStorage.getItem('straf3.token');
  } catch {
    return null;
  }
}

/** @param {string|null} value */
export function setToken(value) {
  try {
    if (value === null) sessionStorage.removeItem('straf3.token');
    else sessionStorage.setItem('straf3.token', value);
  } catch {
    /* private mode: the session is simply not remembered across reloads */
  }
}

/**
 * @param {string} path
 * @param {RequestInit} [init]
 * @returns {Promise<ApiResult<any>>}
 */
async function call(path, init) {
  /** @type {Record<string,string>} */
  const headers = { accept: 'application/json' };
  const bearer = token();
  if (bearer) headers.authorization = `Bearer ${bearer}`;
  let res;
  try {
    res = await fetch(BASE + path, { ...init, headers: { ...headers, ...(init?.headers ?? {}) } });
  } catch (err) {
    return {
      status: 'failed',
      code: 0,
      error: 'request_failed',
      detail: `the request to ${BASE}${path} did not complete: ${
        err instanceof Error ? err.message : String(err)
      }`,
    };
  }

  let body = null;
  const text = await res.text();
  if (text.length > 0) {
    try {
      body = JSON.parse(text);
    } catch {
      if (res.ok) {
        return {
          status: 'failed',
          code: res.status,
          error: 'not_json',
          detail: `${path} returned ${text.length} bytes that are not JSON`,
        };
      }
    }
  }

  // The dev server's own two answers about the service's existence.
  if (res.status === 503 && body?.error === 'no_records_service') {
    return { status: 'absent', detail: body.detail ?? 'no records service is configured' };
  }
  if (res.status === 502 && body?.error === 'records_service_unreachable') {
    return { status: 'absent', detail: body.detail ?? 'the records service is not reachable' };
  }

  if (!res.ok) {
    return {
      status: 'failed',
      code: res.status,
      error: typeof body?.error === 'string' ? body.error : null,
      detail: body?.detail ?? body?.error ?? `${path} returned HTTP ${res.status}`,
    };
  }
  return { status: 'ok', data: body, source: 'service' };
}

/** `GET /v1/meta` — the build, the profiles, the artifact the browser is served. */
export function meta() {
  return call('/meta');
}

/** `GET /v1/maps` — the index, with per-profile record times. */
export function maps() {
  return call('/maps');
}

/** @param {string} slug */
export function map(slug) {
  return call(`/maps/${encodeURIComponent(slug)}`);
}

/**
 * `GET /v1/maps/:slug/leaderboard`.
 *
 * The category travels as **one** `profile` parameter carrying the whole
 * category key — `cpm`, or `cpm@a1b2c3d4e5f60718` — which is the same grammar
 * the URL path uses (URLS.md §2) and the shape the service implements.
 *
 * It was briefly two parameters here, a family plus a separate
 * `profile_digest`, and that is worth a warning rather than a silent fix: a
 * service reading only `profile` sees a bare family, answers with the *current*
 * board, and a pinned URL quietly renders rows set under constants it did not
 * ask for. That is the substitution ARCHITECTURE §7.2 step 2 forbids, arriving
 * by way of a query parameter nobody was reading. Sending the key whole means
 * a service that does not understand pinning cannot accidentally half-understand
 * it — it either answers the pinned board or it fails.
 *
 * The site still checks the `category` that comes back against the one it
 * asked for, because "cannot half-understand it" is a property of this call and
 * not a guarantee about the answer.
 *
 * @param {string} slug
 * @param {import('./router.js').Category} category
 * @param {{limit?: number, offset?: number}} [page]
 */
export function leaderboard(slug, category, page = {}) {
  const q = new URLSearchParams({ profile: categoryText(category) });
  if (page.limit !== undefined) q.set('limit', String(page.limit));
  if (page.offset !== undefined) q.set('offset', String(page.offset));
  return call(`/maps/${encodeURIComponent(slug)}/leaderboard?${q}`);
}

/**
 * A run, by either spelling of `<run>` (URLS.md §5).
 *
 * @param {string} runRef 16-hex run digest, or a `runs.id` UUID
 */
export function run(runRef) {
  const path = /^[0-9a-f]{16}$/.test(runRef)
    ? `/runs/by-digest/${runRef}`
    : `/runs/${encodeURIComponent(runRef)}`;
  return call(path);
}

/**
 * `POST /v1/attempts` — a ticket for one run (ARCHITECTURE §3.1, §7.3).
 *
 * Fetched when a run *starts*, not when it finishes, because the ticket's TTL
 * has to cover the run. A failure here is not fatal to playing: the site plays
 * the map anyway and says the run will not be rankable, which is true and is
 * better than refusing to start.
 *
 * @param {string} slug
 * @param {import('./router.js').Category} category
 */
export function attempt(slug, category) {
  return call('/attempts', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ map: slug, profile: categoryText(category) }),
  });
}

/**
 * `POST /v1/runs` — submit a finished run.
 *
 * The body is the raw `.s3d` bytes, uncompressed; the ticket travels as
 * `X-Straf3-Ticket`. Both are the service's shapes as implemented.
 *
 * The two `X-Straf3-Client-*` headers are the browser's *claim* about the run,
 * sent through a channel that is not the file. The service does not need them
 * — it re-simulates and computes the time itself (§7.2 step 5), and its answer
 * echoes the `run_digest` it derived from the bytes. That is precisely what
 * makes them useful: the site can then compare what the browser said against
 * what the service computed, and a recording whose header was written by a code
 * path that never ran the simulation has nothing to agree with. Unknown request
 * headers are ignored by a service that does not read them, so this costs
 * nothing if it never does.
 *
 * @param {object} run
 * @param {string} run.ticket
 * @param {number} run.time_ms          what the client says; never ranked
 * @param {string} run.run_digest_hex16 what the client says the rolling digest was
 * @param {Uint8Array} run.s3d
 */
export function submitRun({ ticket, time_ms, run_digest_hex16, s3d }) {
  return call('/runs', {
    method: 'POST',
    headers: {
      'content-type': 'application/vnd.straf3.demo',
      'x-straf3-ticket': ticket,
      'x-straf3-client-run-digest': run_digest_hex16,
      'x-straf3-client-time-ms': String(time_ms),
    },
    body: s3d,
  });
}

/**
 * The `.s3d` bytes for a run — what a ghost and a replay are made of.
 *
 * Returns bytes, not JSON, so it does not go through {@link call}.
 *
 * @param {string} runRef
 * @returns {Promise<ApiResult<Uint8Array>>}
 */
export async function demo(runRef) {
  const path = /^[0-9a-f]{16}$/.test(runRef)
    ? `${BASE}/runs/by-digest/${runRef}/demo`
    : `${BASE}/runs/${encodeURIComponent(runRef)}/demo`;
  let res;
  try {
    res = await fetch(path);
  } catch (err) {
    return {
      status: 'failed',
      code: 0,
      error: 'request_failed',
      detail: `${path}: ${err instanceof Error ? err.message : String(err)}`,
    };
  }
  if (res.status === 503 || res.status === 502) {
    return { status: 'absent', detail: 'the records service is not answering for demo downloads' };
  }
  if (!res.ok) {
    return { status: 'failed', code: res.status, error: null, detail: `${path} returned HTTP ${res.status}` };
  }
  return { status: 'ok', data: new Uint8Array(await res.arrayBuffer()), source: 'service' };
}

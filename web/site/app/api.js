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
 * The category is passed as the two things it is: a family, and — when the URL
 * pinned one — an exact physics digest. A service that does not understand
 * `profile_digest` must answer with what it *did* use, so the page can say the
 * board it is showing is not the board that was asked for. It must not
 * silently substitute the current profile (§7.2 step 2).
 *
 * @param {string} slug
 * @param {import('./router.js').Category} category
 * @param {{limit?: number, offset?: number}} [page]
 */
export function leaderboard(slug, category, page = {}) {
  const q = new URLSearchParams({ profile: category.family });
  if (category.digest) q.set('profile_digest', category.digest);
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
    body: JSON.stringify({
      map: slug,
      profile: category.family,
      profile_digest: category.digest ?? undefined,
    }),
  });
}

/**
 * `POST /v1/runs` — submit a finished run.
 *
 * `run_digest_hex16` travels in the payload beside the bytes even though the
 * service recomputes it from the recording and ranks on its own re-simulation
 * (§7.2 step 5). It is here as a *claim to disagree with*: the site reports
 * what the browser said the digest was, through a channel that is not the file
 * header, so a header written by a code path that never ran the simulation has
 * nothing to agree with. The same value is written to the console for the same
 * reason.
 *
 * The bytes go as base64 in JSON rather than as a raw body: one shape for the
 * whole `/v1` surface, and the payload is ~17 KiB.
 *
 * @param {object} run
 * @param {string} run.ticket
 * @param {number} run.time_ms          what the client says; never ranked
 * @param {string} run.run_digest_hex16 what the client says the rolling digest was
 * @param {Uint8Array} run.s3d
 */
export function submitRun({ ticket, time_ms, run_digest_hex16, s3d }) {
  let binary = '';
  for (let i = 0; i < s3d.length; i += 1) binary += String.fromCharCode(s3d[i]);
  return call('/runs', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      ticket,
      client_time_ms: time_ms,
      run_digest_hex16,
      s3d_base64: btoa(binary),
    }),
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

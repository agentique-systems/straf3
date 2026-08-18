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
 *         | {status:'failed', code:number, detail:string}} ApiResult
 */

const BASE = '/v1';

/**
 * @param {string} path
 * @param {RequestInit} [init]
 * @returns {Promise<ApiResult<any>>}
 */
async function call(path, init) {
  let res;
  try {
    res = await fetch(BASE + path, { headers: { accept: 'application/json' }, ...init });
  } catch (err) {
    return {
      status: 'failed',
      code: 0,
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
        return { status: 'failed', code: res.status, detail: `${path} returned ${text.length} bytes that are not JSON` };
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
    return { status: 'failed', code: 0, detail: `${path}: ${err instanceof Error ? err.message : String(err)}` };
  }
  if (res.status === 503 || res.status === 502) {
    return { status: 'absent', detail: 'the records service is not answering for demo downloads' };
  }
  if (!res.ok) {
    return { status: 'failed', code: res.status, detail: `${path} returned HTTP ${res.status}` };
  }
  return { status: 'ok', data: new Uint8Array(await res.arrayBuffer()), source: 'service' };
}

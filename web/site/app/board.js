// @ts-check
/**
 * The leaderboard, and the four different things "no rows" can mean.
 *
 * This module is requirement r9 in one file. A board with no rows is not one
 * state, it is four, and they demand four different sentences:
 *
 *  1. **rows** — the service answered and there are times.
 *  2. **empty** — the service answered, and the answer is that nobody has set
 *     a time on this board yet. Nothing is wrong. The map is playable and the
 *     first person to finish it holds the record.
 *  3. **unanswerable** — there is no answer. The service is not configured, not
 *     reachable, or up but unable to reach its database. The board *may* be
 *     full; we do not know, and the page must not imply we do.
 *  4. **unknown category** — the URL pinned a physics digest the service has no
 *     profile for. This is its own third thing (URLS.md §3): rendering it as
 *     empty would claim nobody has set a time under those constants, and
 *     rendering the current board instead would silently answer a question
 *     nobody asked — the exact substitution ARCHITECTURE §7.2 step 2 forbids
 *     the verifier from making. The site does not get to make it either.
 *
 * States 2 and 3 look identical in a naive DOM — an empty `<tbody>` — and they
 * are completely different facts about the world. Everything below exists to
 * keep them apart, and `boardView` is a pure function of the asked-for category
 * and the API result so that all four can be rendered side by side and looked
 * at (see `/dev/r9` in `web/dev/serve.mjs`).
 */

import { h, absent, digest as digestEl, provenance, action } from './ui.js';
import { formatTime } from './s3d.js';
import { categoryText, href, isDigest16 } from './router.js';

/**
 * One row of a board, read defensively.
 *
 * Missing fields render as `unknown` rather than as a plausible default. A
 * blank player name is better than "anonymous" invented by the site, and a
 * missing time is better than a zero that sorts to the top.
 *
 * @param {any} e
 * @param {number} index
 */
function entryRow(e, index) {
  const rank = Number.isInteger(e?.rank) ? e.rank : index + 1;
  const name = e?.player?.display_name ?? e?.display_name ?? null;
  const timeMs = Number.isFinite(e?.time_ms) ? e.time_ms : null;
  const runRef = e?.run_digest ?? e?.run_digest_hex16 ?? e?.run_id ?? e?.id ?? null;

  const nameCell = name
    ? h('span', null, name)
    : h('span', { class: 'unknown', title: 'the service returned this entry without a display name' }, 'unknown');

  return h('tr', { class: rank === 1 ? 'is-record' : null },
    h('td', { class: 'rank' }, String(rank)),
    h('td', null, nameCell),
    h('td', { class: 'num time' }, timeMs === null
      ? h('span', { class: 'unknown' }, 'unknown')
      : formatTime(timeMs)),
    h('td', { class: 'num' }, e?.set_at ? String(e.set_at).slice(0, 10) : ''),
    h('td', null, runRef
      ? h('a', { href: href.record(String(runRef)), class: 'mono' }, 'record')
      : ''),
  );
}

/**
 * @param {any[]} entries
 * @param {number|null} total
 */
function table(entries, total) {
  return h('div', null,
    h('table', null,
      h('thead', null, h('tr', null,
        h('th', { class: 'rank' }, '#'),
        h('th', null, 'player'),
        h('th', { class: 'num' }, 'time'),
        h('th', { class: 'num' }, 'set'),
        h('th', null, ''),
      )),
      h('tbody', null, ...entries.map(entryRow)),
    ),
    total !== null && total > entries.length
      ? h('p', { class: 'pending' }, `showing ${entries.length} of ${total}`)
      : null,
  );
}

/**
 * The board that was *asked for*, versus the board that came back.
 *
 * A service that does not understand `profile_digest` must answer with the
 * category it actually used (api.js, ARCHITECTURE §7.2 step 2). If that is not
 * the category in the URL, the page says so above the rows rather than
 * presenting them as the pinned board.
 *
 * @param {import('./router.js').Category} asked
 * @param {any} answered  the `category` object from the response, if any
 */
function substitutionWarning(asked, answered) {
  if (!answered) return null;
  const gotDigest = answered.digest ?? answered.profile_digest ?? null;
  const gotFamily = answered.family ?? answered.profile ?? answered.kind ?? null;

  if (asked.digest && gotDigest && String(gotDigest) !== asked.digest) {
    return absent({
      kind: 'error',
      what: 'These are not the rows this URL asked for.',
      why:
        `The URL pins physics ${asked.digest}, and the records service answered with ` +
        `${gotDigest} instead. A time set under different constants is not comparable ` +
        'to one set under these, so the rows below are not this board.',
      next: 'Treat them as another category until the service answers the pinned one.',
    });
  }
  if (gotFamily && asked.family && String(gotFamily) !== asked.family) {
    return absent({
      kind: 'error',
      what: `This is the ${gotFamily} board, not ${asked.family}.`,
      why: `The URL asked for ${asked.family} and the records service answered with ${gotFamily}.`,
    });
  }
  return null;
}

/**
 * Render one board from one API result. Pure — no fetching, no globals.
 *
 * @param {object} o
 * @param {string} o.map
 * @param {import('./router.js').Category} o.category  the category the URL asked for
 * @param {import('./api.js').ApiResult<any>} o.result
 * @returns {HTMLElement}
 */
export function boardView({ map, category, result }) {
  const label = `${map} · ${categoryText(category)}`;

  // 3. Unanswerable: there is nothing to ask, or asking did not work.
  if (result.status === 'absent') {
    return h('div', null, absent({
      kind: 'unavailable',
      what: 'The records service could not answer.',
      why: result.detail,
      next:
        `This is not an empty board. There may be times on ${label}; ` +
        'this page does not know, and will not guess.',
    }));
  }

  if (result.status === 'failed') {
    // 4. A pinned digest nothing has a profile for. Its own state — never
    //    empty, never the current board.
    if (result.error === 'unknown_physics_digest') {
      return h('div', null, absent({
        kind: 'unknown',
        what: 'Unknown physics.',
        why:
          `This URL pins physics digest ${category.digest}, and the records service has no ` +
          'profile with that digest. The constants this board is frozen to are not ones it ' +
          'has ever seen, so it cannot say whether the board is empty or full.',
        next: 'The current board is a different board and is not shown in its place.',
      }),
      category.digest
        ? h('p', { class: 'pin-note' },
            'The current ', h('code', null, category.family), ' board is at ',
            h('a', { href: href.map(map, { family: category.family, digest: null }) },
              href.map(map, { family: category.family, digest: null })),
            ' — a different question, deliberately not answered here.')
        : null,
      );
    }

    if (result.error === 'unknown_map') {
      return h('div', null, absent({
        kind: 'unknown',
        what: `The records service has no map called "${map}".`,
        why: result.detail,
        next: 'The map may still be playable from a local file; it is not ranked.',
      }));
    }

    if (result.error === 'database_unavailable' || result.code === 503 || result.code === 502) {
      return h('div', null, absent({
        kind: 'unavailable',
        what: 'The records service could not answer.',
        why: `${result.detail} (HTTP ${result.code}${result.error ? `, ${result.error}` : ''})`,
        next: 'This is not an empty board — the rows are unknown, not absent.',
      }));
    }

    return h('div', null, absent({
      kind: 'error',
      what: 'The records service refused this board.',
      why: `${result.detail} (HTTP ${result.code}${result.error ? `, ${result.error}` : ''})`,
      next: 'This is not an empty board.',
    }));
  }

  const data = result.data ?? {};
  const entries = Array.isArray(data.entries) ? data.entries : null;
  const total = Number.isFinite(data.total) ? data.total : null;

  if (entries === null) {
    return h('div', null, absent({
      kind: 'error',
      what: 'The records service answered with something that is not a board.',
      why:
        'A leaderboard response must be an object carrying an `entries` array — a bare list, ' +
        'a 204 or a 404 would all be indistinguishable from "empty", which is why the shape ' +
        'is fixed. This answer had no `entries`.',
    }));
  }

  const warning = substitutionWarning(category, data.category);

  // 2. Empty, and stated as empty — a fact, not a failure.
  if (entries.length === 0) {
    return h('div', null,
      warning,
      absent({
        kind: 'empty',
        what: 'Nobody has set a time here yet.',
        why:
          `The records service answered for ${label} and the board has no entries. ` +
          'This is the board being new, not the service being unavailable.',
        next: 'Finish a run and you hold the record.',
      }),
      h('div', { class: 'page-head', style: 'border:0;margin:1rem 0 0;padding:0' },
        h('div', null),
        h('div', { class: 'actions' },
          action(href.play(map, { category }), `play ${map}`, { kind: 'primary' }))),
      provenance('service', `GET /v1/maps/${map}/leaderboard — 200, ${total ?? 0} total`),
    );
  }

  // 1. Rows.
  return h('div', null,
    warning,
    table(entries, total),
    provenance('service', `GET /v1/maps/${map}/leaderboard — ${entries.length} of ${total ?? entries.length}`),
  );
}

/**
 * The category tabs above a board.
 *
 * The unpinned tabs mean "the current board"; the pinned tab, when the URL has
 * one, keeps its digest and is marked as frozen. Switching families from a
 * pinned board drops the pin, because `cpm@<digest>` and `vq3@<same digest>`
 * is not a thing that exists — a digest belongs to one profile.
 *
 * @param {string} map
 * @param {import('./router.js').Category} active
 * @param {string[]} families
 */
export function categoryTabs(map, active, families) {
  const known = families.length ? families : ['vq3', 'cpm'];
  const tabs = known.map((family) => {
    const isActive = family === active.family && !active.digest;
    return h('a', {
      class: 'category-tab',
      href: href.map(map, { family, digest: null }),
      'aria-current': isActive ? 'page' : null,
    }, family);
  });

  if (active.digest) {
    tabs.push(h('a', {
      class: 'category-tab pinned',
      href: href.map(map, active),
      'aria-current': 'page',
      title: `frozen to physics digest ${active.digest}`,
    }, categoryText(active)));
  }

  return h('nav', { class: 'categories', 'aria-label': 'category' }, ...tabs);
}

/**
 * The one-line explanation of what kind of board this URL names.
 *
 * @param {import('./router.js').Category} category
 */
export function categoryNote(category) {
  if (category.digest) {
    return h('p', { class: 'pin-note' },
      'Pinned to ', digestEl(category.digest, 'physics digest'),
      ' — this board is frozen to exactly those constants and does not move when ',
      h('code', null, category.family), ' is tuned.');
  }
  return h('p', { class: 'pin-note' },
    'The current ', h('code', null, category.family),
    ' board. Tuning ', h('code', null, category.family),
    ' changes what this page shows; a link out of a record pins the digest instead.');
}

/** @param {string} s */
export function looksPinned(s) {
  const at = s.indexOf('@');
  return at >= 0 && isDigest16(s.slice(at + 1));
}

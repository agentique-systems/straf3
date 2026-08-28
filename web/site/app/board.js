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
 * and the API result, so all four can be produced on demand and looked at:
 * `node web/dev/serve.mjs --fixtures` serves each from a real route.
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
 * `rank` comes from the service's `rank() over (order by time_ms, set_at)`, so
 * tied times share a rank. It is used as given rather than replaced by the row
 * index: renumbering a tie into 1, 2 invents an ordering the data does not have.
 *
 * @param {any} e
 * @param {number} index
 */
function entryRow(e, index) {
  const rank = Number.isInteger(e?.rank) ? e.rank : index + 1;
  const name = typeof e?.player === 'string' ? e.player : (e?.player?.display_name ?? e?.display_name ?? null);
  const timeMs = Number.isFinite(e?.time_ms) ? e.time_ms : null;
  const runRef = e?.run_digest ?? e?.run_id ?? null;

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
 * The service must answer with the category it actually used (ARCHITECTURE
 * §7.2 step 2). When that is not the category in the URL, the rows are **not
 * shown at all** — the mismatch replaces them rather than heading them.
 *
 * The temptation is to print a warning and render the rows underneath, on the
 * grounds that some data beats none. It does not. A pinned URL that displays
 * the current board with a caption is still displaying the current board, and
 * URLS.md §3 says a pinned key whose digest the service does not honour renders
 * as unknown, "not as empty and never as the current board". Rows a reader can
 * see are rows a reader will read; the caption loses.
 *
 * @param {import('./router.js').Category} asked
 * @param {any} answered  the `category` object from the response, if any
 * @returns {HTMLElement|null} a replacement for the board, or null to render it
 */
function substitution(asked, answered) {
  if (!answered) return null;
  const gotDigest = answered.digest ?? null;
  const gotFamily = answered.family ?? null;

  if (asked.digest && gotDigest && String(gotDigest) !== asked.digest) {
    return h('div', null, absent({
      kind: 'unknown',
      what: 'The records service answered about a different board.',
      why:
        `This URL pins physics ${asked.digest}. The service replied with the board for ` +
        `${gotDigest} instead — times set under different constants, which are not comparable ` +
        'to times set under the ones this URL names.',
      next:
        'Those rows are not shown here. Showing them under this address would make this page ' +
        'the current board wearing a pinned URL.',
    }));
  }
  if (gotFamily && asked.family && String(gotFamily) !== asked.family) {
    return h('div', null, absent({
      kind: 'unknown',
      what: `The records service answered with the ${gotFamily} board.`,
      why: `This URL asks for ${asked.family}, and ${gotFamily} is a different game with a different board.`,
      next: 'Its rows are not shown here.',
    }));
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

    // A family with no rows at all, as opposed to a *pinned* key naming a
    // digest with no row. Different questions, different answers.
    if (result.error === 'unknown_physics_family') {
      return h('div', null, absent({
        kind: 'unknown',
        what: `There is no physics family called "${category.family}".`,
        why:
          `${result.detail} A category is (map, physics profile), and the records service has ` +
          `no profile of kind ${category.family} — so there is no board here to be empty or full.`,
        next: 'The families this service knows are listed on the map page above.',
      }));
    }

    // Malformed against URLS.md §2's grammar. A client error, not a missing
    // resource — the router normally catches these before a request is made,
    // so reaching here means the two grammars disagree, which is worth saying.
    if (result.error === 'invalid_category') {
      return h('div', null, absent({
        kind: 'error',
        what: 'This category key is not well formed.',
        why:
          `${result.detail} A category is <family> or <family>@<digest16>, with the digest ` +
          'sixteen lowercase hex characters.',
      }));
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

  // Before anything is rendered: is this even the board that was asked for?
  const wrongBoard = substitution(category, data.category);
  if (wrongBoard) return wrongBoard;

  // 2. Empty, and stated as empty — a fact, not a failure.
  if (entries.length === 0) {
    return h('div', null,
      absent({
        kind: 'empty',
        what: 'Nobody has set a time here yet.',
        why:
          `The records service answered for ${label} and the board has no entries. ` +
          'This is the board being new, not the service being unavailable.',
        next: 'Finish a run and you hold the record.',
      }),
      h('div', { class: 'actions', style: 'margin-top:1rem' },
        action(href.play(map, { category }), `play ${map}`, { kind: 'primary' })),
      provenance('service', `GET /v1/maps/${map}/leaderboard — 200, ${total ?? 0} total`),
    );
  }

  // 1. Rows.
  return h('div', null,
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
 * `families` is whatever the service said this map has, and there is
 * deliberately no fallback list. Defaulting to `['vq3', 'cpm']` when the
 * service cannot answer would put two tabs on the page for boards nothing has
 * claimed exist — a small invention, on the page whose whole job is not making
 * them.
 *
 * @param {string} map
 * @param {import('./router.js').Category} active
 * @param {string[]} families
 */
export function categoryTabs(map, active, families) {
  const tabs = families.map((family) => {
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

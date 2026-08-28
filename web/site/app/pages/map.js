// @ts-check
/**
 * `/m/<map>` and `/m/<map>/<category>` — a map's record book.
 *
 * Two behaviours here are the URL scheme rather than the layout:
 *
 *  - **`/m/<map>` canonicalises.** A bare map URL is a convenience for typing,
 *    never a stored link (URLS.md §3), so it resolves to the map's default
 *    category and the address bar ends up on the explicit form. The default is
 *    a fact the records service holds — so when the service cannot answer, the
 *    page does *not* canonicalise to a guess. It says which categories the URL
 *    grammar allows and waits. Guessing `cpm` would put a URL in the address
 *    bar, and then in someone's history, that nothing ever asserted.
 *  - **The pin travels.** The category tabs mean "current" and are unpinned.
 *    Every link this page emits *out of a record* — a row's watch link, a play
 *    link beside a time — carries that record's physics digest, because a
 *    record is bound to the constants it was set under.
 */

import * as api from '../api.js';
import { h, absent, action, pageHead, shareLink } from '../ui.js';
import { href, categoryText } from '../router.js';
import { boardView, categoryTabs, categoryNote } from '../board.js';
import * as router from '../router.js';

/** @param {any} route */
export function title(route) {
  const c = route.category ? ` · ${categoryText(route.category)}` : '';
  return `${route.map}${c} — straf3`;
}

/**
 * @param {any} route
 * @param {HTMLElement} host
 * @param {{alive: () => boolean}} ctx
 */
export async function render(route, host, ctx) {
  const slug = route.map;

  const head = pageHead({
    title: slug,
    sub: route.category
      ? `record book · ${categoryText(route.category)}`
      : 'record book',
    actions: [action(href.play(slug, { category: route.category }), `play ${slug}`, { kind: 'primary' })],
  });

  const tabsHost = h('div', null);
  const noteHost = h('div', null);
  const boardHost = h('div', null, h('p', { class: 'pending' }, 'asking the records service…'));
  const detailHost = h('div', null);

  host.append(
    head,
    tabsHost,
    noteHost,
    h('section', null, h('h2', null, 'leaderboard'), boardHost),
    detailHost,
  );

  if (route.category) {
    noteHost.replaceChildren(categoryNote(route.category));
    host.append(shareLink(href.map(slug, route.category), 'this board'));
  }

  // The map detail answers two questions at once: which categories exist (for
  // the tabs), and which one is the default (for canonicalising a bare URL).
  const detail = await api.map(slug);
  if (!ctx.alive()) return;

  /** @type {any[]} */
  let categories = [];
  /** @type {string|null} */
  let defaultFamily = null;

  if (detail.status === 'ok') {
    categories = Array.isArray(detail.data?.categories) ? detail.data.categories : [];
    defaultFamily = typeof detail.data?.default_category === 'string'
      ? detail.data.default_category.split('@')[0]
      : null;
    detailHost.replaceChildren(mapFacts(detail.data));
  } else if (detail.status === 'absent') {
    detailHost.replaceChildren(absent({
      kind: 'unavailable',
      what: 'The records service could not describe this map.',
      why: detail.detail,
      next: `Playing does not need it: ${href.play(slug)} serves the map from this origin.`,
    }));
  } else if (detail.error === 'unknown_map') {
    detailHost.replaceChildren(absent({
      kind: 'unknown',
      what: `The records service has no map called "${slug}".`,
      why: detail.detail,
      next: 'It may exist as a file and simply not be registered; it cannot be ranked until it is.',
    }));
  } else {
    // 502 and 503 are "could not answer" wherever they appear, and they get the
    // same amber treatment as an unanswerable board. A red refusal box beside
    // an amber one, both describing the same outage, would read as two
    // different problems.
    detailHost.replaceChildren(absent({
      kind: detail.code === 503 || detail.code === 502 ? 'unavailable' : 'error',
      what: 'The records service could not describe this map.',
      why: `${detail.detail} (HTTP ${detail.code}${detail.error ? `, ${detail.error}` : ''})`,
    }));
  }

  // ── canonicalise a bare /m/<map> ──────────────────────────────────────────
  if (!route.category) {
    if (defaultFamily) {
      router.go(href.map(slug, { family: defaultFamily, digest: null }), { replace: true });
      return; // a new render is already under way
    }
    const families = categories.map((/** @type {any} */ c) => String(c?.family)).filter(Boolean);
    boardHost.replaceChildren(absent({
      kind: 'unavailable',
      what: 'Which category?',
      why:
        'A category is (map, physics profile), and which profile is this map\'s default is a ' +
        'fact the records service holds. It did not answer, so this URL cannot resolve to ' +
        'the explicit form the way a bare map URL is supposed to.',
      next: 'Pick a category and the address bar will hold a link worth keeping.',
    }));
    if (families.length) tabsHost.replaceChildren(categoryTabs(slug, { family: '', digest: null }, families));
    return;
  }

  // Only the families the service actually named. When it named none, the one
  // in the URL is still worth a tab — it is what this page is about — but no
  // sibling is offered, because none is known to exist.
  const families = categories.map((/** @type {any} */ c) => String(c?.family)).filter(Boolean);
  tabsHost.replaceChildren(categoryTabs(slug, route.category,
    families.length ? families : (route.category.digest ? [] : [route.category.family])));

  const board = await api.leaderboard(slug, route.category, { limit: 50 });
  if (!ctx.alive()) return;

  boardHost.replaceChildren(boardView({ map: slug, category: route.category, result: board }));
}

/**
 * What the service knows about the map itself.
 *
 * `collision_digest` is here because it is the thing that decides whether a
 * recording can be replayed at all: a ghost replayed against geometry that
 * moved shows a run that never happened (URLS.md §4).
 *
 * @param {any} m
 */
function mapFacts(m) {
  const cats = Array.isArray(m?.categories) ? m.categories : [];
  return h('section', null,
    h('h2', null, 'the map'),
    h('dl', { class: 'facts' },
      fact('name', m?.name ?? null),
      fact('author', m?.author ?? null),
      fact('collision digest', m?.collision_digest ?? null, 'the compiled hulls — a replay is only valid against these'),
      fact('map compiler', m?.map_compiler_version ?? null),
      fact('timing', m?.has_timing === undefined
        ? null
        : (m.has_start_trigger && m.has_finish_trigger ? 'start and finish triggers' : 'no timing triggers'),
        m?.has_timing === false ? 'a run here cannot be timed, so it cannot be ranked' : undefined),
      fact('source', m?.source_url ?? null),
    ),
    cats.length
      ? h('div', null,
          h('p', { class: 'pin-note' },
            'Frozen links to each of this map\'s boards — these keep meaning what they mean ',
            'after the physics is tuned:'),
          h('ul', { class: 'notes' }, ...cats.map((/** @type {any} */ c) =>
            c?.key
              ? h('li', null,
                  h('a', { href: `/m/${m.slug}/${c.key}`, class: 'mono' }, `/m/${m.slug}/${c.key}`),
                  c?.label ? ` — ${c.label}` : '')
              : null)))
      : null,
  );
}

/**
 * A fact row that renders a missing value as `unknown` rather than as blank.
 *
 * @param {string} term
 * @param {any} value
 * @param {string} [note]
 */
function fact(term, value, note) {
  return h('div', { class: 'fact' },
    h('dt', null, term),
    h('dd', null,
      value === null || value === undefined || value === ''
        ? h('span', { class: 'unknown' }, 'unknown')
        : String(value),
      note ? h('small', null, note) : null),
  );
}

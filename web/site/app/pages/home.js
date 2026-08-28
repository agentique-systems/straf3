// @ts-check
/**
 * `/` — the map index.
 *
 * The index is the one page whose emptiness is most tempting to fake. A grid of
 * placeholder cards while `/v1/maps` is in flight looks better than a sentence
 * and is a picture of maps that may not exist; a grid that stays empty when the
 * request fails says the platform has no maps, which is a claim about the
 * world, not about the request. So: a sentence while asking, and one of three
 * different answers after.
 */

import * as api from '../api.js';
import { h, absent, action, pageHead, provenance } from '../ui.js';
import { formatTime } from '../s3d.js';
import { href } from '../router.js';

export function title() {
  return 'straf3 — maps';
}

/**
 * One map card.
 *
 * Each category row carries the service's own count and record for that board,
 * so the difference between "0 entries" and "no record" is already made here
 * rather than inferred from a missing time. A category with no record shows the
 * family and the word *no record*, never a `--:--.---` shaped like a time.
 *
 * The category links are **unpinned** — this is the index, and the index means
 * "the current cpm board". A link out of a *record* pins the digest instead
 * (URLS.md §3); that is the record page's job, not this one's.
 *
 * @param {any} m
 */
function card(m) {
  const slug = String(m?.slug ?? '');
  const name = m?.name ?? slug;
  const categories = Array.isArray(m?.categories) ? m.categories : [];

  return h('article', { class: 'card' },
    h('h3', null, h('a', { href: href.map(slug) }, name)),
    h('p', { class: 'card-sub' }, slug, m?.author ? ` · ${m.author}` : ''),

    categories.length
      ? h('table', null, h('tbody', null, ...categories.map((/** @type {any} */ c) => {
          const family = String(c?.family ?? '?');
          const rec = c?.record ?? null;
          return h('tr', null,
            h('td', null, h('a', {
              href: href.map(slug, { family, digest: null }),
              class: 'mono',
              title: c?.label ?? undefined,
            }, family)),
            h('td', { class: 'num time' }, rec && Number.isFinite(rec.time_ms)
              ? h('a', { href: href.record(String(rec.run_digest ?? rec.run_id)) }, formatTime(rec.time_ms))
              : h('span', { class: 'unknown' }, 'no record')),
            h('td', null, rec?.player ?? (Number.isFinite(c?.entries) ? `${c.entries} entries` : '')),
          );
        })))
      : h('p', { class: 'pending' }, 'the service lists no categories for this map'),

    h('div', { class: 'card-actions' },
      action(href.play(slug), 'play', { kind: 'primary' }),
      action(href.map(slug), 'record book'),
    ),
  );
}

/**
 * @param {any} _route
 * @param {HTMLElement} host
 * @param {{alive: () => boolean}} ctx
 */
export async function render(_route, host, ctx) {
  const body = h('div', null, h('p', { class: 'pending' }, 'asking the records service for the map list…'));

  host.append(
    pageHead({
      title: 'straf3',
      sub: 'A first-person movement game in the Quake 3 Defrag tradition, on a deterministic simulation.',
    }),
    h('p', { class: 'lede' },
      'Every link here is a real path. Paste one into a fresh tab and it loads: a map, ',
      'a category board frozen to exact physics, a record, or the game itself with the ',
      'map already running.'),
    h('section', null, h('h2', null, 'maps'), body),
  );

  const result = await api.maps();
  if (!ctx.alive()) return;

  if (result.status === 'absent') {
    body.replaceChildren(absent({
      kind: 'unavailable',
      what: 'The records service could not answer.',
      why: result.detail,
      next:
        'The map list lives in the records service, so this page does not know which maps ' +
        'exist. It is not claiming there are none.',
    }), localHint());
    return;
  }

  if (result.status === 'failed') {
    body.replaceChildren(absent({
      kind: result.code === 503 || result.code === 502 ? 'unavailable' : 'error',
      what: 'The records service could not answer for the map list.',
      why: `${result.detail} (HTTP ${result.code}${result.error ? `, ${result.error}` : ''})`,
      next: 'This is not "there are no maps" — the list is unknown.',
    }), localHint());
    return;
  }

  const maps = Array.isArray(result.data?.maps)
    ? result.data.maps
    : Array.isArray(result.data)
      ? result.data
      : null;

  if (maps === null) {
    body.replaceChildren(absent({
      kind: 'error',
      what: 'The records service answered with something that is not a map list.',
      why: 'Expected an object carrying a `maps` array.',
    }));
    return;
  }

  if (maps.length === 0) {
    body.replaceChildren(absent({
      kind: 'empty',
      what: 'The records service has no maps yet.',
      why: 'It answered, and its map table is empty. Nothing is wrong; nothing has been seeded.',
      next: 'A map becomes browsable once it is registered with its collision digest.',
    }), localHint());
    return;
  }

  body.replaceChildren(
    h('div', { class: 'cards' }, ...maps.map(card)),
    provenance('service', `GET /v1/maps — ${maps.length} map${maps.length === 1 ? '' : 's'}`),
  );
}

/**
 * What can still be done with no records service.
 *
 * `/play/<map>` needs no service: the map comes from `/assets/maps/`, and the
 * physics from the URL or the build. Saying so turns an unavailable index into
 * a page that still gets you into the game.
 */
function localHint() {
  return h('section', null,
    h('h2', null, 'without the records service'),
    h('p', { class: 'lede' },
      'Playing does not need it. The map is served from this origin and the physics comes ',
      'from the URL; only ranking a time needs the service.'),
    h('div', { class: 'actions' },
      action(href.play('coil'), 'play coil', { kind: 'primary' }),
      action(href.map('coil'), "coil's record book"),
    ),
  );
}

// @ts-check
/**
 * Not a route.
 *
 * The shell was served with 200 — that is the fallback rule (URLS.md §7), and
 * it is right: any server implementing `try_files $uri /index.html` cannot know
 * which extension-less paths are routes, and a fallback served as 404 would
 * tell every crawler and link checker that the *real* pages do not exist
 * either. The page itself is where "this is not a thing" gets said.
 *
 * What this page must never do is **redirect**. `/r/0123456789ABCDEF` is not a
 * misspelling to be helpfully corrected into `/r/0123456789abcdef`: URLS.md §2
 * fixes every identifier as lowercase, and accepting a second spelling by
 * redirect gives one record two URLs, a cache two copies, and a share button an
 * address that does not match the bar. So the page names the rule and offers a
 * link the reader can choose to follow.
 */

import { h, absent, action } from '../ui.js';
import { href } from '../router.js';
import { pageHead } from '../ui.js';

/** @param {any} route */
export function title(route) {
  return `not found — ${route.path ?? ''} · straf3`;
}

/**
 * If the path differs from the path only by case, say so — and offer the
 * lowercase form as a link rather than going there.
 *
 * @param {string} path
 */
function caseHint(path) {
  const lower = path.toLowerCase();
  if (lower === path || !/[A-Z]/.test(path)) return null;
  return h('div', null,
    h('p', { class: 'pin-note' },
      'Every identifier in a straf3 URL is lowercase, and an uppercase one is not a ',
      'misspelling of a valid URL — it is a different URL, and it does not exist. ',
      'This page does not redirect to the lowercase form, because one record with two ',
      'addresses is one record too many.'),
    h('p', null, 'You probably meant ', h('a', { href: lower }, h('code', null, lower)), '.'),
  );
}

/**
 * @param {any} route
 * @param {HTMLElement} host
 */
export async function render(route, host) {
  const path = route.path ?? location.pathname;

  host.append(
    pageHead({
      title: 'Not found',
      sub: h('code', null, path + location.search),
    }),
    absent({
      kind: 'error',
      what: 'This is not a straf3 URL.',
      why: route.why ?? 'the path does not match any route in docs/web/URLS.md §1',
    }),
    caseHint(path),
    h('section', null,
      h('h2', null, 'the whole scheme'),
      h('dl', { class: 'route-list' },
        h('dt', null, '/'), h('dd', null, 'the map index'),
        h('dt', null, '/m/<map>'), h('dd', null, "a map's record book, default category"),
        h('dt', null, '/m/<map>/<category>'), h('dd', null, "one category's board — cpm, vq3, or family@digest16"),
        h('dt', null, '/r/<run>'), h('dd', null, 'a record: the run as evidence'),
        h('dt', null, '/play/<map>'), h('dd', null, 'launch that map in the browser client'),
        h('dt', null, '/watch/<run>'), h('dd', null, 'play that record back'),
      ),
      h('div', { class: 'actions' }, action(href.home(), 'the map index', { kind: 'primary' })),
    ),
  );
}

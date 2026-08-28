// @ts-check
/**
 * The entry point: one shell, one router, six pages.
 *
 * `index.html` is served for every route in URLS.md §1 with status 200, so the
 * first thing that happens on any cold load — a pasted `/watch/<run>`, a
 * bookmarked `/m/coil/cpm@…`, a link out of a chat client — is this module
 * reading `location.pathname` and deciding what the page is. There is no hash
 * routing: every one of those URLs is a real path the server saw, which is what
 * makes it cacheable, indexable and checkable.
 *
 * Three rules this file enforces on everything above it:
 *
 *  - **Nothing assembles a path.** Links come from `router.href`. A page that
 *    concatenated `'/m/' + slug` would be the place the scheme starts to drift.
 *  - **An unparseable URL renders as not-found, and is never redirected.** An
 *    uppercase digest is a 404, not a redirect to its lowercase spelling
 *    (URLS.md §2): two spellings of one record is how a cache ends up holding
 *    two copies of a page and a share button produces a link that does not
 *    match the address bar.
 *  - **A page render can be superseded.** Pages await the network; a visitor
 *    who navigates during that await must not have the old page's late data
 *    land on the new one. Every render carries a generation and drops itself if
 *    it is no longer current.
 */

import * as router from './router.js';
import { h, absent } from './ui.js';

import * as home from './pages/home.js';
import * as mapPage from './pages/map.js';
import * as record from './pages/record.js';
import * as play from './pages/play.js';
import * as watch from './pages/watch.js';
import * as notfound from './pages/notfound.js';

/** @type {Record<string, {render: Function, title: Function, immersive?: boolean}>} */
const PAGES = {
  home,
  map: mapPage,
  record,
  play,
  watch,
  notfound,
};

const app = /** @type {HTMLElement} */ (document.getElementById('app'));

let generation = 0;
/** @type {null | (() => void)} */
let teardown = null;

/**
 * The context every page render is given.
 *
 * `alive()` is the whole of it: a page that has awaited anything checks it
 * before touching the DOM.
 *
 * @param {number} gen
 */
function context(gen) {
  return {
    /** Is this render still the current one? */
    alive: () => gen === generation,
    /** Register cleanup — canvas observers, animation frames, abort controllers. */
    /** @param {() => void} fn */
    onTeardown: (fn) => {
      if (gen === generation) teardown = fn;
      else fn();
    },
  };
}

async function render() {
  const gen = ++generation;

  if (teardown) {
    try {
      teardown();
    } catch (err) {
      console.error('[straf3] page teardown threw', err);
    }
    teardown = null;
  }

  const route = router.current();
  const page = PAGES[route.name] ?? notfound;

  delete document.body.dataset.mode;
  if (page.immersive) document.body.dataset.mode = 'immersive';

  document.title = page.title(route);
  markNav(route);
  app.replaceChildren();

  try {
    await page.render(route, app, context(gen));
  } catch (err) {
    if (gen !== generation) return;
    console.error('[straf3] page render threw', err);
    app.replaceChildren(absent({
      kind: 'error',
      what: 'This page failed to render.',
      why: err instanceof Error ? `${err.name}: ${err.message}` : String(err),
      next: 'The URL itself is fine — reloading will try again.',
    }));
  }
}

/** @param {router.Route} route */
function markNav(route) {
  for (const a of document.querySelectorAll('.site-nav a')) {
    const isHome = a.getAttribute('href') === '/' && route.name === 'home';
    if (isHome) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  }
}

/**
 * Plain `<a href>` anchors navigate without a reload.
 *
 * Intercepted here rather than by wrapping every link in a component, so a
 * hand-written anchor anywhere in the site behaves the same as a generated one
 * — and so that middle-click, ctrl-click, and copy-link-address keep working,
 * because the href is real.
 *
 * @param {MouseEvent} event
 */
function onClick(event) {
  if (event.defaultPrevented || event.button !== 0) return;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;

  const anchor = /** @type {HTMLElement|null} */ (event.target)?.closest?.('a[href]');
  if (!(anchor instanceof HTMLAnchorElement)) return;
  if (anchor.target && anchor.target !== '_self') return;
  if (anchor.hasAttribute('download') || anchor.origin !== location.origin) return;

  // `/health`, `/v1/…`, `/dev/…` and `/client/…` are the server's, not the
  // router's. URLS.md §6: those namespaces do not overlap with site routes.
  const first = anchor.pathname.split('/')[1];
  if (first === 'v1' || first === 'health' || first === 'dev' || first === 'client' || first === 'assets') return;

  event.preventDefault();
  const url = anchor.pathname + anchor.search + anchor.hash;
  if (url === location.pathname + location.search + location.hash) return;
  router.go(url);
}

/** The header's one-line statement of where this page is being served from. */
function showOrigin() {
  const note = document.getElementById('origin-note');
  if (note) note.textContent = `one origin · ${location.origin}`;
}

showOrigin();
document.addEventListener('click', onClick);
window.addEventListener('popstate', () => {
  void render();
});
void render();

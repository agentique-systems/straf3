// @ts-check
/**
 * The URL scheme, as code.
 *
 * `docs/web/URLS.md` is the specification; this file parses and builds it, and
 * nothing else in the site is allowed to assemble a path by string
 * concatenation. One place that knows the shapes is what makes them
 * changeable — and, more importantly, what makes them *not* quietly drift.
 *
 * Every route's first segment is a fixed keyword, so dispatch is a switch on
 * segment 0 with no lookahead and no ambiguity, and no map slug or run id can
 * ever shadow an action route.
 */

/** Permanently reserved first path segments — URLS.md §6. */
export const RESERVED = new Set([
  'v1', 'm', 'r', 'play', 'watch', 'assets', 'app', 'client', 'dev', 'health',
]);

const MAP_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;
const FAMILY_RE = /^[a-z0-9]{1,16}$/;
const DIGEST16_RE = /^[0-9a-f]{16}$/;
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** @param {string} s */
export function isMapSlug(s) {
  return MAP_RE.test(s);
}

/** @param {string} s */
export function isDigest16(s) {
  return DIGEST16_RE.test(s);
}

/** @param {string} s */
export function isUuid(s) {
  return UUID_RE.test(s);
}

/** A `<run>`: a 16-hex run digest, or a `runs.id` UUID. @param {string} s */
export function isRunRef(s) {
  return isDigest16(s) || isUuid(s);
}

/**
 * @typedef {object} Category
 * @property {string} family        a `physics_profiles.kind` — `vq3`, `cpm`
 * @property {string|null} digest   16-hex `PhysicsId`, when the URL pins one
 */

/**
 * Parse `<family>` or `<family>@<digest16>`.
 *
 * @param {string} text
 * @returns {Category|null}
 */
export function parseCategory(text) {
  const at = text.indexOf('@');
  if (at < 0) return FAMILY_RE.test(text) ? { family: text, digest: null } : null;
  const family = text.slice(0, at);
  const digest = text.slice(at + 1);
  if (!FAMILY_RE.test(family) || !isDigest16(digest)) return null;
  return { family, digest };
}

/**
 * The URL text for a category. Pinned categories keep their digest; that is
 * the difference between a link that still means something after the constants
 * are tuned and one that does not (ARCHITECTURE §5.4).
 *
 * @param {Category} c
 */
export function categoryText(c) {
  return c.digest ? `${c.family}@${c.digest}` : c.family;
}

/** Is this category frozen to exact constants, or does it follow the family? */
/** @param {Category} c */
export function isPinned(c) {
  return c.digest !== null;
}

/**
 * @typedef {{name: 'home'}
 *         | {name: 'map', map: string, category: Category|null}
 *         | {name: 'record', run: string}
 *         | {name: 'play', map: string, category: Category|null, ghost: string|null}
 *         | {name: 'watch', run: string, seekMs: number|null}
 *         | {name: 'notfound', path: string, why: string}} Route
 */

/**
 * Parse a location into a route.
 *
 * @param {string} pathname
 * @param {string} [search]
 * @returns {Route}
 */
export function parse(pathname, search = '') {
  const q = new URLSearchParams(search);
  const parts = pathname.split('/').filter((s) => s.length > 0);

  if (parts.length === 0) return { name: 'home' };

  const nf = (/** @type {string} */ why) => ({ name: /** @type {const} */ ('notfound'), path: pathname, why });

  switch (parts[0]) {
    case 'm': {
      if (parts.length < 2) return nf('a map link needs a map: /m/<map>');
      if (!isMapSlug(parts[1])) return nf(`"${parts[1]}" is not a map slug`);
      if (parts.length === 2) return { name: 'map', map: parts[1], category: null };
      if (parts.length > 3) return nf('too many path segments for a map link');
      const category = parseCategory(parts[2]);
      if (!category) return nf(`"${parts[2]}" is not a category — expected vq3, cpm, or family@digest16`);
      return { name: 'map', map: parts[1], category };
    }

    case 'r': {
      if (parts.length !== 2) return nf('a record link is /r/<run>');
      if (!isRunRef(parts[1])) return nf(`"${parts[1]}" is not a run — expected 16 hex digits or a UUID`);
      return { name: 'record', run: parts[1] };
    }

    case 'play': {
      if (parts.length !== 2) return nf('a launch link is /play/<map>');
      if (!isMapSlug(parts[1])) return nf(`"${parts[1]}" is not a map slug`);
      const p = q.get('p');
      let category = null;
      if (p !== null) {
        category = parseCategory(p);
        if (!category) return nf(`?p=${p} is not a category`);
      }
      const ghost = q.get('ghost');
      if (ghost !== null && !isRunRef(ghost)) return nf(`?ghost=${ghost} is not a run`);
      return { name: 'play', map: parts[1], category, ghost };
    }

    case 'watch': {
      if (parts.length !== 2) return nf('a replay link is /watch/<run>');
      if (!isRunRef(parts[1])) return nf(`"${parts[1]}" is not a run`);
      const t = q.get('t');
      let seekMs = null;
      if (t !== null) {
        const n = Number(t);
        if (!Number.isInteger(n) || n < 0) return nf(`?t=${t} is not a whole number of milliseconds`);
        seekMs = n;
      }
      return { name: 'watch', run: parts[1], seekMs };
    }

    default:
      return nf(`"${parts[0]}" is not a route — try /, /m/<map>, /r/<run>, /play/<map>, /watch/<run>`);
  }
}

// ── building ────────────────────────────────────────────────────────────────

export const href = {
  home: () => '/',
  /** @param {string} map @param {Category} [category] */
  map: (map, category) => (category ? `/m/${map}/${categoryText(category)}` : `/m/${map}`),
  /** @param {string} run */
  record: (run) => `/r/${run}`,
  /**
   * @param {string} map
   * @param {{category?: Category|null, ghost?: string|null}} [o]
   */
  play: (map, o = {}) => {
    const q = new URLSearchParams();
    if (o.category) q.set('p', categoryText(o.category));
    if (o.ghost) q.set('ghost', o.ghost);
    const s = q.toString();
    return s ? `/play/${map}?${s}` : `/play/${map}`;
  },
  /** @param {string} run @param {number|null} [seekMs] */
  watch: (run, seekMs = null) =>
    seekMs ? `/watch/${run}?t=${seekMs}` : `/watch/${run}`,
};

/**
 * Navigate without a reload. Every internal link on the site goes through
 * here, and `main.js` also intercepts plain `<a href>` clicks so a hand-written
 * anchor behaves the same.
 *
 * @param {string} url
 * @param {{replace?: boolean}} [o]
 */
export function go(url, o = {}) {
  if (o.replace) history.replaceState(null, '', url);
  else history.pushState(null, '', url);
  window.dispatchEvent(new PopStateEvent('popstate'));
}

/** The current route, from the address bar. */
export function current() {
  return parse(location.pathname, location.search);
}

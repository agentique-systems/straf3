// @ts-check
/**
 * DOM helpers, and the site's rules about telling the truth.
 *
 * Two of the functions here carry a policy rather than a convenience:
 *
 *  - {@link absent} is how the site renders data it does not have. There is no
 *    skeleton loader that resolves into a plausible number, no `--:--.---`
 *    that looks like a time, no example row. An empty board says it is empty
 *    and says *why* it is empty, because "nobody has set a time" and "the
 *    records service is not answering" are different facts and a leaderboard
 *    that conflates them is lying about the state of the world.
 *  - {@link provenance} labels where a fact came from. A record read from a
 *    local `.s3d` is real, and it is not a verified record; the difference is
 *    the entire value of the verification service and it has to be visible.
 */

/**
 * @param {string} tag
 * @param {Record<string, any>|null} [attrs]
 * @param {...(Node|string|null|undefined|Array<Node|string|null|undefined>)} children
 * @returns {HTMLElement}
 */
export function h(tag, attrs = null, ...children) {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (v === null || v === undefined || v === false) continue;
      if (k === 'class') el.className = String(v);
      else if (k === 'dataset') Object.assign(el.dataset, v);
      else if (k.startsWith('on') && typeof v === 'function') {
        el.addEventListener(k.slice(2).toLowerCase(), v);
      } else if (v === true) el.setAttribute(k, '');
      else el.setAttribute(k, String(v));
    }
  }
  const add = (/** @type {any} */ c) => {
    if (c === null || c === undefined || c === false) return;
    if (Array.isArray(c)) { c.forEach(add); return; }
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  };
  children.forEach(add);
  return el;
}

/** @param {HTMLElement} host @param {...Node} nodes */
export function replace(host, ...nodes) {
  host.replaceChildren(...nodes);
}

/**
 * The absence of data, stated.
 *
 * @param {object} o
 * @param {string} o.what     what is missing, in the site's own words
 * @param {string} o.why      the reason, concretely — not "something went wrong"
 * @param {string} [o.next]   what the reader can do about it
 * @param {'empty'|'unavailable'|'unknown'|'error'} [o.kind]
 */
export function absent({ what, why, next, kind = 'empty' }) {
  return h('div', { class: `absent absent-${kind}`, role: 'status' },
    h('p', { class: 'absent-what' }, what),
    h('p', { class: 'absent-why' }, why),
    next ? h('p', { class: 'absent-next' }, next) : null,
  );
}

/**
 * Where a fact came from. Rendered next to the facts it qualifies.
 *
 * @param {'service'|'local-file'|'client'|'url'} source
 * @param {string} detail
 */
export function provenance(source, detail) {
  const label = {
    service: 'from the records service',
    'local-file': 'from a local file — not a verified record',
    client: 'from the browser client',
    url: 'from this URL',
  }[source];
  return h('p', { class: `provenance provenance-${source}` }, label, ': ', h('span', null, detail));
}

/**
 * A digest, rendered so it can be read and compared by eye.
 *
 * Full value in the title and in the copy target; the eye gets the first and
 * last four. A truncated digest that cannot be recovered is worse than no
 * digest, so the full value is always one selection away.
 *
 * @param {string} hex16
 * @param {string} [label]
 */
export function digest(hex16, label) {
  return h('code', { class: 'digest', title: label ? `${label}: ${hex16}` : hex16 },
    hex16.slice(0, 4), h('span', { class: 'digest-mid' }, hex16.slice(4, 12)), hex16.slice(12),
  );
}

/**
 * A definition row for the evidence tables.
 *
 * @param {string} term
 * @param {Node|string|null} value
 * @param {string} [note]
 */
export function fact(term, value, note) {
  return h('div', { class: 'fact' },
    h('dt', null, term),
    h('dd', null, value ?? h('span', { class: 'unknown' }, 'unknown'), note ? h('small', null, note) : null),
  );
}

/** @param {number} n */
export function bytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MiB`;
}

/**
 * A page heading with an optional subtitle and actions.
 *
 * @param {object} o
 * @param {string} o.title
 * @param {Node|string|null} [o.sub]
 * @param {Node[]} [o.actions]
 */
export function pageHead({ title, sub, actions }) {
  return h('header', { class: 'page-head' },
    h('div', null, h('h1', null, title), sub ? h('p', { class: 'sub' }, sub) : null),
    actions && actions.length ? h('div', { class: 'actions' }, ...actions) : null,
  );
}

/**
 * The site's primary action link. Uses a real `<a href>` so middle-click,
 * copy-link-address and open-in-new-tab all work — a durable link that only
 * works when left-clicked is not durable.
 *
 * @param {string} href
 * @param {string} text
 * @param {{kind?: 'primary'|'secondary', title?: string}} [o]
 */
export function action(href, text, o = {}) {
  return h('a', { class: `button button-${o.kind ?? 'secondary'}`, href, title: o.title }, text);
}

/**
 * Copy a URL to the clipboard, showing the URL itself either way.
 *
 * The point of the web surface is that a link can be sent to someone, so the
 * link is always *visible* and selectable; the clipboard is a shortcut, not
 * the mechanism.
 *
 * @param {string} path an absolute site path
 * @param {string} label
 */
export function shareLink(path, label) {
  const url = new URL(path, location.origin).toString();
  const status = h('span', { class: 'share-status' });
  return h('div', { class: 'share' },
    h('span', { class: 'share-label' }, label),
    h('input', { class: 'share-url', value: url, readonly: true, spellcheck: 'false',
      onclick: (/** @type {Event} */ e) => /** @type {HTMLInputElement} */ (e.currentTarget).select() }),
    h('button', { class: 'button button-secondary', onclick: async () => {
      try {
        await navigator.clipboard.writeText(url);
        status.textContent = 'copied';
      } catch {
        status.textContent = 'select the box and copy';
      }
    } }, 'copy'),
    status,
  );
}

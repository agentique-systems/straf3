// @ts-check
/**
 * `/play/<map>` — a map URL launches that map.
 *
 * Criterion 15, and the whole point of it is what is *not* here: there is no
 * menu, no map picker, no "click to start". The page loads, the client loads,
 * the map the URL names is the map that loads, and the player is in it. The
 * only click is the one that takes pointer lock, and that click is required —
 * a page that grabbed the mouse on load would be hostile, and the browser would
 * refuse it anyway without a user gesture (URLS.md §4 behaviour 2).
 *
 * Three refusals are kept apart here, because conflating them would be the
 * substitution r3 forbids:
 *
 *  - **A pinned physics digest the build cannot honour → refuse.** The client
 *    says so through `onStatus("refused", …)` naming both digests, and the page
 *    puts that message where it cannot be missed. It does not run the nearest
 *    thing: a run produced under physics the URL did not name is not the run
 *    the link promised.
 *  - **A ghost that will not resolve → play anyway**, with the failure stated
 *    (URLS.md §4 behaviour 4). A missing ghost is not a reason to refuse a map.
 *  - **No records service → play anyway.** Playing needs the map and the
 *    physics; only *ranking* needs the service. The page says the run will not
 *    be rankable rather than refusing to start.
 */

import * as api from '../api.js';
import * as clientBridge from '../client.js';
import { h, absent, action, provenance, digest as digestEl, bytes as fmtBytes } from '../ui.js';
import { formatTime } from '../s3d.js';
import { href, categoryText } from '../router.js';

export const immersive = true;

/** @param {any} route */
export function title(route) {
  return `play ${route.map} — straf3`;
}

/**
 * @param {any} route
 * @param {HTMLElement} host
 * @param {{alive: () => boolean, onTeardown: (fn: () => void) => void}} ctx
 */
export async function render(route, host, ctx) {
  const slug = route.map;
  /** @type {import('../router.js').Category|null} */
  const urlCategory = route.category;

  const canvas = /** @type {HTMLCanvasElement} */ (h('canvas', { id: 'straf3-canvas', tabindex: '0' }));
  const status = h('p', { class: 'stage-status' }, 'loading the browser client…');
  const overlay = h('div', { class: 'stage-overlay' }, status);
  const hint = h('p', { class: 'stage-hint' }, 'WASD move · mouse look · Space jump · click to capture · Esc to release');
  const stage = h('div', { class: 'stage' }, canvas, overlay, hint);

  const rankState = h('span', { class: 'mono' }, 'checking…');
  const bar = h('div', { class: 'stage-bar' },
    h('strong', null, slug),
    h('span', { class: 'mono' }, urlCategory ? categoryText(urlCategory) : 'default physics'),
    route.ghost ? h('span', { class: 'mono' }, `ghost ${route.ghost}`) : null,
    h('span', { class: 'spacer' }),
    rankState,
    h('a', { href: href.map(slug, urlCategory), class: 'button' }, 'record book'),
  );

  host.append(bar, stage);

  const stopFit = clientBridge.fitCanvas(canvas);
  ctx.onTeardown(stopFit);

  /** @param {'loading'|'ready'|'error'|'refused'} kind @param {Node|string} node */
  const say = (kind, node) => {
    overlay.hidden = false;
    overlay.replaceChildren(typeof node === 'string'
      ? h('p', { class: 'stage-status' }, node)
      : node);
    if (kind === 'ready') overlay.hidden = true;
  };

  // ── what the URL names, and where it comes from ───────────────────────────
  //
  // The map's `.map` is served from this origin at /assets/maps/<slug>.map —
  // the dev server's read-only mount, contracts §A. The records service names
  // the same URL in `source_url`; when it can answer, its answer is used, and
  // when it cannot, the mount path is not a guess about the world, it is where
  // the mount is.
  const detail = await api.map(slug);
  if (!ctx.alive()) return;

  const sourceUrl = detail.status === 'ok' && typeof detail.data?.source_url === 'string'
    ? detail.data.source_url
    : `/assets/maps/${slug}.map`;

  /** @type {{family: string, digest: string|null}|null} */
  let physics = urlCategory;
  /** @type {string|null} */
  let physicsNote = null;

  if (!physics && detail.status === 'ok' && typeof detail.data?.default_category === 'string') {
    const [family, pinned] = String(detail.data.default_category).split('@');
    physics = { family, digest: pinned ?? null };
    physicsNote = `no ?p= in the URL, so this is ${slug}'s default category as the records service reports it`;
  } else if (!physics) {
    physicsNote = 'no ?p= in the URL and no records service to name a default — the client uses its build\'s profile and reports which';
  }

  // ── the attempt ticket ────────────────────────────────────────────────────
  /** @type {string|null} */
  let ticket = null;
  if (!api.token()) {
    rankState.replaceChildren(h('span', { class: 'unknown' }, 'not signed in — this run will not be ranked'));
  } else if (physics) {
    const got = await api.attempt(slug, physics);
    if (!ctx.alive()) return;
    if (got.status === 'ok' && typeof got.data?.ticket === 'string') {
      ticket = got.data.ticket;
      rankState.replaceChildren(h('span', { class: 'ok' }, 'ticket held — a finished run can be ranked'));
    } else {
      const why = got.status === 'ok' ? 'the service returned no ticket' : got.detail;
      rankState.replaceChildren(h('span', { class: 'unknown' }, 'no ticket — this run will not be ranked'));
      rankState.title = why;
    }
  }

  // ── the ghost, which degrades ─────────────────────────────────────────────
  /** @type {string|null} */
  let ghostUrl = null;
  /** @type {string|null} */
  let ghostProblem = null;
  if (route.ghost) {
    const ghost = await api.run(route.ghost);
    if (!ctx.alive()) return;
    if (ghost.status === 'ok' && typeof ghost.data?.demo === 'string') {
      ghostUrl = ghost.data.demo;
    } else {
      ghostProblem = ghost.status === 'ok'
        ? `run ${route.ghost} has no downloadable recording yet — a demo is only served once the run is verified`
        : `run ${route.ghost} could not be resolved: ${ghost.detail}`;
    }
  }

  // ── the callbacks, then launch ────────────────────────────────────────────
  const results = h('div', null);

  clientBridge.installHooks({
    onStatus: (s) => {
      if (!ctx.alive()) return;
      if (s.kind === 'refused') {
        say('refused', absent({
          kind: 'error',
          what: 'This build cannot honour what the URL asked for.',
          why: s.message,
          next:
            'It is not running the nearest thing instead. A run produced under physics the URL ' +
            'did not name is not the run this link promised.',
        }));
      } else if (s.kind === 'error') {
        say('error', absent({ kind: 'error', what: 'The client reported a problem.', why: s.message }));
      } else if (s.kind === 'ready') {
        say('ready', '');
      } else {
        say('loading', s.message || 'loading…');
      }
    },

    onPointerLock: (locked) => {
      hint.textContent = locked
        ? 'Esc releases the mouse'
        : 'WASD move · mouse look · Space jump · click to capture · Esc to release';
    },

    onRunFinished: (run) => {
      if (!ctx.alive()) return;
      results.replaceChildren(runFinished(run, { ticket, slug, physics }));
      overlay.hidden = false;
      overlay.replaceChildren(results);
    },
  });

  const launched = await clientBridge.launch({
    mode: 'play',
    canvas_id: 'straf3-canvas',
    map: { slug, source_url: sourceUrl },
    physics: physics ?? undefined,
    ghost_url: ghostUrl,
  });
  if (!ctx.alive()) return;

  if (!launched.ok) {
    say('error', h('div', null,
      absent({
        kind: launched.kind === 'no-bundle' ? 'unavailable' : 'error',
        what: launched.kind === 'no-bundle'
          ? 'The browser client has not been built.'
          : launched.kind === 'no-webgpu'
            ? 'This browser will not give straf3 a GPU adapter.'
            : 'The browser client failed to start.',
        why: launched.why,
        next: launched.kind === 'no-bundle'
          ? 'Everything else on this URL is correct: the map, the physics and the ghost are resolved and waiting.'
          : undefined,
      }),
      urlFacts({ slug, sourceUrl, physics, physicsNote, ghostUrl, ghostProblem, ghostRef: route.ghost }),
    ));
    return;
  }

  if (ghostProblem) {
    say('error', h('div', null,
      absent({
        kind: 'unavailable',
        what: 'Playing without a ghost.',
        why: ghostProblem,
        next: 'The map itself is unaffected — a missing ghost is not a reason to refuse it.',
      }),
      h('div', { class: 'actions' },
        h('button', { class: 'button button-primary', onclick: () => { overlay.hidden = true; } }, 'play anyway')),
    ));
  }
}

/**
 * What this URL resolved to. Shown when the client cannot start, because the
 * routing half of the page is still correct and worth seeing.
 *
 * @param {object} o
 */
function urlFacts({ slug, sourceUrl, physics, physicsNote, ghostUrl, ghostProblem, ghostRef }) {
  const row = (/** @type {string} */ t, /** @type {any} */ v, /** @type {string} [n]*/ n) =>
    h('div', { class: 'fact' },
      h('dt', null, t),
      h('dd', null, v ?? h('span', { class: 'unknown' }, 'unknown'), n ? h('small', null, n) : null));

  return h('div', { style: 'text-align:left;margin-top:1.2rem' },
    h('h3', null, 'what this URL resolved to'),
    h('dl', { class: 'facts' },
      row('map', slug),
      row('map source', h('a', { href: sourceUrl }, sourceUrl)),
      row('physics', physics ? categoryText(physics) : null, physicsNote ?? undefined),
      row('pinned', physics?.digest ? 'yes — the client must refuse if it cannot honour it' : 'no — the family follows the current profile'),
      row('ghost', ghostRef ? (ghostUrl ?? h('span', { class: 'bad' }, ghostProblem ?? 'unresolved')) : 'none'),
    ),
  );
}

/**
 * A finished run.
 *
 * The digest is on screen, on `globalThis.straf3.lastRun`, on the console, and
 * in `data-run-digest` here. It is the same value four ways on purpose: the
 * native comparison harness needs it from a channel that is not the `.s3d`
 * header it is checking (r6), and a value that only exists inside the file
 * proves nothing about the code path that wrote the file.
 *
 * @param {import('../client.js').FinishedRun} run
 * @param {{ticket: string|null, slug: string, physics: any}} o
 */
function runFinished(run, { ticket, slug }) {
  const blob = new Blob([run.s3d], { type: 'application/vnd.straf3.demo' });
  const downloadUrl = URL.createObjectURL(blob);
  const fileName = `${slug}-${run.run_digest_hex16}.s3d`;

  const submitState = h('p', { class: 'pending' }, ticket ? 'submitting…' : '');

  const box = h('div', {
    class: 'run-result',
    id: 'straf3-run-result',
    dataset: {
      runDigest: run.run_digest_hex16,
      timeMs: String(run.time_ms),
      s3dBytes: String(run.s3d.length),
    },
  },
    h('h3', null, 'Run finished'),
    h('dl', { class: 'facts' },
      h('div', { class: 'fact' }, h('dt', null, 'time'), h('dd', null,
        formatTime(run.time_ms), h('small', null, 'the client\'s claim — the service computes its own'))),
      h('div', { class: 'fact' }, h('dt', null, 'run digest'), h('dd', null,
        digestEl(run.run_digest_hex16, 'run digest'),
        h('small', null, 'reported by the browser, out of band from the .s3d header'))),
      h('div', { class: 'fact' }, h('dt', null, 'recording'), h('dd', null, fmtBytes(run.s3d.length))),
    ),
    h('div', { class: 'actions' },
      h('a', { class: 'button button-primary', href: downloadUrl, download: fileName }, 'download the .s3d'),
      h('a', { class: 'button', href: href.play(slug) }, 'run it again'),
    ),
    submitState,
  );

  if (!ticket) {
    submitState.replaceChildren();
    box.append(absent({
      kind: 'empty',
      what: 'Not submitted.',
      why:
        'Submitting needs a signed-in attempt ticket and there is none. The recording above is ' +
        'real and complete — download it, and it can be claimed later.',
    }));
    return box;
  }

  void (async () => {
    const res = await api.submitRun({
      ticket,
      time_ms: run.time_ms,
      run_digest_hex16: run.run_digest_hex16,
      s3d: run.s3d,
    });
    if (res.status !== 'ok') {
      submitState.replaceChildren();
      box.append(absent({
        kind: res.status === 'absent' ? 'unavailable' : 'error',
        what: 'The run was not submitted.',
        why: res.status === 'absent' ? res.detail : `${res.detail} (HTTP ${res.code}${res.error ? `, ${res.error}` : ''})`,
        next: 'The recording is downloadable above and nothing about it is lost.',
      }));
      return;
    }
    const serverDigest = res.data?.run_digest ?? null;
    const agrees = serverDigest !== null && String(serverDigest) === run.run_digest_hex16;
    submitState.replaceChildren();
    box.append(
      h('p', null, 'Accepted, and queued for verification. Nothing has re-simulated it yet.'),
      h('dl', { class: 'facts' },
        h('div', { class: 'fact' }, h('dt', null, 'the service\'s digest'), h('dd', null,
          serverDigest ? digestEl(String(serverDigest)) : h('span', { class: 'unknown' }, 'unknown'),
          h('small', null, agrees
            ? 'matches what the browser reported out of band'
            : 'does NOT match what the browser reported — worth investigating'))),
      ),
      res.data?.watch ? h('div', { class: 'actions' }, action(String(res.data.watch), 'watch it back')) : null,
      provenance('service', 'POST /v1/runs'),
    );
  })();

  return box;
}

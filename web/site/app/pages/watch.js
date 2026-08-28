// @ts-check
/**
 * `/watch/<run>` — a record URL plays that record back.
 *
 * The other half of criterion 15. What is played back is **the recording,
 * re-simulated** — not a stored path, not an interpolated camera track. The
 * `.s3d` is a command stream; running it through the same deterministic
 * simulation the run was produced by is the only playback that shows what
 * actually happened.
 *
 * The consequence for this page is a discipline about where the map and the
 * physics come from: **the recording's own header, never the URL and never the
 * current defaults** (URLS.md §4 behaviour 2). So the config handed to the
 * client in watch mode carries a recording URL and a seek, and deliberately
 * carries no map and no physics — there is nothing for the site to get wrong,
 * because the site is not the one that knows. A `.s3d` carries a `WorldId` and
 * a `PhysicsId` precisely so this cannot go wrong.
 *
 * `?t=` is a seek *hint*, in milliseconds like every other duration here. A
 * client that cannot seek starts at zero and says so; it does not silently
 * pretend it seeked.
 */

import * as api from '../api.js';
import * as records from '../records.js';
import * as clientBridge from '../client.js';
import { h, absent, action, provenance, digest as digestEl, bytes as fmtBytes } from '../ui.js';
import { formatTime, hex64 } from '../s3d.js';
import { href, isUuid } from '../router.js';
import * as router from '../router.js';

export const immersive = true;

/** @param {any} route */
export function title(route) {
  return `watch ${route.run} — straf3`;
}

/**
 * @param {any} route
 * @param {HTMLElement} host
 * @param {{alive: () => boolean, onTeardown: (fn: () => void) => void}} ctx
 */
export async function render(route, host, ctx) {
  const runRef = route.run;
  const seekMs = route.seekMs ?? 0;

  const canvas = /** @type {HTMLCanvasElement} */ (h('canvas', { id: 'straf3-canvas', tabindex: '0' }));
  const status = h('p', { class: 'stage-status' }, `resolving ${runRef}…`);
  const overlay = h('div', { class: 'stage-overlay' }, status);
  const stage = h('div', { class: 'stage' }, canvas, overlay);

  const label = h('span', { class: 'mono' }, runRef);
  const detail = h('span', { class: 'mono' }, '');
  const bar = h('div', { class: 'stage-bar' },
    h('strong', null, 'watch'),
    label,
    seekMs ? h('span', { class: 'mono' }, `from ${formatTime(seekMs)}`) : null,
    detail,
    h('span', { class: 'spacer' }),
    h('a', { href: href.record(runRef), class: 'button' }, 'the record'),
  );

  host.append(bar, stage);

  const stopFit = clientBridge.fitCanvas(canvas);
  ctx.onTeardown(stopFit);

  /** @param {Node|string} node */
  const say = (node) => {
    overlay.hidden = false;
    overlay.replaceChildren(typeof node === 'string' ? h('p', { class: 'stage-status' }, node) : node);
  };

  const res = await records.resolve(runRef);
  if (!ctx.alive()) return;

  // Same canonicalisation as the record page: a UUID resolves and then the
  // address bar holds the digest, which is the spelling that survives a
  // database restore (URLS.md §5).
  if (isUuid(runRef)) {
    const canonical = res.record?.run_digest ?? res.recording?.runDigest ?? null;
    if (canonical && /^[0-9a-f]{16}$/.test(String(canonical))) {
      router.go(href.watch(String(canonical), route.seekMs), { replace: true });
      return;
    }
  }

  // ── where the bytes the client will re-simulate come from ─────────────────
  //
  // The client fetches the recording itself, from a URL on this origin. Two
  // sources can supply one, and they are not equivalent: a verified run from
  // the records service, or a local .s3d served by the dev server. Both are
  // real command streams and both re-simulate identically; only one of them has
  // ever been checked by anything, and the page says which.
  /** @type {string|null} */
  let recordingUrl = null;
  /** @type {'service'|'local-file'|null} */
  let source = null;

  if (res.record && typeof res.record.demo === 'string') {
    recordingUrl = res.record.demo;
    source = 'service';
  } else if (res.recording?.source === 'local-file') {
    recordingUrl = `/dev/runs/${encodeURIComponent(res.recording.origin.replace(/^runs\//, ''))}`;
    source = 'local-file';
  }

  if (res.recording) detail.replaceChildren(headerLine(res.recording));

  if (!recordingUrl) {
    const why = res.record && res.record.demo === null
      ? `This run exists but its recording is not downloadable yet: a demo is only served once the run is verified (status is "${res.record.status}").`
      : 'Neither the records service nor a local .s3d could supply the bytes for this run.';
    say(h('div', null,
      absent({
        kind: 'unknown',
        what: 'There is no recording to play back.',
        why,
        next: 'Playback re-simulates the recording, so without the bytes there is nothing to run — and nothing worth faking.',
      }),
      h('ul', { class: 'notes' }, ...res.notes.map((n) => h('li', null, n))),
      res.recording ? headerFacts(res.recording) : null,
    ));
    return;
  }

  clientBridge.installHooks({
    onStatus: (s) => {
      if (!ctx.alive()) return;
      if (s.kind === 'refused') {
        say(h('div', null, absent({
          kind: 'error',
          what: 'This build cannot honour the recording\'s identity.',
          why: s.message,
          next:
            'Playback is refused rather than approximated. A ghost replayed against geometry ' +
            'that moved, or physics that was tuned, shows a run that never happened.',
        }), res.recording ? headerFacts(res.recording) : null));
      } else if (s.kind === 'error') {
        say(absent({ kind: 'error', what: 'The client reported a problem.', why: s.message }));
      } else if (s.kind === 'ready') {
        overlay.hidden = true;
      } else {
        say(s.message || 'loading…');
      }
    },
  });

  say('loading the browser client…');

  const launched = await clientBridge.launch({
    mode: 'watch',
    canvas_id: 'straf3-canvas',
    recording_url: recordingUrl,
    seek_ms: seekMs,
  });
  if (!ctx.alive()) return;

  if (!launched.ok) {
    say(h('div', null,
      absent({
        kind: launched.kind === 'no-bundle' ? 'unavailable' : 'error',
        what: launched.kind === 'no-bundle'
          ? 'The browser client has not been built, so nothing can re-simulate this.'
          : launched.kind === 'no-webgpu'
            ? 'This browser will not give straf3 a GPU adapter.'
            : 'The browser client failed to start.',
        why: launched.why,
        next: 'The recording itself resolved, and everything below was read out of its bytes.',
      }),
      h('p', { class: 'pin-note' },
        'Recording: ', h('a', { href: recordingUrl, class: 'mono' }, recordingUrl),
        source === 'local-file' ? ' — a local file, not a verified record' : ''),
      res.recording ? headerFacts(res.recording) : null,
    ));
  }
}

/**
 * The one-line summary in the bar: what this recording says it is.
 *
 * @param {import('../records.js').LoadedRecording} rec
 */
function headerLine(rec) {
  const d = rec.decoded;
  const world = d.world.kind === 'map' ? d.world.name : d.world.kind;
  return h('span', null,
    `${world} · ${d.physicsName} · ${d.commandCount} cmd · `,
    d.runTimeMs === null ? 'unfinished' : formatTime(d.runTimeMs),
    rec.source === 'local-file' ? h('span', { class: 'unknown' }, ' · local file') : null,
  );
}

/**
 * The recording's header, decoded in this browser.
 *
 * Shown when playback cannot happen, because these are real facts read out of
 * real bytes and they are the most useful thing the page still has.
 *
 * @param {import('../records.js').LoadedRecording} rec
 */
function headerFacts(rec) {
  const d = rec.decoded;
  const row = (/** @type {string} */ t, /** @type {any} */ v) =>
    h('div', { class: 'fact' }, h('dt', null, t),
      h('dd', null, v ?? h('span', { class: 'unknown' }, 'unknown')));

  return h('div', { style: 'text-align:left;margin-top:1.2rem' },
    h('h3', null, 'the recording, decoded here'),
    h('dl', { class: 'facts' },
      row('run digest', digestEl(rec.runDigest, 'run digest')),
      row('world', d.world.kind === 'map'
        ? `${d.world.name} (collision ${hex64(d.world.collisionDigest)})`
        : d.world.kind),
      row('physics', `${d.physicsName} — ${hex64(d.physicsDigest)}`),
      row('commands', `${d.commandCount} at ${d.rateHz} Hz`),
      row('claims time', d.runTimeMs === null ? null : formatTime(d.runTimeMs)),
      row('size', fmtBytes(d.byteLength)),
      row('content digest', d.contentDigestOk
        ? h('span', { class: 'ok' }, 'verified in this browser')
        : h('span', { class: 'bad' }, 'does not match the bytes')),
    ),
    provenance(rec.source === 'service' ? 'service' : 'local-file', rec.origin),
    h('div', { class: 'actions' }, action(href.record(rec.runDigest), 'the record page')),
  );
}

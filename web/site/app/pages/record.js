// @ts-check
/**
 * `/r/<run>` — a record, as evidence.
 *
 * This page has one job beyond displaying numbers: **keep straight which of
 * them are facts and which are claims.**
 *
 *  - A time from the records service was *computed* by re-simulating the
 *    recording. It is the ranked time.
 *  - `client_time_ms` is what the browser displayed to the player. It is a
 *    diagnostic, never the ranked time, and it is labelled as the client's
 *    claim wherever it appears (ARCHITECTURE §7.2 step 5).
 *  - Header fields read from a local `.s3d` are real — the map, the physics
 *    digest, the command count, the content digest, which this page re-checks
 *    in the browser — and none of them is a verified record, because nothing
 *    verified it.
 *
 * The other job is the pin. **Every link out of this page is pinned to the
 * record's own physics digest** (URLS.md §3): a record is bound to the
 * constants it was set under, so "the board this belongs to" and "play under
 * this record's physics" both name the digest rather than the family. Only
 * navigation that means "current" emits the unpinned form, and nothing on this
 * page means current.
 */

import * as api from '../api.js';
import * as records from '../records.js';
import { h, absent, action, pageHead, provenance, digest as digestEl, shareLink, bytes } from '../ui.js';
import { formatTime, hex64 } from '../s3d.js';
import { href, isUuid, categoryText } from '../router.js';
import * as router from '../router.js';

/** @param {any} route */
export function title(route) {
  return `record ${route.run} — straf3`;
}

/**
 * @param {any} route
 * @param {HTMLElement} host
 * @param {{alive: () => boolean}} ctx
 */
export async function render(route, host, ctx) {
  const runRef = route.run;

  const head = pageHead({
    title: 'Record',
    sub: h('code', null, runRef),
  });
  const body = h('div', null, h('p', { class: 'pending' }, `resolving ${runRef}…`));
  host.append(head, body);

  const res = await records.resolve(runRef);
  if (!ctx.alive()) return;

  // URLS.md §5: the site canonicalises a UUID URL to the digest form. The
  // digest is computable from the file alone and survives a database restore;
  // the UUID survives none of that, so the durable spelling is what ends up in
  // the address bar and in anything copied out of it.
  if (isUuid(runRef)) {
    const canonical = res.record?.run_digest ?? res.recording?.runDigest ?? null;
    if (canonical && /^[0-9a-f]{16}$/.test(String(canonical))) {
      router.go(href.record(String(canonical)), { replace: true });
      return;
    }
  }

  if (res.source === 'none') {
    body.replaceChildren(
      absent({
        kind: 'unknown',
        what: 'No recording with this name could be found.',
        why:
          'A run can be resolved from the records service or from a local .s3d served by the ' +
          'dev server. Neither had it. That is not the same as the run not existing.',
      }),
      h('ul', { class: 'notes' }, ...res.notes.map((n) => h('li', null, n))),
    );
    return;
  }

  const parts = h('div', null);
  body.replaceChildren(parts);

  if (res.record) parts.append(verified(res.record));
  if (res.recording) parts.append(recordingFacts(res.recording, res.record));

  parts.append(
    h('section', null,
      h('h2', null, 'links'),
      links(res),
      shareLink(href.record(res.record?.run_digest ?? res.recording?.runDigest ?? runRef), 'this record')),
    res.notes.length
      ? h('ul', { class: 'notes' }, ...res.notes.map((n) => h('li', null, n)))
      : null,
  );
}

/**
 * The service's verdict. Status first, because it decides what every other
 * number on the page means.
 *
 * @param {any} r
 */
function verified(r) {
  const status = String(r?.status ?? 'unknown');
  const rankedTime = Number.isFinite(r?.time_ms) ? r.time_ms : null;
  const d = r?.diagnostics ?? {};

  /** @type {Record<string, {kind: 'empty'|'unavailable'|'unknown'|'error', what: string, why: string}>} */
  const verdicts = {
    pending: {
      kind: 'empty',
      what: 'Not verified yet.',
      why: 'The run is queued. Nothing has re-simulated it, so it has no ranked time.',
    },
    did_not_finish: {
      kind: 'empty',
      what: 'This run did not finish.',
      why: 'Re-simulation ran the whole command stream and it never crossed the finish trigger.',
    },
    rejected: {
      kind: 'error',
      what: 'Rejected.',
      why: r?.reject_reason ?? 'the service rejected this run and gave no reason',
    },
    divergent: {
      kind: 'error',
      what: 'The browser and the server disagreed.',
      why:
        'Re-simulating this recording produced a different rolling digest than the recording ' +
        'carries. That is a determinism finding, not a cheating verdict — it means two builds ' +
        'that should agree did not.',
    },
    error: {
      kind: 'error',
      what: 'Verification failed.',
      why: r?.reject_reason ?? 'the verifier errored on this run',
    },
  };

  const verdict = verdicts[status];

  return h('section', null,
    h('h2', null, 'the record'),
    verdict ? absent(verdict) : null,

    status === 'verified'
      ? h('p', { class: 'lede' },
          h('strong', { style: 'font-size:1.6rem;font-family:var(--mono)' },
            rankedTime === null ? 'unknown' : formatTime(rankedTime)),
          ' — computed by re-simulating this recording, not accepted from it.')
      : null,

    h('dl', { class: 'facts' },
      row('status', status),
      row('ranked time', rankedTime === null ? null : formatTime(rankedTime),
        'server-computed; null unless verified'),
      row('player', r?.player?.display_name ?? null),
      row('map', r?.map?.slug ?? null),
      row('category', r?.category?.key ?? (r?.category ? categoryText({
        family: r.category.family, digest: r.category.digest ?? null,
      }) : null)),
      row('commands', Number.isFinite(r?.commands) ? String(r.commands) : null),
      row('tick rate', Number.isFinite(r?.tick_rate_hz) ? `${r.tick_rate_hz} Hz` : null),
      row('run digest', r?.run_digest ? digestEl(String(r.run_digest), 'run digest') : null),
      row('submitted', r?.submitted_at ?? null),
      row('verified', r?.verified_at ?? null),
    ),

    h('h3', { style: 'margin-top:1.2rem' }, 'diagnostics'),
    h('p', { class: 'pin-note' },
      'None of these is the ranked time. They exist to tell "we agree" from "we disagree" ',
      '— a mismatch is a determinism bug report, not a verdict about a player.'),
    h('dl', { class: 'facts' },
      row('client\'s claimed time', Number.isFinite(d?.client_time_ms) ? formatTime(d.client_time_ms) : null,
        'what the browser displayed — never ranked, never compared against'),
      row('client rolling digest', d?.client_rolling_digest ? digestEl(String(d.client_rolling_digest)) : null),
      row('server rolling digest', d?.server_rolling_digest ? digestEl(String(d.server_rolling_digest)) : null),
      row('agreement', agreement(d)),
      row('first divergence', Number.isFinite(d?.divergence_at) ? `command ${d.divergence_at}` : null,
        'the first command the two builds disagreed on'),
      row('sim build', d?.sim_build ?? null),
    ),
    provenance('service', `GET /v1/runs/by-digest/${r?.run_digest ?? '…'}`),
  );
}

/** @param {any} d */
function agreement(d) {
  const a = d?.client_rolling_digest;
  const b = d?.server_rolling_digest;
  if (!a || !b) return null;
  return String(a) === String(b)
    ? h('span', { class: 'ok' }, 'identical at every command')
    : h('span', { class: 'bad' }, 'the two builds produced different state');
}

/**
 * The recording itself, decoded here in the browser.
 *
 * `contentDigestOk` and `traceMatchesDigest` are recomputed locally, so they are
 * facts this page established rather than claims it was handed. They still say
 * nothing about whether the run is *valid* — that needs a simulation, and this
 * is a parser.
 *
 * @param {import('../records.js').LoadedRecording} rec
 * @param {any} serviceRecord
 */
function recordingFacts(rec, serviceRecord) {
  const d = rec.decoded;
  const world = d.world.kind === 'map'
    ? `${d.world.name} (collision ${hex64(d.world.collisionDigest)})`
    : d.world.kind;

  return h('section', null,
    h('h2', null, 'the recording'),

    rec.source === 'local-file' && !serviceRecord
      ? absent({
          kind: 'unavailable',
          what: 'This is a local file, not a record.',
          why:
            'These header fields are real and the content digest below was re-checked in this ' +
            'browser. What the file does not have is a verification result, because nothing ' +
            'verified it. The time it claims is the client\'s claim.',
        })
      : null,

    h('dl', { class: 'facts' },
      row('run digest', digestEl(rec.runDigest, 'run digest'), 'folded over every command\'s state checksum'),
      row('claims time', d.runTimeMs === null ? null : formatTime(d.runTimeMs),
        rec.source === 'local-file' ? 'the client\'s claim — nothing has verified it' : undefined),
      row('finished', d.finished ? 'crossed both triggers' : 'never crossed the finish trigger'),
      row('commands', String(d.commandCount)),
      row('command rate', `${d.rateHz} Hz`),
      row('sim time', formatTime(d.simTimeMs), 'exact integer sum of every command duration'),
      row('world', world),
      row('physics', `${d.physicsName} — ${hex64(d.physicsDigest)}`),
      row('size', bytes(d.byteLength)),
      row('content digest', d.contentDigestOk
        ? h('span', { class: 'ok' }, 'verified in this browser')
        : h('span', { class: 'bad' }, 'DOES NOT MATCH the bytes'),
        'FNV-1a over everything preceding it'),
      row('checksum trace', d.trace === null
        ? 'not recorded'
        : d.traceMatchesDigest
          ? h('span', { class: 'ok' }, `${d.trace.length} checksums, folding to the claimed run digest`)
          : h('span', { class: 'bad' }, 'folds to a different digest than the header claims'),
        d.trace === null ? 'without it a divergence cannot be localised to a command' : undefined),
    ),
    provenance(rec.source === 'service' ? 'service' : 'local-file', rec.origin),
  );
}

/**
 * Every link out of a record is pinned to the record's physics digest.
 *
 * @param {import('../records.js').Resolution} res
 */
function links(res) {
  const rec = res.recording;
  const r = res.record;

  const runRef = r?.run_digest ?? rec?.runDigest ?? null;
  const mapSlug = r?.map?.slug ?? (rec && rec.decoded.world.kind === 'map' ? rec.decoded.world.name : null);

  // The digest comes from the record, never from the current defaults.
  const family = r?.category?.family ?? rec?.decoded.physicsName ?? null;
  const physDigest = r?.category?.digest ?? (rec ? hex64(rec.decoded.physicsDigest) : null);
  /** @type {import('../router.js').Category|null} */
  const pinned = family && physDigest ? { family: String(family), digest: String(physDigest) } : null;

  const items = [];
  if (runRef) items.push(action(href.watch(String(runRef)), 'watch this run', { kind: 'primary' }));
  if (mapSlug && pinned) {
    items.push(action(href.map(String(mapSlug), pinned), 'the board this belongs to',
      { title: `frozen to physics ${pinned.digest}` }));
    items.push(action(href.play(String(mapSlug), { category: pinned, ghost: runRef ? String(runRef) : null }),
      'play it, racing this ghost',
      { title: `under exactly physics ${pinned.digest}` }));
  }

  return h('div', null,
    h('div', { class: 'actions' }, ...items),
    pinned
      ? h('p', { class: 'pin-note' },
          'These links pin physics ', digestEl(pinned.digest),
          ' rather than naming ', h('code', null, pinned.family),
          '. A record is bound to the constants it was set under, so a link out of it that ',
          'meant "whatever cpm is today" would stop pointing at this run\'s board the moment ',
          'cpm was tuned.')
      : h('p', { class: 'pin-note' },
          'The physics this run was set under is not known, so no pinned link can be built for it.'),
  );
}

/**
 * @param {string} term
 * @param {any} value
 * @param {string} [note]
 */
function row(term, value, note) {
  return h('div', { class: 'fact' },
    h('dt', null, term),
    h('dd', null,
      value === null || value === undefined || value === ''
        ? h('span', { class: 'unknown' }, 'unknown')
        : (value instanceof Node ? value : String(value)),
      note ? h('small', null, note) : null),
  );
}

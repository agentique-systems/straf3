// @ts-check
/**
 * Resolving `<run>` to an actual recording, from whichever source has one.
 *
 * Two sources exist and they are not equivalent:
 *
 *  1. **The records service.** A run it returns has been re-simulated
 *     server-side; its `time_ms` was *computed*, not accepted, and its
 *     verification verdict is a fact about the run.
 *  2. **A local `.s3d` file**, served by the dev server from `runs/`. Its
 *     header fields are real — the map it ran against, the physics digest, the
 *     command count, the time the recording claims — and the content digest
 *     can be re-checked here in the browser. What it does *not* have is a
 *     verification result, because nothing verified it. The site must never
 *     present the claimed time from a local file as a record.
 *
 * The distinction rides on every value this module returns as `source`, and
 * the record page renders it. This is the honest version of "develop the page
 * before the service exists": use real data from a real file and label exactly
 * what it is, rather than invent a row.
 */

import * as api from './api.js';
import { decode, hex64 } from './s3d.js';

/**
 * @typedef {object} LocalFile
 * @property {string} name
 * @property {number} bytes
 */

/** @type {Promise<{files: LocalFile[], enabled: boolean, detail: string}>|null} */
let localIndexPromise = null;

/**
 * The dev server's `runs/` listing. Fetched once per page load.
 * @returns {Promise<{files: LocalFile[], enabled: boolean, detail: string}>}
 */
export function localIndex() {
  if (localIndexPromise) return localIndexPromise;
  localIndexPromise = (async () => {
    let res;
    try {
      res = await fetch('/dev/runs', { headers: { accept: 'application/json' } });
    } catch (err) {
      return { files: [], enabled: false, detail: `not reachable: ${err instanceof Error ? err.message : String(err)}` };
    }
    if (res.status === 403) {
      const body = await res.json().catch(() => null);
      return { files: [], enabled: false, detail: body?.detail ?? 'local recordings are disabled' };
    }
    if (!res.ok) return { files: [], enabled: false, detail: `HTTP ${res.status}` };
    const body = await res.json().catch(() => null);
    if (!body || !Array.isArray(body.files)) return { files: [], enabled: false, detail: 'unreadable listing' };
    return { files: body.files, enabled: true, detail: body.dir ?? 'runs/' };
  })();
  return localIndexPromise;
}

/**
 * @typedef {object} LoadedRecording
 * @property {'service'|'local-file'} source
 * @property {string} origin        a human-readable where-from
 * @property {Uint8Array} bytes
 * @property {import('./s3d.js').Decoded} decoded
 * @property {string} runDigest     16-hex, from the header
 */

/**
 * Read and decode one local `.s3d`.
 *
 * @param {string} name
 * @returns {Promise<LoadedRecording|{error: string}>}
 */
export async function loadLocal(name) {
  let res;
  try {
    res = await fetch(`/dev/runs/${encodeURIComponent(name)}`);
  } catch (err) {
    return { error: `could not read ${name}: ${err instanceof Error ? err.message : String(err)}` };
  }
  if (!res.ok) return { error: `could not read ${name}: HTTP ${res.status}` };
  const bytes = new Uint8Array(await res.arrayBuffer());
  try {
    const decoded = decode(bytes);
    return {
      source: 'local-file',
      origin: `runs/${name}`,
      bytes,
      decoded,
      runDigest: hex64(decoded.runDigest),
    };
  } catch (err) {
    return { error: `${name} did not decode: ${err instanceof Error ? err.message : String(err)}` };
  }
}

/**
 * Find a local recording whose run digest matches.
 *
 * Linear over the listing, decoding each header. The set is a handful of files
 * of a few tens of kilobytes; an index would be a cache to invalidate for no
 * measurable gain.
 *
 * @param {string} digest16
 * @returns {Promise<LoadedRecording|null>}
 */
export async function findLocalByDigest(digest16) {
  const index = await localIndex();
  if (!index.enabled) return null;
  for (const file of index.files) {
    const loaded = await loadLocal(file.name);
    if ('error' in loaded) continue;
    if (loaded.runDigest === digest16) return loaded;
  }
  return null;
}

/**
 * @typedef {object} Resolution
 * @property {'service'|'local-file'|'none'} source
 * @property {any|null} record            the service's run row, when there is one
 * @property {LoadedRecording|null} recording  the decoded `.s3d`, when we have bytes
 * @property {string[]} notes             what was tried, and what it said
 */

/**
 * Resolve a `<run>` reference to everything the site can learn about it.
 *
 * The service is asked first and is authoritative when it answers. A local
 * file is a fallback that supplies bytes and header facts and never a verdict.
 *
 * @param {string} runRef
 * @returns {Promise<Resolution>}
 */
export async function resolve(runRef) {
  /** @type {string[]} */
  const notes = [];

  const fromService = await api.run(runRef);
  if (fromService.status === 'ok') {
    const record = fromService.data;
    /** @type {LoadedRecording|null} */
    let recording = null;
    const bytes = await api.demo(runRef);
    if (bytes.status === 'ok') {
      try {
        const decoded = decode(bytes.data);
        recording = {
          source: 'service',
          origin: `GET /v1/runs/${runRef}/demo`,
          bytes: bytes.data,
          decoded,
          runDigest: hex64(decoded.runDigest),
        };
      } catch (err) {
        notes.push(`the service returned a .s3d that did not decode: ${err instanceof Error ? err.message : String(err)}`);
      }
    } else {
      notes.push(`the recording itself is not available: ${bytes.status === 'absent' ? bytes.detail : bytes.detail}`);
    }
    return { source: 'service', record, recording, notes };
  }

  // "The service said no" and "the service did not say" are different notes,
  // for the same reason an empty board and an unreachable one are different
  // pages. A 404 `unknown_run` is an answer — a complete, authoritative one —
  // and filing it under "could not answer" would make the service look broken
  // every time someone followed a link to a run it has never held.
  if (fromService.status === 'absent') {
    notes.push(`the records service was not asked: ${fromService.detail}`);
  } else if (fromService.code === 404 && fromService.error === 'unknown_run') {
    notes.push(`the records service answered, and it has no run by this name: ${fromService.detail}`);
  } else {
    notes.push(`the records service could not answer (HTTP ${fromService.code}): ${fromService.detail}`);
  }

  if (/^[0-9a-f]{16}$/.test(runRef)) {
    const local = await findLocalByDigest(runRef);
    if (local) {
      notes.push(`found a local recording with this run digest: ${local.origin}`);
      return { source: 'local-file', record: null, recording: local, notes };
    }
    const index = await localIndex();
    notes.push(
      index.enabled
        ? `no local recording in ${index.detail} carries this run digest`
        : `local recordings are not being served: ${index.detail}`,
    );
  } else {
    notes.push('a UUID can only be resolved by the records service — a local file is named by its run digest');
  }

  return { source: 'none', record: null, recording: null, notes };
}

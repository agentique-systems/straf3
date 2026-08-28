// @ts-check
/**
 * The bridge to the browser client, and the only place the site touches wasm.
 *
 * The division of labour is the wave contract §B, and it is worth restating
 * because it is what keeps two seats from reaching into each other: **the page
 * owns the DOM, the network and the sign-in token; the wasm module owns the
 * canvas, the simulation and the recording.** The client never calls `/v1` and
 * never sees a bearer token. The site never touches the simulation.
 *
 * They meet at exactly two places:
 *
 *  - `start_web(configJson)` — the site hands over a JSON config. JSON rather
 *    than positional arguments so the signature never has to change again.
 *  - `globalThis.straf3` — callbacks the site defines and the client calls.
 *    The client must not crash if one is absent, so nothing here is required.
 *
 * Two things this module does *before* entering wasm, both because doing them
 * inside is worse:
 *
 *  1. **The backend decision.** wgpu does not fall back from WebGPU to WebGL2;
 *     with `navigator.gpu` present but `requestAdapter()` returning null it
 *     crashes inside the WebGPU backend. So the page establishes that an
 *     adapter really exists and tells the module what it found. On a
 *     software-only host, Chrome needs `--enable-unsafe-webgpu
 *     --use-angle=swiftshader` before it will offer one.
 *  2. **The bundle probe.** `import()` of a missing module fails with a syntax
 *     or network error naming a URL, three layers from "the client has not
 *     been built". A HEAD first turns that into a sentence.
 */

const BUNDLE = '/client/straf3_game.js';

/**
 * @typedef {object} ClientStatus
 * @property {'loading'|'ready'|'error'|'refused'} kind
 * @property {string} message
 */

/**
 * @typedef {object} FinishedRun
 * @property {number} time_ms
 * @property {string} run_digest_hex16
 * @property {Uint8Array} s3d
 */

/**
 * Was a WebGPU adapter actually obtainable?
 *
 * @returns {Promise<{ok: true, backend: string} | {ok: false, why: string}>}
 */
export async function pickBackend() {
  const forced = new URLSearchParams(location.search).get('backend');
  if (forced) return { ok: true, backend: forced };
  if (!('gpu' in navigator)) {
    return {
      ok: false,
      why:
        'This browser does not expose WebGPU (`navigator.gpu` is undefined). ' +
        'Chrome 113+, Edge 113+, or Firefox with WebGPU enabled will run straf3.',
    };
  }
  try {
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) {
      return {
        ok: false,
        why:
          'WebGPU is present but no adapter was offered. On a machine with no hardware GPU, ' +
          'launch Chrome with --enable-unsafe-webgpu --use-angle=swiftshader.',
      };
    }
    return { ok: true, backend: 'webgpu' };
  } catch (err) {
    return { ok: false, why: `requesting a WebGPU adapter threw: ${err instanceof Error ? err.message : String(err)}` };
  }
}

/**
 * Is the client bundle built and served?
 *
 * @returns {Promise<{ok: true} | {ok: false, why: string}>}
 */
export async function probeBundle() {
  let res;
  try {
    res = await fetch(BUNDLE, { method: 'HEAD' });
  } catch (err) {
    return { ok: false, why: `${BUNDLE} could not be fetched: ${err instanceof Error ? err.message : String(err)}` };
  }
  if (res.status === 404) {
    return {
      ok: false,
      why:
        `${BUNDLE} is not being served. The dev server mounts /client from ` +
        'crates/straf3-game/web/pkg/, which is produced by crates/straf3-game/web/build.sh. ' +
        'Until that has been run there is no browser client to load.',
    };
  }
  if (!res.ok) return { ok: false, why: `${BUNDLE} returned HTTP ${res.status}` };
  return { ok: true };
}

/**
 * Keep a canvas's backing store in device pixels, matching its CSS box.
 *
 * The renderer configures its surface from the canvas's `width`/`height`, so a
 * canvas left at the default 300×150 renders a postage stamp scaled up.
 *
 * @param {HTMLCanvasElement} canvas
 * @returns {() => void} stop observing
 */
export function fitCanvas(canvas) {
  const apply = () => {
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    const w = Math.max(1, Math.round((rect.width || window.innerWidth) * dpr));
    const h = Math.max(1, Math.round((rect.height || window.innerHeight) * dpr));
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
  };
  apply();
  const ro = new ResizeObserver(apply);
  ro.observe(canvas);
  window.addEventListener('resize', apply);
  return () => {
    ro.disconnect();
    window.removeEventListener('resize', apply);
  };
}

/**
 * Install the callback surface the client calls into.
 *
 * `onRunFinished` is where a finished run leaves wasm, and the digest it
 * carries is deliberately published three ways:
 *
 *  - a console line, greppable, `[straf3] run-finished …`;
 *  - `globalThis.straf3.lastRun`, readable by anything driving the page;
 *  - `data-run-digest` on the page's result element.
 *
 * The reason is r6. A harness that re-simulates the recording natively and
 * compares the result against the recording's own header is checking the file
 * against itself: a header written by a code path that never ran the
 * simulation would agree with itself perfectly. Reporting the digest through a
 * channel that is not the file gives the comparison a second witness, and
 * `--expect-digest` is where it goes.
 *
 * @param {object} hooks
 * @param {(s: ClientStatus) => void} [hooks.onStatus]
 * @param {(run: FinishedRun) => void} [hooks.onRunFinished]
 * @param {(locked: boolean) => void} [hooks.onPointerLock]
 */
export function installHooks(hooks) {
  const bag = /** @type {any} */ (globalThis).straf3 ?? {};

  bag.onStatus = (/** @type {any} */ kind, /** @type {any} */ message) => {
    const s = { kind: String(kind ?? 'loading'), message: String(message ?? '') };
    console.log(`[straf3] status ${s.kind}: ${s.message}`);
    bag.lastStatus = s;
    hooks.onStatus?.(/** @type {ClientStatus} */ (s));
  };

  bag.onRunFinished = (/** @type {any} */ run) => {
    const finished = {
      time_ms: Number(run?.time_ms ?? NaN),
      run_digest_hex16: String(run?.run_digest_hex16 ?? ''),
      s3d: run?.s3d instanceof Uint8Array ? run.s3d : new Uint8Array(run?.s3d ?? []),
    };
    // One line, fixed shape, out of band from the .s3d itself.
    console.log(
      `[straf3] run-finished run_digest_hex16=${finished.run_digest_hex16} ` +
      `time_ms=${finished.time_ms} s3d_bytes=${finished.s3d.length}`,
    );
    bag.lastRun = {
      time_ms: finished.time_ms,
      run_digest_hex16: finished.run_digest_hex16,
      s3d_bytes: finished.s3d.length,
      at: new Date().toISOString(),
    };
    hooks.onRunFinished?.(finished);
  };

  bag.onPointerLock = (/** @type {any} */ locked) => {
    const isLocked = Boolean(locked);
    console.log(`[straf3] pointer-lock ${isLocked ? 'acquired' : 'released'}`);
    bag.pointerLocked = isLocked;
    hooks.onPointerLock?.(isLocked);
  };

  /** @type {any} */ (globalThis).straf3 = bag;
  return bag;
}

/**
 * @typedef {object} LaunchConfig
 * @property {'play'|'watch'} mode
 * @property {string} canvas_id
 * @property {{slug: string, source_url: string}} [map]
 * @property {{family: string, digest: string|null}} [physics]
 * @property {string|null} [ghost_url]
 * @property {string|null} [recording_url]
 * @property {number} [seek_ms]
 */

/**
 * Load the client and start it.
 *
 * Resolves when `start_web` has been called — not when the game is running.
 * winit's web backend never returns normally from `spawn_app`; it throws a
 * sentinel exception to unwind, and that is its ordinary control flow on this
 * platform, not a failure. Anything else is reported.
 *
 * @param {LaunchConfig} config
 * @returns {Promise<{ok: true} | {ok: false, why: string, kind: 'no-webgpu'|'no-bundle'|'threw'}>}
 */
export async function launch(config) {
  // The bundle is checked first, and the order is not arbitrary: if there is no
  // client to run, whether this browser would have given it a GPU is not a
  // question anyone needs answered. Reporting "no WebGPU adapter" to someone
  // who has simply not built the client yet sends them to the wrong problem.
  const bundle = await probeBundle();
  if (!bundle.ok) return { ok: false, why: bundle.why, kind: 'no-bundle' };

  const backend = await pickBackend();
  if (!backend.ok) return { ok: false, why: backend.why, kind: 'no-webgpu' };

  const full = { backend: backend.backend, ...config };
  console.log(`[straf3] start_web ${JSON.stringify(full)}`);

  try {
    const module = await import(/* @vite-ignore */ BUNDLE);
    await module.default();
    // The config travels as JSON so this call signature never has to change.
    // A build that predates the JSON config still takes a bare backend string;
    // calling it with JSON would start it in a default map rather than the one
    // the URL named, and silently substituting the map is exactly what r3
    // forbids — so the arity is checked rather than assumed.
    if (module.start_web.length === 0) {
      return {
        ok: false,
        kind: 'threw',
        why:
          'The loaded client build takes no configuration, so it cannot be told which map ' +
          'and physics this URL names. Rebuild it from a source that implements ' +
          'start_web(configJson) — see the wave contract §B.',
      };
    }
    module.start_web(JSON.stringify(full));
    return { ok: true };
  } catch (err) {
    const text = err instanceof Error ? `${err.name}: ${err.message}` : String(err);
    if (text.includes('Using exceptions for control flow')) return { ok: true };
    console.error('[straf3] client failed to start', err);
    return { ok: false, kind: 'threw', why: text };
  }
}

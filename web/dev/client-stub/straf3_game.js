/**
 * A stand-in for the browser client, implementing wave contract §B and nothing
 * else.
 *
 * **This does not simulate anything.** It has no physics, no renderer and no
 * recorder; it draws a placeholder on the canvas and calls the callbacks the
 * real client calls, with the shapes the real client uses. Its entire purpose
 * is to let the *site* half of the JS↔wasm interface be exercised and looked at
 * before the wasm half exists:
 *
 *  - is the config the page builds actually right — the map the URL named, the
 *    physics the URL named, pinned or not, the ghost, the seek?
 *  - does `onStatus("refused", …)` reach the page and replace the stage rather
 *    than getting logged somewhere nobody reads?
 *  - does `onRunFinished` publish `run_digest_hex16` where a harness can read
 *    it — the console line, `globalThis.straf3.lastRun`, and the DOM?
 *
 * Serve it with `node web/dev/serve.mjs --client-dir web/dev/client-stub` and
 * load a page with `?backend=stub`, which skips the WebGPU adapter probe. The
 * `?stub=` query controls what it pretends happened:
 *
 *   ?stub=ready      start normally and idle          (default)
 *   ?stub=refuse     onStatus("refused", …) naming both digests
 *   ?stub=finish     start, then emit a finished run after a moment
 *
 * The digest it reports in `finish` is derived from the config so it is stable
 * per URL and obviously synthetic. **No number this file produces is a
 * measurement, and none of them may be published as one.**
 */

/** wasm-bindgen's default export is the module initialiser. */
export default async function init() {
  console.log('[straf3-stub] init — this is NOT the browser client');
  return {};
}

/** @param {string} configJson */
export function start_web(configJson) {
  const config = JSON.parse(configJson);
  const hooks = globalThis.straf3 ?? {};
  const mode = new URLSearchParams(location.search).get('stub') ?? 'ready';

  console.log('[straf3-stub] start_web', config);
  hooks.onStatus?.('loading', 'stub client starting');

  const canvas = document.getElementById(config.canvas_id);
  if (canvas) paint(canvas, config, mode);

  // Pointer lock is the client's, and it is taken on a click and never on load
  // (r4). The stub honours that rule so the page's own handling of it can be
  // seen; the browser would refuse a load-time request anyway.
  if (canvas) {
    canvas.addEventListener('pointerdown', () => {
      canvas.requestPointerLock?.();
    });
    document.addEventListener('pointerlockchange', () => {
      hooks.onPointerLock?.(document.pointerLockElement === canvas);
    });
  }

  if (mode === 'refuse') {
    const asked = config.physics?.digest ?? '(unpinned)';
    hooks.onStatus?.(
      'refused',
      `This build implements physics 1111222233334444 and the URL pins ${asked}. ` +
      'They are different constants, so the run this link promises is not a run this build can produce.',
    );
    return;
  }

  hooks.onStatus?.('ready', 'stub client running');

  if (mode === 'finish') {
    setTimeout(() => {
      const run = fabricateRun(config);
      console.log('[straf3-stub] emitting a fabricated finished run');
      hooks.onRunFinished?.(run);
    }, 400);
  }
}

/**
 * A syntactically valid `.s3d` header carrying a run digest, so the page's
 * download, submit and decode paths see bytes rather than a placeholder.
 * The command block is empty and nothing simulated it.
 *
 * Layout follows `crates/straf3-replay/src/codec.rs`, transcribed the same way
 * `web/site/app/s3d.js` transcribes it.
 */
function fabricateRun(config) {
  const digest = synthDigest(JSON.stringify(config));
  const name = new TextEncoder().encode(config.physics?.family ?? 'stub');
  const mapName = new TextEncoder().encode(config.map?.slug ?? 'stub');

  // header: rate_hz u32, command_count u32, sim_time_ms u32, run_time_ms u32,
  // run_finished u8, world_tag u8, spawn 3×f32, spawn_yaw f32, run_digest u64,
  // [world_tag 2: collision_digest u64, name], physics_digest u64, name
  const headerLen = 4 + 4 + 4 + 4 + 1 + 1 + 12 + 4 + 8 + (8 + 4 + mapName.length) + 8 + (4 + name.length);
  const total = 4 + 4 + 4 + 4 + headerLen + 8;
  const buf = new ArrayBuffer(total);
  const dv = new DataView(buf);
  const u8 = new Uint8Array(buf);
  let o = 0;

  u8.set(new TextEncoder().encode('S3DR'), o); o += 4;
  dv.setUint32(o, 1, true); o += 4;               // format_version
  dv.setUint32(o, 0, true); o += 4;               // flags (no trace)
  dv.setUint32(o, headerLen, true); o += 4;       // header_len

  dv.setUint32(o, 125, true); o += 4;             // rate_hz
  dv.setUint32(o, 0, true); o += 4;               // command_count
  dv.setUint32(o, 0, true); o += 4;               // sim_time_ms
  dv.setUint32(o, 24_318, true); o += 4;          // run_time_ms
  dv.setUint8(o, 1); o += 1;                      // run_finished
  dv.setUint8(o, 2); o += 1;                      // world_tag = map
  dv.setFloat32(o, 0, true); o += 4;
  dv.setFloat32(o, 0, true); o += 4;
  dv.setFloat32(o, 0, true); o += 4;
  dv.setFloat32(o, 0, true); o += 4;              // spawn_yaw
  dv.setBigUint64(o, BigInt('0x' + digest), true); o += 8;   // run_digest
  dv.setBigUint64(o, 0xa11ce5a11ce5a11cn, true); o += 8;     // collision_digest
  dv.setUint32(o, mapName.length, true); o += 4;
  u8.set(mapName, o); o += mapName.length;
  dv.setBigUint64(o, BigInt('0x' + (config.physics?.digest ?? '1111222233334444')), true); o += 8;
  dv.setUint32(o, name.length, true); o += 4;
  u8.set(name, o); o += name.length;

  // content digest: FNV-1a over everything before it
  let h = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = (1n << 64n) - 1n;
  for (let i = 0; i < o; i += 1) { h ^= BigInt(u8[i]); h = (h * prime) & mask; }
  dv.setBigUint64(o, h, true);

  return { time_ms: 24_318, run_digest_hex16: digest, s3d: u8 };
}

/** A stable, obviously-synthetic 16-hex value derived from a string. */
function synthDigest(text) {
  let h = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = (1n << 64n) - 1n;
  for (const ch of new TextEncoder().encode(text)) { h ^= BigInt(ch); h = (h * prime) & mask; }
  return h.toString(16).padStart(16, '0');
}

/** Something visibly not a game, so no screenshot of this is mistaken for one. */
function paint(canvas, config, mode) {
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const w = canvas.width;
  const hgt = canvas.height;
  ctx.fillStyle = '#0b0d10';
  ctx.fillRect(0, 0, w, hgt);
  ctx.strokeStyle = '#1e252d';
  ctx.lineWidth = 2;
  for (let x = 0; x < w; x += 48) { ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, hgt); ctx.stroke(); }
  for (let y = 0; y < hgt; y += 48) { ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke(); }
  ctx.fillStyle = '#e8b14a';
  ctx.font = '600 20px ui-monospace, monospace';
  ctx.textAlign = 'center';
  ctx.fillText('STUB CLIENT — nothing is being simulated', w / 2, hgt / 2 - 16);
  ctx.fillStyle = '#8b95a1';
  ctx.font = '15px ui-monospace, monospace';
  ctx.fillText(`${config.mode} · ${config.map?.slug ?? config.recording_url ?? '?'} · stub=${mode}`, w / 2, hgt / 2 + 16);
}

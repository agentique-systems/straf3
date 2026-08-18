// @ts-check
/**
 * A `.s3d` header decoder, in the browser.
 *
 * This is a *parser*, not a simulator. It reads the header and the command
 * block and it verifies the content digest. It never computes a time, never
 * re-derives a run digest, and never decides whether a run is valid — those
 * are the verifier's job (`docs/web/ARCHITECTURE.md` §7.2), and a record page
 * that answered them itself would be inventing evidence.
 *
 * Every field below is transcribed from `crates/straf3-replay/src/codec.rs`.
 * The layout comment at the top of that file is the specification; if the two
 * ever disagree, that file is right and this one is a bug. Two properties of
 * the format make a JS transcription safe:
 *
 *  - every integer is little-endian and explicitly sized — no `usize`, so the
 *    file a 32-bit wasm target writes is the file a 64-bit native target
 *    writes;
 *  - every `f32` travels as its `to_bits` pattern, so nothing on this path
 *    rounds. `DataView.getFloat32` on the same four bytes reproduces the exact
 *    value, `-0.0` and denormals included.
 *
 * 64-bit digests are decoded as `BigInt`. A `u64` does not fit in a JS number
 * and a digest that has been silently rounded to 53 bits is worse than no
 * digest at all: it would compare equal to a value it is not.
 */

/** The four bytes every `.s3d` starts with — `codec::MAGIC`. */
export const MAGIC = 'S3DR';

/** The only layout this reader accepts — `codec::FORMAT_VERSION`. */
export const FORMAT_VERSION = 1;

/** `flags` bit 0: the per-command checksum trace is present — `codec::FLAG_TRACE`. */
export const FLAG_TRACE = 1 << 0;

/** Bytes per encoded command — `codec::COMMAND_BYTES`. */
export const COMMAND_BYTES = 13;

/** `codec::MAX_NAME_BYTES`. A corrupt length asks for a kilobyte, not 4 GiB. */
export const MAX_NAME_BYTES = 4096;

/** FNV-1a 64-bit, as `straf3_replay::digest`. */
const FNV_OFFSET = 0xcbf2_9ce4_8422_2325n;
const FNV_PRIME = 0x0000_0100_0000_01b3n;
const U64 = (1n << 64n) - 1n;

/**
 * The byte-wise FNV-1a fold that produces the content digest.
 * Mirrors `digest::Fnv1a::bytes`.
 *
 * @param {Uint8Array} bytes
 * @returns {bigint}
 */
export function fnv1a(bytes) {
  let h = FNV_OFFSET;
  for (let i = 0; i < bytes.length; i += 1) {
    h ^= BigInt(bytes[i]);
    h = (h * FNV_PRIME) & U64;
  }
  return h;
}

/**
 * Fold one 64-bit value into a rolling digest, little-endian.
 * Mirrors `digest::fold`.
 *
 * @param {bigint} digest
 * @param {bigint} value
 * @returns {bigint}
 */
export function fold(digest, value) {
  let h = digest;
  let v = value & U64;
  for (let i = 0; i < 8; i += 1) {
    h ^= v & 0xffn;
    h = (h * FNV_PRIME) & U64;
    v >>= 8n;
  }
  return h;
}

/**
 * Fold a whole sequence of per-command checksums, seeded at `FNV_OFFSET`.
 * Mirrors `digest::fold_all` — the definition of a run digest.
 *
 * @param {Iterable<bigint>} values
 * @returns {bigint}
 */
export function foldAll(values) {
  let h = FNV_OFFSET;
  for (const v of values) h = fold(h, v);
  return h;
}

/**
 * A `u64` as the sixteen lowercase hex digits everything else in this project
 * writes it as. Zero-padded, so digests sort and compare as strings.
 *
 * @param {bigint} value
 * @returns {string}
 */
export function hex64(value) {
  return (value & U64).toString(16).padStart(16, '0');
}

/** Thrown for anything malformed. The message names the field, not the offset. */
export class DecodeError extends Error {
  /** @param {string} message */
  constructor(message) {
    super(message);
    this.name = 'DecodeError';
  }
}

/**
 * A bounds-checked forward cursor — the JS twin of `codec::Cursor`, and for
 * the same reason: exactly one place can run off the end of a truncated file,
 * and it throws a named error instead of returning `undefined` that then
 * decodes into a plausible-looking record.
 */
class Cursor {
  /** @param {Uint8Array} bytes */
  constructor(bytes) {
    this.bytes = bytes;
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    this.at = 0;
  }

  /** @param {number} n @param {string} what */
  take(n, what) {
    const end = this.at + n;
    if (end > this.bytes.length) {
      throw new DecodeError(
        `truncated reading ${what}: need ${end} bytes, file has ${this.bytes.length}`,
      );
    }
    const out = this.bytes.subarray(this.at, end);
    this.at = end;
    return out;
  }

  /** @param {string} what */
  u8(what) {
    const at = this.at;
    this.take(1, what);
    return this.view.getUint8(at);
  }

  /** @param {string} what */
  i8(what) {
    const at = this.at;
    this.take(1, what);
    return this.view.getInt8(at);
  }

  /** @param {string} what */
  u16(what) {
    const at = this.at;
    this.take(2, what);
    return this.view.getUint16(at, true);
  }

  /** @param {string} what */
  u32(what) {
    const at = this.at;
    this.take(4, what);
    return this.view.getUint32(at, true);
  }

  /** @param {string} what */
  u64(what) {
    const at = this.at;
    this.take(8, what);
    return this.view.getBigUint64(at, true);
  }

  /** `f32` from its stored bit pattern — no conversion happens on this path. */
  /** @param {string} what */
  f32(what) {
    const at = this.at;
    this.take(4, what);
    return this.view.getFloat32(at, true);
  }

  /** @param {string} what */
  name(what) {
    const len = this.u32(`${what} length`);
    if (len > MAX_NAME_BYTES) {
      throw new DecodeError(`${what} length ${len} exceeds MAX_NAME_BYTES ${MAX_NAME_BYTES}`);
    }
    return new TextDecoder('utf-8', { fatal: true }).decode(this.take(len, what));
  }
}

/**
 * @typedef {{kind: 'empty'}
 *         | {kind: 'flat', heightBits: number, height: number}
 *         | {kind: 'map', name: string, collisionDigest: bigint}} WorldId
 */

/**
 * @typedef {object} Command
 * @property {number} durationMs
 * @property {number} buttons
 * @property {number} pitch  16-bit view angle, as recorded (C3)
 * @property {number} yaw
 * @property {number} roll
 * @property {number} forwardMove
 * @property {number} rightMove
 * @property {number} upMove
 */

/**
 * @typedef {object} Decoded
 * @property {number} formatVersion
 * @property {number} flags
 * @property {boolean} hasTrace
 * @property {number} rateHz            command rate; part of the physics, not the frame loop
 * @property {number} commandCount
 * @property {number} simTimeMs         exact integer sum of every command's duration
 * @property {number|null} runTimeMs    null when the run never crossed both triggers
 * @property {boolean} finished
 * @property {{x:number,y:number,z:number}} spawn
 * @property {number} spawnYaw          degrees
 * @property {bigint} runDigest         the rolling digest the recording claims
 * @property {WorldId} world
 * @property {bigint} physicsDigest
 * @property {string} physicsName
 * @property {bigint} contentDigest     the trailing digest as stored
 * @property {boolean} contentDigestOk  recomputed over the preceding bytes
 * @property {number} byteLength
 * @property {Command[]} commands
 * @property {bigint[]|null} trace      per-command checksums, when present
 * @property {bigint|null} traceFold    foldAll(trace) — must equal runDigest
 * @property {boolean|null} traceMatchesDigest
 */

/**
 * Decode a `.s3d` file.
 *
 * `commands` is decoded because the record page reports the command count as
 * a *verified* count rather than the header's claim, and because a file whose
 * command block does not match `command_count` is corrupt in a way the content
 * digest alone would not localise.
 *
 * @param {ArrayBuffer|Uint8Array} input
 * @param {{maxCommands?: number}} [limits] `Limits.max_commands` — the
 *   architecture's §7.3 intake bound is 150 000 (20 minutes at 125 Hz).
 * @returns {Decoded}
 */
export function decode(input, limits = {}) {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  const maxCommands = limits.maxCommands ?? 150_000;

  if (bytes.length < 16 + 8) {
    throw new DecodeError(`file is ${bytes.length} bytes — too short to be a recording`);
  }

  const c = new Cursor(bytes);
  const magic = new TextDecoder().decode(c.take(4, 'magic'));
  if (magic !== MAGIC) {
    throw new DecodeError(`not a .s3d file: magic is ${JSON.stringify(magic)}, expected "S3DR"`);
  }
  const formatVersion = c.u32('format_version');
  if (formatVersion !== FORMAT_VERSION) {
    throw new DecodeError(
      `format version ${formatVersion} — this reader knows version ${FORMAT_VERSION} only`,
    );
  }
  const flags = c.u32('flags');
  if ((flags & ~FLAG_TRACE) !== 0) {
    throw new DecodeError(`unknown flag bits set (0x${flags.toString(16)}) — newer producer`);
  }
  const hasTrace = (flags & FLAG_TRACE) !== 0;

  const headerLen = c.u32('header_len');
  const headerStart = c.at;
  c.take(headerLen, 'header');
  const h = new Cursor(bytes.subarray(headerStart, headerStart + headerLen));

  const rateHz = h.u32('rate_hz');
  const commandCount = h.u32('command_count');
  const simTimeMs = h.u32('sim_time_ms');
  const runTimeRaw = h.u32('run_time_ms');
  const finished = h.u8('run_finished') !== 0;
  const worldTag = h.u8('world_tag');
  const spawn = { x: h.f32('spawn.x'), y: h.f32('spawn.y'), z: h.f32('spawn.z') };
  const spawnYaw = h.f32('spawn_yaw');
  const runDigest = h.u64('run_digest');

  /** @type {WorldId} */
  let world;
  if (worldTag === 0) {
    world = { kind: 'empty' };
  } else if (worldTag === 1) {
    const heightBits = h.u32('flat height');
    const dv = new DataView(new ArrayBuffer(4));
    dv.setUint32(0, heightBits, true);
    world = { kind: 'flat', heightBits, height: dv.getFloat32(0, true) };
  } else if (worldTag === 2) {
    const collisionDigest = h.u64('collision_digest');
    world = { kind: 'map', collisionDigest, name: h.name('map name') };
  } else {
    throw new DecodeError(`unknown world tag ${worldTag}`);
  }

  const physicsDigest = h.u64('physics_digest');
  const physicsName = h.name('physics name');

  if (commandCount > maxCommands) {
    throw new DecodeError(
      `${commandCount} commands exceeds the ${maxCommands}-command limit`,
    );
  }

  /** @type {Command[]} */
  const commands = new Array(commandCount);
  for (let i = 0; i < commandCount; i += 1) {
    commands[i] = {
      durationMs: c.u16('command duration'),
      buttons: c.u16('command buttons'),
      pitch: c.u16('command pitch'),
      yaw: c.u16('command yaw'),
      roll: c.u16('command roll'),
      forwardMove: c.i8('command forward_move'),
      rightMove: c.i8('command right_move'),
      upMove: c.i8('command up_move'),
    };
  }

  /** @type {bigint[]|null} */
  let trace = null;
  if (hasTrace) {
    trace = new Array(commandCount);
    for (let i = 0; i < commandCount; i += 1) trace[i] = c.u64('checksum trace');
  }

  const digestAt = c.at;
  const contentDigest = c.u64('content digest');
  if (c.at !== bytes.length) {
    throw new DecodeError(
      `${bytes.length - c.at} trailing bytes after the content digest`,
    );
  }
  const contentDigestOk = fnv1a(bytes.subarray(0, digestAt)) === contentDigest;

  const traceFold = trace ? foldAll(trace) : null;

  return {
    formatVersion,
    flags,
    hasTrace,
    rateHz,
    commandCount,
    simTimeMs,
    runTimeMs: finished ? runTimeRaw : null,
    finished,
    spawn,
    spawnYaw,
    runDigest,
    world,
    physicsDigest,
    physicsName,
    contentDigest,
    contentDigestOk,
    byteLength: bytes.length,
    commands,
    trace,
    traceFold,
    traceMatchesDigest: traceFold === null ? null : traceFold === runDigest,
  };
}

/**
 * `time_ms` rendered the one way this project renders it.
 *
 * Milliseconds in, always — §5.1: "No column, API field or TypeScript type in
 * this platform is a duration in seconds." This function is the only place the
 * site turns one into something with a colon in it.
 *
 * @param {number|null|undefined} ms
 * @returns {string}
 */
export function formatTime(ms) {
  if (ms === null || ms === undefined) return '—';
  const sign = ms < 0 ? '-' : '';
  const t = Math.abs(ms);
  const minutes = Math.floor(t / 60_000);
  const seconds = Math.floor((t % 60_000) / 1000);
  const millis = t % 1000;
  const ss = String(seconds).padStart(minutes > 0 ? 2 : 1, '0');
  const mmm = String(millis).padStart(3, '0');
  return minutes > 0 ? `${sign}${minutes}:${ss}.${mmm}` : `${sign}${ss}.${mmm}`;
}

// @ts-check
/**
 * A minimal, structurally valid `.s3d` encoder — for fixtures only.
 *
 * **Nothing here simulates anything.** It writes a header and a content digest
 * in the layout `crates/straf3-replay/src/codec.rs` specifies, with an empty
 * command block, so that the site's real decoder (`web/site/app/s3d.js`) has
 * real bytes to decode and the watch path has a real URL to fetch. The run
 * digest it carries is a value passed in, not a value folded over a simulation,
 * and a file this produces would be rejected by the verifier the moment it
 * re-simulated it — which is correct, because there is nothing in it to
 * re-simulate.
 *
 * The reason it exists at all: `/watch/<run>` cannot be exercised without bytes
 * to play back, and inventing a *plausible* recording is worse than inventing
 * an obviously empty one. This one is transparently empty — zero commands, zero
 * sim time — so no screenshot of it can be mistaken for a run.
 */

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const U64 = (1n << 64n) - 1n;

/**
 * @param {object} o
 * @param {string} o.runDigest    16-hex
 * @param {string} o.physicsDigest 16-hex
 * @param {string} o.physicsName
 * @param {string} o.mapName
 * @param {string} o.collisionDigest 16-hex
 * @param {number} o.runTimeMs
 * @returns {Buffer}
 */
export function encodeDemo({ runDigest, physicsDigest, physicsName, mapName, collisionDigest, runTimeMs }) {
  const pname = Buffer.from(physicsName, 'utf8');
  const mname = Buffer.from(mapName, 'utf8');

  // rate_hz, command_count, sim_time_ms, run_time_ms, run_finished, world_tag,
  // spawn xyz, spawn_yaw, run_digest, [collision_digest, name], physics_digest, name
  const headerLen = 4 + 4 + 4 + 4 + 1 + 1 + 12 + 4 + 8 + (8 + 4 + mname.length) + 8 + (4 + pname.length);
  const buf = Buffer.alloc(4 + 4 + 4 + 4 + headerLen + 8);
  let o = 0;

  buf.write('S3DR', o, 'ascii'); o += 4;
  buf.writeUInt32LE(1, o); o += 4;           // format_version
  buf.writeUInt32LE(0, o); o += 4;           // flags — no checksum trace
  buf.writeUInt32LE(headerLen, o); o += 4;

  buf.writeUInt32LE(125, o); o += 4;         // rate_hz
  buf.writeUInt32LE(0, o); o += 4;           // command_count — empty, deliberately
  buf.writeUInt32LE(0, o); o += 4;           // sim_time_ms
  buf.writeUInt32LE(runTimeMs, o); o += 4;
  buf.writeUInt8(1, o); o += 1;              // run_finished
  buf.writeUInt8(2, o); o += 1;              // world_tag = map
  buf.writeFloatLE(0, o); o += 4;
  buf.writeFloatLE(0, o); o += 4;
  buf.writeFloatLE(0, o); o += 4;
  buf.writeFloatLE(0, o); o += 4;            // spawn_yaw
  buf.writeBigUInt64LE(BigInt('0x' + runDigest), o); o += 8;
  buf.writeBigUInt64LE(BigInt('0x' + collisionDigest), o); o += 8;
  buf.writeUInt32LE(mname.length, o); o += 4;
  mname.copy(buf, o); o += mname.length;
  buf.writeBigUInt64LE(BigInt('0x' + physicsDigest), o); o += 8;
  buf.writeUInt32LE(pname.length, o); o += 4;
  pname.copy(buf, o); o += pname.length;

  let h = FNV_OFFSET;
  for (let i = 0; i < o; i += 1) {
    h ^= BigInt(buf[i]);
    h = (h * FNV_PRIME) & U64;
  }
  buf.writeBigUInt64LE(h, o);
  return buf;
}

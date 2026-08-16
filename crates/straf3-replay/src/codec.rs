//! The `.s3d` byte layout, written out by hand.
//!
//! # The layout
//!
//! Every integer is little-endian. There is no padding, no alignment
//! assumption and no `usize` anywhere: wasm is a 32-bit target and a native
//! `usize` would give the browser a different file for the same run, which is
//! the exact failure criterion 7 requirement 3 exists to prevent. Every `f32`
//! travels as its [`f32::to_bits`] pattern, never as text and never as a
//! value, so `-0.0` survives, a denormal survives, and no rounding happens
//! anywhere on the path.
//!
//! ```text
//! offset  size      field
//! ──────  ────      ─────
//! 0       4         magic "S3DR"
//! 4       4         format_version : u32          (refuse anything but 1)
//! 8       4         flags : u32                   (bit 0 = checksum trace present)
//! 12      4         header_len : u32              (bytes of the header section)
//! 16      header_len
//!                   ┌ rate_hz          : u32
//!                   │ command_count    : u32  = N
//!                   │ sim_time_ms      : u32
//!                   │ run_time_ms      : u32
//!                   │ run_finished     : u8   (0 or 1)
//!                   │ world_tag        : u8   (0 empty, 1 flat, 2 map)
//!                   │ spawn x, y, z    : u32 × 3   (f32 bits)
//!                   │ spawn_yaw        : u32       (f32 bits, degrees)
//!                   │ run_digest       : u64
//!                   │ world payload    : ─ tag 0: nothing
//!                   │                    ├ tag 1: height : u32 (f32 bits)
//!                   │                    └ tag 2: collision_digest : u64,
//!                   │                             name : u32 length + UTF-8
//!                   │ physics_digest   : u64
//!                   └ physics_name     : u32 length + UTF-8
//!         13 × N    commands
//!                   ┌ duration_ms : u16
//!                   │ buttons     : u16
//!                   │ pitch       : u16   ┐ the 16-bit view angles, exactly
//!                   │ yaw         : u16   │ as the command carries them (C3)
//!                   │ roll        : u16   ┘
//!                   │ forward_move: i8
//!                   │ right_move  : i8
//!                   └ up_move     : i8
//!         8 × N     per-command state checksums   (only when flags bit 0 is set)
//!         8         content digest : u64
//! ```
//!
//! # Why the view angles are shorts here and degrees in the text fixture
//!
//! `straf3_game::record` writes each angle as the degrees it stands for,
//! because its reader is `straf3-headless`'s own independently written parser
//! and the file has to stay human-diffable. That round trip is exact — the
//! printed value is one of the 65 536 representable angles and re-quantises to
//! the identical short, measured exhaustively — but it is exact *because* a
//! test says so, and it costs about nine bytes an angle.
//!
//! Here the recorded value goes in as the recorded value. There is no
//! conversion on the path at all, so there is no round trip to be exact about.
//!
//! # Why there is no run-length encoding
//!
//! `straf3_game::record`'s text format folds identical consecutive commands
//! into a repeat count, and it should: a minute of standing still is one line
//! instead of 7 500. Here it would buy almost nothing. A recording worth saving
//! is a run, and during a run the view angle changes on nearly every command,
//! so consecutive commands are almost never byte-identical. A minute-long
//! personal best is about 98 KiB written straight out, and the decoder is a
//! flat loop with no state — which is worth more than the bytes, because a
//! decoder with state is a decoder that can disagree with the encoder.
//!
//! # Why there is a content digest as well as a run digest
//!
//! They answer different questions. The run digest says *this is the run that
//! was simulated*, and only a re-simulation can check it. The content digest
//! says *these are the bytes that were written*, and it is checked before
//! anything else is believed — so a file truncated by a full disk fails at
//! load with "corrupt", rather than parsing into a plausible recording with
//! half its commands missing and then failing much later as a mysterious
//! divergence.

use straf3_sim::num::to_bits;
use straf3_sim::{Buttons, TickRate, UserCmd, ViewAngles};

use crate::digest::{Fnv1a, fold_all};
use crate::error::LoadError;
use crate::identity::{PhysicsId, WorldId};
use crate::recording::{Outcome, Recording, RunStart};

/// The four bytes every `.s3d` starts with.
pub const MAGIC: [u8; 4] = *b"S3DR";

/// The format version this crate writes and the only one it reads.
///
/// Bump it for any change to the layout above. A reader refuses a version it
/// does not know rather than guessing, because the failure mode of guessing is
/// a recording that loads and replays to the wrong run.
pub const FORMAT_VERSION: u32 = 1;

/// `flags` bit 0: the per-command checksum trace is present.
pub const FLAG_TRACE: u32 = 1 << 0;

/// Every flag bit this version defines. A file setting anything else was
/// written by a newer producer and is refused.
const KNOWN_FLAGS: u32 = FLAG_TRACE;

/// Bytes per encoded command.
pub const COMMAND_BYTES: usize = 13;

/// The longest name (map or physics profile) a reader will accept.
///
/// Not a limit anything legitimate comes near — it is there so a corrupt
/// length field asks for a kilobyte and is refused, rather than asking for
/// four gigabytes and being granted.
pub const MAX_NAME_BYTES: u32 = 4096;

// ── writing ─────────────────────────────────────────────────────────────────

/// Build the whole file.
///
/// `include_trace` is only ever true when the recording actually has a trace —
/// [`Recording::to_bytes_with_checksums`] is the only caller that sets it, and
/// it returns `None` rather than calling here when there is nothing to write.
pub fn encode(rec: &Recording, include_trace: bool) -> Vec<u8> {
    let trace = if include_trace { rec.trace() } else { None };
    let flags = if trace.is_some() { FLAG_TRACE } else { 0 };

    let header = encode_header(rec);
    let mut out = Vec::with_capacity(
        16 + header.len() + rec.commands_unchecked().len() * COMMAND_BYTES + 8 + 8,
    );
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);

    for cmd in rec.commands_unchecked() {
        encode_command(cmd, &mut out);
    }
    if let Some(trace) = trace {
        for checksum in trace {
            out.extend_from_slice(&checksum.to_le_bytes());
        }
    }

    let mut content = Fnv1a::new();
    content.bytes(&out);
    out.extend_from_slice(&content.finish().to_le_bytes());
    out
}

fn encode_header(rec: &Recording) -> Vec<u8> {
    let mut h = Vec::with_capacity(64);
    let start = rec.start();
    let claimed = rec.claimed();

    h.extend_from_slice(&start.rate.hz().to_le_bytes());
    h.extend_from_slice(&(rec.commands_unchecked().len() as u32).to_le_bytes());
    h.extend_from_slice(&claimed.sim_time_ms.to_le_bytes());
    h.extend_from_slice(&claimed.run_time_ms.unwrap_or(0).to_le_bytes());
    h.push(u8::from(claimed.run_time_ms.is_some()));
    h.push(world_tag(rec.world()));
    h.extend_from_slice(&to_bits(start.spawn.x).to_le_bytes());
    h.extend_from_slice(&to_bits(start.spawn.y).to_le_bytes());
    h.extend_from_slice(&to_bits(start.spawn.z).to_le_bytes());
    h.extend_from_slice(&to_bits(start.yaw).to_le_bytes());
    h.extend_from_slice(&claimed.digest.to_le_bytes());

    match rec.world() {
        WorldId::Empty => {}
        WorldId::Flat { height_bits } => h.extend_from_slice(&height_bits.to_le_bytes()),
        WorldId::Map {
            name,
            collision_digest,
        } => {
            h.extend_from_slice(&collision_digest.to_le_bytes());
            encode_name(name, &mut h);
        }
    }

    h.extend_from_slice(&rec.physics().digest.to_le_bytes());
    encode_name(&rec.physics().name, &mut h);
    h
}

const fn world_tag(world: &WorldId) -> u8 {
    match world {
        WorldId::Empty => 0,
        WorldId::Flat { .. } => 1,
        WorldId::Map { .. } => 2,
    }
}

fn encode_name(name: &str, out: &mut Vec<u8>) {
    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}

fn encode_command(cmd: &UserCmd, out: &mut Vec<u8>) {
    out.extend_from_slice(&cmd.duration_ms.to_le_bytes());
    out.extend_from_slice(&cmd.buttons.0.to_le_bytes());
    out.extend_from_slice(&cmd.view.pitch.to_le_bytes());
    out.extend_from_slice(&cmd.view.yaw.to_le_bytes());
    out.extend_from_slice(&cmd.view.roll.to_le_bytes());
    out.push(cmd.forward_move as u8);
    out.push(cmd.right_move as u8);
    out.push(cmd.up_move as u8);
}

// ── reading ─────────────────────────────────────────────────────────────────

/// A bounds-checked forward cursor.
///
/// Every read goes through it, so there is exactly one place that can run off
/// the end of a truncated file and it returns an error instead of panicking.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], LoadError> {
        let end = self.at.checked_add(n).ok_or(LoadError::Truncated {
            need: u64::MAX,
            have: self.bytes.len() as u64,
        })?;
        if end > self.bytes.len() {
            return Err(LoadError::Truncated {
                need: end as u64,
                have: self.bytes.len() as u64,
            });
        }
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, LoadError> {
        Ok(self.take(1)?[0])
    }

    fn i8(&mut self) -> Result<i8, LoadError> {
        Ok(self.u8()? as i8)
    }

    fn u16(&mut self) -> Result<u16, LoadError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, LoadError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, LoadError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn name(&mut self, field: &'static str) -> Result<String, LoadError> {
        let len = self.u32()?;
        if len > MAX_NAME_BYTES {
            return Err(LoadError::NameTooLong { field, len });
        }
        let bytes = self.take(len as usize)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| LoadError::BadUtf8 { field })
    }

    const fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
}

/// Parse a whole file.
///
/// The order of the checks is load-bearing. The content digest is verified
/// *first*, over the file minus its last eight bytes, so nothing downstream
/// has to be defensive about a corrupt length field — and so a truncated file
/// is reported as truncated rather than as a recording with the wrong number
/// of commands.
pub fn decode(bytes: &[u8]) -> Result<Recording, LoadError> {
    if bytes.len() < 24 {
        return Err(LoadError::Truncated {
            need: 24,
            have: bytes.len() as u64,
        });
    }
    let (body, stored_content) = bytes.split_at(bytes.len() - 8);
    let stored_content = u64::from_le_bytes([
        stored_content[0],
        stored_content[1],
        stored_content[2],
        stored_content[3],
        stored_content[4],
        stored_content[5],
        stored_content[6],
        stored_content[7],
    ]);

    let mut c = Cursor::new(body);
    let magic = c.take(4)?;
    if magic != MAGIC {
        return Err(LoadError::NotAnS3d {
            found: [magic[0], magic[1], magic[2], magic[3]],
        });
    }

    // Version before content, and content before everything else. A `.s3d`
    // from a future version has a different content digest scheme by
    // definition, so reporting it as "corrupt" would be a lie; reporting the
    // version is the truth and is actionable.
    let version = c.u32()?;
    if version != FORMAT_VERSION {
        return Err(LoadError::UnsupportedVersion {
            found: version,
            supported: FORMAT_VERSION,
        });
    }

    let mut content = Fnv1a::new();
    content.bytes(body);
    let computed = content.finish();
    if computed != stored_content {
        return Err(LoadError::Corrupt {
            stored: stored_content,
            computed,
        });
    }

    let flags = c.u32()?;
    if flags & !KNOWN_FLAGS != 0 {
        return Err(LoadError::UnknownFlags {
            bits: flags & !KNOWN_FLAGS,
        });
    }
    let header_len = c.u32()?;

    let header_start = c.at;
    let rate_hz = c.u32()?;
    let rate = TickRate::from_hz(rate_hz).ok_or(LoadError::BadRate { hz: rate_hz })?;
    let command_count = c.u32()?;
    let sim_time_ms = c.u32()?;
    let run_time_ms = c.u32()?;
    let run_finished = match c.u8()? {
        0 => false,
        1 => true,
        other => {
            return Err(LoadError::BadBool {
                field: "run_finished",
                value: other,
            });
        }
    };
    let tag = c.u8()?;
    let spawn_x = f32::from_bits(c.u32()?);
    let spawn_y = f32::from_bits(c.u32()?);
    let spawn_z = f32::from_bits(c.u32()?);
    let yaw = f32::from_bits(c.u32()?);
    let digest = c.u64()?;

    let world = match tag {
        0 => WorldId::Empty,
        1 => WorldId::Flat {
            height_bits: c.u32()?,
        },
        2 => {
            let collision_digest = c.u64()?;
            WorldId::Map {
                name: c.name("map name")?,
                collision_digest,
            }
        }
        other => return Err(LoadError::BadWorldTag { tag: other }),
    };

    // Read into locals first. A struct literal would read in the order the
    // fields are *written*, which happens to be right and would silently stop
    // being right if somebody tidied the fields into declaration order.
    let physics_digest = c.u64()?;
    let physics_name = c.name("physics name")?;
    let physics = PhysicsId {
        name: physics_name,
        digest: physics_digest,
    };

    let consumed = (c.at - header_start) as u64;
    if consumed != u64::from(header_len) {
        return Err(LoadError::HeaderLength {
            declared: header_len,
            actual: consumed,
        });
    }

    // Check the declared command count against what is actually left before
    // reserving anything for it. `command_count` came out of a file; a
    // 4-billion-command claim must cost a comparison, not 52 GiB.
    let trace_present = flags & FLAG_TRACE != 0;
    let per_command = COMMAND_BYTES + if trace_present { 8 } else { 0 };
    let need = u64::from(command_count) * per_command as u64;
    if need > c.remaining() as u64 {
        return Err(LoadError::Truncated {
            need: c.at as u64 + need,
            have: bytes.len() as u64,
        });
    }

    let mut commands = Vec::with_capacity(command_count as usize);
    for _ in 0..command_count {
        commands.push(decode_command(&mut c)?);
    }

    let trace = if trace_present {
        let mut trace = Vec::with_capacity(command_count as usize);
        for _ in 0..command_count {
            trace.push(c.u64()?);
        }
        // Criterion 2's anti-tamper rule, applied to a saved run: a report's
        // digest must be derived from that report's own per-command
        // checksums. Without this a file could carry a run digest copied off
        // somebody else's leaderboard entry alongside a trace that folds to
        // something else, and the two would only be caught by a full
        // re-simulation.
        let folded = fold_all(trace.iter().copied());
        if folded != digest {
            return Err(LoadError::DigestNotDerivedFromTrace {
                stored: digest,
                folded,
            });
        }
        Some(trace)
    } else {
        None
    };

    if c.remaining() != 0 {
        return Err(LoadError::TrailingBytes {
            count: c.remaining() as u64,
        });
    }

    Ok(Recording::from_parts(
        RunStart {
            rate,
            spawn: straf3_sim::num::vec3(spawn_x, spawn_y, spawn_z),
            yaw,
        },
        commands,
        world,
        physics,
        Outcome {
            sim_time_ms,
            run_time_ms: run_finished.then_some(run_time_ms),
            digest,
        },
        trace,
    ))
}

fn decode_command(c: &mut Cursor<'_>) -> Result<UserCmd, LoadError> {
    let duration_ms = c.u16()?;
    let buttons = Buttons(c.u16()?);
    let pitch = c.u16()?;
    let yaw = c.u16()?;
    let roll = c.u16()?;
    // Read into locals in wire order, then assemble. `UserCmd`'s fields are
    // declared in a different order than they are written, and relying on a
    // struct literal's left-to-right evaluation to bridge that would make the
    // encoding depend on how the literal happens to be laid out.
    let forward_move = c.i8()?;
    let right_move = c.i8()?;
    let up_move = c.i8()?;
    Ok(UserCmd {
        duration_ms,
        forward_move,
        right_move,
        up_move,
        buttons,
        view: ViewAngles { pitch, yaw, roll },
    })
}

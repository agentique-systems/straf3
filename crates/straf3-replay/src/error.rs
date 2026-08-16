//! What can go wrong, and what each failure actually means for the player.
//!
//! These are hand-written rather than derived, matching `straf3_map`'s
//! `CompileError`: no crate below the line takes a dependency for an error
//! message, and the messages here are meant to be shown to somebody who has
//! just been told their personal best cannot be raced. "Mismatch" is not an
//! explanation; "the map has been recompiled since this run was recorded" is.

use crate::identity::{PhysicsId, WorldId};
use crate::recording::Outcome;

/// A `.s3d` that could not be read as one.
///
/// Every variant is a *refusal*, never a recovery. There is no lenient mode
/// and no "load what we can": a recording is a claim about a run, and a
/// half-parsed claim is worse than no claim, because it looks like one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The first four bytes are not `S3DR`.
    NotAnS3d {
        /// What was there instead.
        found: [u8; 4],
    },
    /// A format version this build does not know.
    UnsupportedVersion {
        /// The version in the file.
        found: u32,
        /// The only version this build reads.
        supported: u32,
    },
    /// Flag bits this build does not define — a file from a newer producer,
    /// carrying a section this reader would silently skip.
    UnknownFlags {
        /// Just the bits that were not recognised.
        bits: u32,
    },
    /// The file ends before the structure it declares does.
    Truncated {
        /// Bytes the declared structure needs.
        need: u64,
        /// Bytes there are.
        have: u64,
    },
    /// Bytes after the last section. A reader that ignored these would ignore
    /// whatever a future version put there.
    TrailingBytes {
        /// How many.
        count: u64,
    },
    /// The content digest does not match the bytes. The file changed after it
    /// was written — truncated by a full disk, mangled by a text-mode copy, or
    /// edited.
    Corrupt {
        /// The digest the file claims.
        stored: u64,
        /// The digest its bytes actually fold to.
        computed: u64,
    },
    /// The header declares a length the header does not have.
    HeaderLength {
        /// What the file said.
        declared: u32,
        /// What parsing the version-1 header actually consumed.
        actual: u64,
    },
    /// The stored run digest is not the fold of the stored per-command
    /// checksums — the two halves of the file disagree about what run it was.
    ///
    /// This is criterion 2's rule for the determinism report, applied to a
    /// recording: a digest must be derived from the numbers travelling beside
    /// it, so a tampered or spliced file fails at load rather than at
    /// re-simulation.
    DigestNotDerivedFromTrace {
        /// The digest in the header.
        stored: u64,
        /// The digest the trace actually folds to.
        folded: u64,
    },
    /// A command rate outside `1..=1000`, which
    /// [`TickRate`](straf3_sim::TickRate) cannot represent.
    BadRate {
        /// The rate in the file.
        hz: u32,
    },
    /// A world tag this version does not define.
    BadWorldTag {
        /// The tag byte.
        tag: u8,
    },
    /// A byte that must be 0 or 1 and was not. Refused rather than coerced,
    /// because a producer that wrote 2 was writing something this reader does
    /// not understand.
    BadBool {
        /// Which field.
        field: &'static str,
        /// What was there.
        value: u8,
    },
    /// A name field whose declared length is absurd.
    NameTooLong {
        /// Which field.
        field: &'static str,
        /// The declared length.
        len: u32,
    },
    /// A name field that is not UTF-8.
    BadUtf8 {
        /// Which field.
        field: &'static str,
    },
}

impl core::fmt::Display for LoadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotAnS3d { found } => write!(
                f,
                "not a .s3d recording: expected the bytes `S3DR`, found {found:?}"
            ),
            Self::UnsupportedVersion { found, supported } => write!(
                f,
                "this .s3d is format version {found}; this build reads version {supported} only"
            ),
            Self::UnknownFlags { bits } => write!(
                f,
                "this .s3d sets flag bits {bits:#x} that this build does not define — \
                 it was written by a newer version and carries a section this reader \
                 would have skipped"
            ),
            Self::Truncated { need, have } => write!(
                f,
                "truncated .s3d: the file declares a structure needing {need} bytes and is {have}"
            ),
            Self::TrailingBytes { count } => {
                write!(f, "{count} unread bytes after the end of the recording")
            }
            Self::Corrupt { stored, computed } => write!(
                f,
                "corrupt .s3d: the file's bytes fold to {computed:016x}, \
                 it claims {stored:016x}"
            ),
            Self::HeaderLength { declared, actual } => write!(
                f,
                "the .s3d header declares {declared} bytes and version 1 parsing consumed {actual}"
            ),
            Self::DigestNotDerivedFromTrace { stored, folded } => write!(
                f,
                "this .s3d claims run digest {stored:016x}, but its own per-command \
                 checksums fold to {folded:016x} — the recording's two halves \
                 describe different runs"
            ),
            Self::BadRate { hz } => write!(
                f,
                "this .s3d was recorded at {hz} Hz, which is outside the 1..=1000 range \
                 an integer-millisecond command can express"
            ),
            Self::BadWorldTag { tag } => write!(
                f,
                "unknown world tag {tag} (0 empty, 1 flat, 2 map) — a newer format version"
            ),
            Self::BadBool { field, value } => {
                write!(f, "`{field}` must be 0 or 1 and is {value}")
            }
            Self::NameTooLong { field, len } => {
                write!(
                    f,
                    "the {field} field declares {len} bytes, which is not a name"
                )
            }
            Self::BadUtf8 { field } => write!(f, "the {field} field is not valid UTF-8"),
        }
    }
}

impl core::error::Error for LoadError {}

/// The recording was not made against the world or the physics it is about to
/// be replayed against — contract item C6.
///
/// This is the error that makes a ghost trustworthy. It is not advisory and it
/// is not skippable: it is what [`Recording::commands_for`] returns instead of
/// the commands.
///
/// [`Recording::commands_for`]: crate::Recording::commands_for
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// A different world. When both sides are the same named map, this is the
    /// stale-geometry case: the map has been recompiled since the run.
    World {
        /// What the recording was made on.
        recorded: WorldId,
        /// What it is about to be replayed on.
        actual: WorldId,
    },
    /// Different movement constants.
    Physics {
        /// What the recording was made under.
        recorded: PhysicsId,
        /// The digest of the profile that is about to be used.
        actual: u64,
    },
}

impl Mismatch {
    /// Whether this is specifically "the same map, recompiled".
    ///
    /// Worth distinguishing in a user interface: every other mismatch means
    /// "you loaded the wrong thing", and this one means "your personal bests
    /// on this map are no longer comparable", which is a different sentence to
    /// have to write.
    #[must_use]
    pub fn is_stale_geometry(&self) -> bool {
        match self {
            Self::World {
                recorded: WorldId::Map { name: a, .. },
                actual: WorldId::Map { name: b, .. },
            } => a == b,
            _ => false,
        }
    }
}

impl core::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::World { recorded, actual } if self.is_stale_geometry() => write!(
                f,
                "this recording was made against a different compilation of the same map: \
                 it ran on {recorded}, and this is {actual}. The collision geometry has \
                 changed, so replaying it here would produce a different run and a \
                 different time."
            ),
            Self::World { recorded, actual } => write!(
                f,
                "this recording was made in {recorded} and is being replayed in {actual}"
            ),
            Self::Physics { recorded, actual } => write!(
                f,
                "this recording was made under physics profile {recorded} and is being \
                 replayed under {actual:016x} — the movement constants have changed, so \
                 the same commands no longer produce the same run"
            ),
        }
    }
}

impl core::error::Error for Mismatch {}

/// A re-simulation that ran but did not reproduce the run the file claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// It never got as far as running: the binding was wrong.
    Mismatch(Mismatch),
    /// It ran, against the right world and the right physics, and produced a
    /// different run.
    ///
    /// On the four targets this project verifies on, this must not happen. If
    /// it does, it is the divergence criterion 2 exists to catch, arriving
    /// through a saved run instead of through the reference stream.
    Diverged {
        /// What the file claims.
        claimed: Box<Outcome>,
        /// What re-simulating actually produced.
        actual: Box<Outcome>,
        /// The index of the first command whose state checksum differed.
        ///
        /// `None` when the recording was written without a checksum trace, in
        /// which case only the folded digest was available to compare — see
        /// [`Recording::to_bytes_with_checksums`](crate::Recording::to_bytes_with_checksums).
        first_diverging_command: Option<u32>,
    },
}

impl From<Mismatch> for VerifyError {
    fn from(m: Mismatch) -> Self {
        Self::Mismatch(m)
    }
}

impl core::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Mismatch(m) => write!(f, "{m}"),
            Self::Diverged {
                claimed,
                actual,
                first_diverging_command,
            } => {
                write!(
                    f,
                    "re-simulating this recording did not reproduce it: it claims \
                     digest {:016x} and {} and produced digest {:016x} and {}",
                    claimed.digest,
                    describe_time(claimed),
                    actual.digest,
                    describe_time(actual),
                )?;
                match first_diverging_command {
                    Some(n) => write!(f, "; first disagreement at command {n}"),
                    None => write!(
                        f,
                        "; the recording carries no checksum trace, so the first \
                         diverging command is unknown"
                    ),
                }
            }
        }
    }
}

fn describe_time(o: &Outcome) -> String {
    match o.run_time_ms {
        Some(ms) => format!("a finished run of {ms} ms"),
        None => format!("no finished run ({} ms simulated)", o.sim_time_ms),
    }
}

impl core::error::Error for VerifyError {}

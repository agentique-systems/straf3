//! ARCHITECTURE §7.3's bounds, in one place, because this endpoint spends CPU
//! on request.
//!
//! Everything here is a refusal that happens *before* a command is simulated.
//! The submission path is deliberately cheap and synchronous: a rejection is
//! worth issuing before any CPU is spent, and the expensive half runs in a
//! different process (§7.1).

use std::time::Duration;

/// §7.3: 20 minutes at 125 Hz. Longer submissions are rejected at decode.
pub const MAX_COMMANDS: u32 = 150_000;

/// §7.3: the compressed body ceiling.
pub const MAX_COMPRESSED_BYTES: usize = 1024 * 1024;

/// §7.3: the decompressed ceiling. Also the ceiling on an uncompressed body,
/// so the two paths cannot disagree about how big "too big" is.
pub const MAX_DECOMPRESSED_BYTES: usize = 8 * 1024 * 1024;

/// The explicit zstd window limit §7.3 asks for.
///
/// Without it a small frame can declare a very large window and make the
/// decoder allocate it before a single byte of output exists — the bound on
/// the *output* would not have been reached yet.
pub const ZSTD_WINDOW_LOG_MAX: u32 = 23;

/// §7.3: a wall-clock deadline per verification. At the ~200 ms §4.3 expects,
/// exceeding this means something is wrong rather than slow.
pub const VERIFY_DEADLINE: Duration = Duration::from_secs(5);

/// §7.3: attempt tickets are single-use and time-bounded. Longer than
/// [`MAX_COMMANDS`] allows a run to be, so it never truncates legitimate play.
pub const ATTEMPT_TTL: Duration = Duration::from_secs(30 * 60);

/// §7.3: a small cap on live unconsumed tickets, so a bulk harvest of tickets
/// cannot precede a bulk resubmission.
pub const MAX_LIVE_ATTEMPTS_PER_PLAYER: i64 = 8;

/// §7.3: per-player submission rate limits.
pub const MAX_SUBMISSIONS_PER_MINUTE: i64 = 30;
/// As above, over a day.
pub const MAX_SUBMISSIONS_PER_DAY: i64 = 500;

/// The bytes a run must actually be, given [`MAX_COMMANDS`].
///
/// Derived from the format rather than guessed: a `.s3d` is a fixed header, a
/// name or two, `COMMAND_BYTES` per command, optionally eight more per command
/// for the checksum trace, and an eight-byte content digest. A body larger than
/// this cannot be a recording this build would accept however it decompresses,
/// so it is refused before the decoder allocates for it.
#[must_use]
pub fn max_plausible_demo_bytes() -> usize {
    const HEADER_SLACK: usize = 4096;
    let per_command = straf3_replay::COMMAND_BYTES + 8;
    HEADER_SLACK + per_command * MAX_COMMANDS as usize + 2 * straf3_replay::MAX_NAME_BYTES as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decompressed_ceiling_admits_the_longest_legal_run() {
        // If §7.3's 8 MiB were smaller than the longest run §7.3 also permits,
        // one of the two bounds would be unreachable and the service would
        // reject runs it claims to allow. 150,000 commands with a trace is
        // about 3 MiB, so there is room — but the relationship is checked here
        // rather than assumed, because both numbers are editable.
        let longest = max_plausible_demo_bytes();
        assert!(
            longest <= MAX_DECOMPRESSED_BYTES,
            "the longest legal run is {longest} B, over the {MAX_DECOMPRESSED_BYTES} B ceiling"
        );
    }
}

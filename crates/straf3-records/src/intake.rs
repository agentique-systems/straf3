//! `POST /v1/runs`: the cheap, synchronous half of accepting a run.
//!
//! ARCHITECTURE §7.2 splits submission in two, and the split is the design
//! rather than an optimisation. Intake does everything that is cheap and must
//! be synchronous, because **a rejection is worth issuing before any CPU is
//! spent simulating**: check the ticket, decode the bytes (parsing only, no
//! simulation), take the run's identity out of the header, and let the global
//! unique index on `runs.run_digest` decide what happens.
//!
//! Nothing here re-simulates. Nothing here writes a `time_ms`. A run leaves
//! this module as `status = 'pending'` and stays unranked until a different
//! process has agreed with it.

use straf3_replay::Recording;

use crate::error::{ApiError, ApiResult};
use crate::limits;

/// A `.s3d` that decoded, with the numbers intake needs from it.
#[derive(Debug)]
pub struct Submission {
    /// The decoded recording.
    pub recording: Recording,
    /// The stored bytes — the decompressed `.s3d`, so what is served back at
    /// `/v1/runs/:id/demo` is byte-identical to what a ghost expects.
    pub bytes: Vec<u8>,
    /// SHA-256 of those bytes.
    pub sha256: Vec<u8>,
}

impl Submission {
    /// The rolling digest: the identity of the run, and the `<digest16>` a
    /// `/watch/` link carries.
    #[must_use]
    pub fn run_digest(&self) -> u64 {
        self.recording.claimed().digest
    }

    /// How many commands it took.
    #[must_use]
    pub fn commands(&self) -> i32 {
        i32::try_from(self.recording.command_count()).unwrap_or(i32::MAX)
    }

    /// The command rate. Recorded and displayed; not part of the category key
    /// (§5.2).
    #[must_use]
    pub fn tick_rate_hz(&self) -> i16 {
        i16::try_from(self.recording.start().rate.hz()).unwrap_or(i16::MAX)
    }
}

/// Turn a request body into a [`Submission`], under §7.3's bounds.
///
/// `content_encoding` is the request's `Content-Encoding`. `zstd` is
/// decompressed with an explicit window limit; anything else is treated as the
/// raw `.s3d`, because the browser client writes the file uncompressed and
/// requiring a compressor of it would buy nothing on a same-origin localhost
/// POST.
///
/// # Errors
///
/// Every bound in [`crate::limits`], and every way `.s3d` bytes can fail to be
/// `.s3d` bytes. All of them before a single command is simulated.
pub fn decode_submission(body: &[u8], content_encoding: Option<&str>) -> ApiResult<Submission> {
    let compressed = content_encoding
        .map(|e| e.trim().eq_ignore_ascii_case("zstd"))
        .unwrap_or(false);

    if compressed && body.len() > limits::MAX_COMPRESSED_BYTES {
        return Err(ApiError::payload_too_large(format!(
            "a compressed submission may be at most {} B; this one is {} B.",
            limits::MAX_COMPRESSED_BYTES,
            body.len()
        )));
    }
    if !compressed && body.len() > limits::MAX_DECOMPRESSED_BYTES {
        return Err(ApiError::payload_too_large(format!(
            "a submission may be at most {} B; this one is {} B.",
            limits::MAX_DECOMPRESSED_BYTES,
            body.len()
        )));
    }

    let bytes = if compressed {
        decompress(body)?
    } else {
        body.to_vec()
    };

    let recording = Recording::from_bytes(&bytes).map_err(|e| {
        ApiError::malformed_demo(format!(
            "these bytes are not a `.s3d` this build reads: {e}"
        ))
    })?;

    // §7.3: 150,000 commands is twenty minutes at 125 Hz. The decoder already
    // refuses a count the file does not actually contain; this refuses a run
    // that is longer than anything worth ranking.
    let commands = recording.command_count();
    if commands > limits::MAX_COMMANDS as usize {
        return Err(ApiError::payload_too_large(format!(
            "a run may be at most {} commands; this one is {commands}.",
            limits::MAX_COMMANDS
        )));
    }
    if commands == 0 {
        return Err(ApiError::malformed_demo(
            "a run with no commands is not a run.".to_string(),
        ));
    }

    let sha256 = {
        use sha2::{Digest, Sha256};
        Sha256::digest(&bytes).to_vec()
    };

    Ok(Submission {
        recording,
        bytes,
        sha256,
    })
}

/// zstd, with §7.3's explicit window limit and output ceiling.
///
/// The window limit matters separately from the output ceiling: without it a
/// small frame can declare a very large window and make the decoder allocate it
/// before one byte of output exists, so the output bound would never be
/// reached.
fn decompress(body: &[u8]) -> ApiResult<Vec<u8>> {
    use std::io::Read;

    let mut decoder = zstd::stream::read::Decoder::new(body)
        .map_err(|e| ApiError::malformed_demo(format!("the zstd frame is unreadable: {e}")))?;
    decoder
        .window_log_max(limits::ZSTD_WINDOW_LOG_MAX)
        .map_err(|e| ApiError::malformed_demo(format!("the zstd window is too large: {e}")))?;

    // One byte past the ceiling, so hitting it is distinguishable from
    // finishing exactly at it.
    let mut out = Vec::new();
    let read = decoder
        .take(limits::MAX_DECOMPRESSED_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| ApiError::malformed_demo(format!("the zstd frame did not decompress: {e}")))?;

    if read > limits::MAX_DECOMPRESSED_BYTES {
        return Err(ApiError::payload_too_large(format!(
            "the submission decompresses to more than {} B.",
            limits::MAX_DECOMPRESSED_BYTES
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use straf3_replay::{Recording, RunStart, WorldId};
    use straf3_sim::num::{s, vec3};
    use straf3_sim::world::FlatGround;
    use straf3_sim::{PhysicsProfile, TickRate, UserCmd};

    use super::*;

    fn a_recording() -> Recording {
        let world = FlatGround::at(s(0.0));
        let profile = PhysicsProfile::cpm();
        Recording::record(
            RunStart {
                rate: TickRate::HZ_125,
                spawn: vec3(s(0.0), s(0.0), s(64.0)),
                yaw: s(0.0),
            },
            vec![UserCmd::still_at(TickRate::HZ_125); 32],
            &world,
            WorldId::map("fixture", 0x0102_0304_0506_0708),
            &profile,
            "cpm",
        )
    }

    #[test]
    fn a_recording_this_build_wrote_decodes_and_keeps_its_identity() {
        let recording = a_recording();
        let bytes = recording.to_bytes_with_checksums().unwrap();
        let submission = decode_submission(&bytes, None).unwrap();

        assert_eq!(submission.run_digest(), recording.claimed().digest);
        assert_eq!(submission.commands(), 32);
        assert_eq!(submission.tick_rate_hz(), 125);
        assert_eq!(
            submission.bytes, bytes,
            "the stored bytes are the sent bytes"
        );
    }

    #[test]
    fn the_zstd_path_reaches_the_same_recording() {
        let recording = a_recording();
        let bytes = recording.to_bytes_with_checksums().unwrap();
        let compressed = zstd::encode_all(bytes.as_slice(), 3).unwrap();

        let submission = decode_submission(&compressed, Some("zstd")).unwrap();
        assert_eq!(submission.run_digest(), recording.claimed().digest);
        assert_eq!(submission.bytes, bytes);
    }

    #[test]
    fn a_body_over_the_ceiling_is_refused_before_it_is_parsed() {
        let too_big = vec![0u8; limits::MAX_DECOMPRESSED_BYTES + 1];
        let err = decode_submission(&too_big, None).unwrap_err();
        assert_eq!(err.code, "demo_too_large");

        let too_big_compressed = vec![0u8; limits::MAX_COMPRESSED_BYTES + 1];
        let err = decode_submission(&too_big_compressed, Some("zstd")).unwrap_err();
        assert_eq!(err.code, "demo_too_large");
    }

    #[test]
    fn a_zstd_bomb_hits_the_output_ceiling_rather_than_the_allocator() {
        // ~64 MiB of zeroes compresses to a few kilobytes and is well inside
        // the compressed-body bound, which is exactly why the *decompressed*
        // bound has to exist separately.
        let bomb = zstd::encode_all(vec![0u8; 64 * 1024 * 1024].as_slice(), 9).unwrap();
        assert!(bomb.len() < limits::MAX_COMPRESSED_BYTES);
        let err = decode_submission(&bomb, Some("zstd")).unwrap_err();
        assert_eq!(err.code, "demo_too_large");
    }

    #[test]
    fn bytes_that_are_not_an_s3d_are_refused_with_the_reason() {
        let err = decode_submission(b"not a demo at all, but long enough", None).unwrap_err();
        assert_eq!(err.code, "malformed_demo");

        let err = decode_submission(&[], None).unwrap_err();
        assert_eq!(err.code, "malformed_demo");
    }

    #[test]
    fn a_truncated_recording_is_refused_rather_than_half_parsed() {
        let bytes = a_recording().to_bytes();
        let err = decode_submission(&bytes[..bytes.len() - 20], None).unwrap_err();
        assert_eq!(err.code, "malformed_demo");
    }
}

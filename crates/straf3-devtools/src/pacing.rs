//! Recording how long each frame actually took.
//!
//! # Why this is a devtool and not part of the loop
//!
//! Acceptance criterion 7 asks for measured frame times — mean, p50, p99 and
//! max — from a release build on the real GPU, vsynced and uncapped. That is a
//! *measurement*, and the first rule of one is that it must not change the
//! thing it measures. So:
//!
//! - The simulation is not involved. It keeps taking whole-millisecond deltas
//!   from `straf3_platform::Clock`, on exactly the code path it uses when
//!   nothing is being recorded. This type reads its own [`Instant`] and hands
//!   the number to nobody.
//! - Nothing is written during the session. [`PacingLog::frame`] pushes one
//!   `u64` into a `Vec` that was reserved up front, so a frame costs an
//!   increment and a store — no allocation, no syscall, no formatting. The
//!   file is written once, at the end, by [`PacingLog::write_csv`].
//!
//! # Nanoseconds, not milliseconds
//!
//! 165 Hz is 6.0606 ms. Truncating each frame to whole milliseconds turns a
//! distribution with a real spread into a two-valued one and destroys the p99
//! entirely, which is the number most worth having.
//!
//! # The format
//!
//! ```text
//! # straf3 pacing log v1
//! # present_mode=fifo  build=release
//! frame,delta_ns
//! 0,6060606
//! 1,6048112
//! ```
//!
//! Every line before `frame,delta_ns` starts with `#` and carries
//! `key=value` pairs. A reader must skip `#` lines it does not recognise
//! rather than fail on them, so context can be added later without breaking
//! anything that already parses these files.
//!
//! ## `present_mode` and `present_mode_requested` are different claims
//!
//! `present_mode=` means the surface was asked what it was configured with —
//! `straf3_render::Renderer::present_mode`. `present_mode_requested=` means
//! the writer only knows what it *wanted*, typically the value it put in
//! `STRAF3_PRESENT_MODE`, and an unsupported request falls back to FIFO
//! silently as far as the requester is concerned.
//!
//! A field named `present_mode` that held a request would be a correct-looking
//! artefact carrying something nobody meant, and it would eventually be quoted
//! as "measured under Immediate" for a run that vsynced. The two names make
//! that misreading impossible rather than merely discouraged.
//!
//! Frame 0's delta is the interval from [`PacingLog::start`] to the first
//! [`PacingLog::frame`]. It is normally an outlier — it contains window
//! creation and device acquisition — and analysis is expected to drop it, but
//! it is recorded rather than hidden, because a tool that silently discards
//! its own worst sample is not measuring.

use std::io::Write as _;
use std::path::Path;
use std::time::Instant;

/// The first line of every pacing log. A reader should refuse a file that does
/// not start with this rather than guess at the columns.
pub const MAGIC: &str = "# straf3 pacing log v1";

/// Per-frame wall-clock deltas, collected in memory and written once.
#[derive(Debug)]
pub struct PacingLog {
    deltas_ns: Vec<u64>,
    previous: Option<Instant>,
    present_mode: String,
    /// The header key the mode is written under. `present_mode` means the
    /// surface was asked what it was configured with; `present_mode_requested`
    /// means the caller only knows what it wanted. The distinction lives in
    /// the *name* rather than in a separate field, so a reader skimming for a
    /// number cannot mistake one for the other.
    mode_key: &'static str,
    /// Free-form `key=value` pairs written into the header.
    notes: Vec<(String, String)>,
}

impl PacingLog {
    /// A log sized for roughly `frames` frames.
    ///
    /// Reserve generously: the point of the reservation is that no frame ever
    /// pays for a `Vec` growth, and 60 seconds at 1000 fps is 480 KiB.
    #[must_use]
    pub fn with_capacity(frames: usize) -> Self {
        Self {
            deltas_ns: Vec::with_capacity(frames),
            previous: None,
            present_mode: "unknown".to_owned(),
            mode_key: "present_mode_requested",
            notes: Vec::new(),
        }
    }

    /// A log sized for `seconds` at `fps`, with headroom.
    #[must_use]
    pub fn for_session(seconds: u64, fps: u64) -> Self {
        Self::with_capacity(usize::try_from(seconds.saturating_mul(fps).saturating_add(1024)).unwrap_or(1 << 20))
    }

    /// Record the present mode the surface was **actually** configured with,
    /// as reported by `straf3_render::Renderer::present_mode`.
    ///
    /// Not the one that was requested. `straf3_render::present::choose` falls
    /// back when a mode is unsupported, and a header naming the request rather
    /// than the grant would mislabel every row beneath it. Callers that only
    /// know what they asked for must use [`PacingLog::set_requested_mode`]
    /// instead, so the reader can tell the two apart.
    pub fn set_present_mode(&mut self, mode: &str) {
        self.present_mode = mode.to_owned();
        self.mode_key = "present_mode";
    }

    /// Record the present mode that was *asked for*, when the granted mode is
    /// not available to the caller.
    ///
    /// This writes the header key `present_mode_requested=`, and `xtask
    /// pacing` prints a caveat rather than reporting the value as the mode
    /// measured. A request is not a measurement: an unsupported mode
    /// falls back silently as far as the requester is concerned, and a run
    /// labelled `immediate` that actually ran in FIFO would be a fiction with
    /// a number attached.
    pub fn set_requested_mode(&mut self, mode: &str) {
        self.present_mode = mode.to_owned();
        self.mode_key = "present_mode_requested";
    }

    /// Add a `key=value` pair to the header.
    pub fn note(&mut self, key: &str, value: &str) {
        self.notes.push((key.to_owned(), value.to_owned()));
    }

    /// Begin timing. Call once, at the point the first frame's interval should
    /// start from.
    pub fn start(&mut self) {
        self.previous = Some(Instant::now());
    }

    /// Record one frame boundary. Call at exactly one point in the frame loop,
    /// the same point every frame.
    pub fn frame(&mut self) {
        let now = Instant::now();
        if let Some(previous) = self.previous {
            self.deltas_ns
                .push(u64::try_from(now.duration_since(previous).as_nanos()).unwrap_or(u64::MAX));
        }
        self.previous = Some(now);
    }

    /// How many intervals have been recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.deltas_ns.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.deltas_ns.is_empty()
    }

    /// The recorded intervals, in order.
    #[must_use]
    pub fn deltas_ns(&self) -> &[u64] {
        &self.deltas_ns
    }

    /// Render the log as CSV text.
    #[must_use]
    pub fn to_csv(&self) -> String {
        let build = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        // 24 bytes a row covers "999999,999999999\n" with room to spare.
        let mut out = String::with_capacity(128 + self.deltas_ns.len() * 24);
        out.push_str(MAGIC);
        out.push('\n');
        out.push_str(&format!(
            "# {}={}  build={build}\n",
            self.mode_key, self.present_mode
        ));
        for (key, value) in &self.notes {
            out.push_str(&format!("# {key}={value}\n"));
        }
        out.push_str("frame,delta_ns\n");
        for (i, ns) in self.deltas_ns.iter().enumerate() {
            out.push_str(&format!("{i},{ns}\n"));
        }
        out
    }

    /// Write the log to `path`.
    ///
    /// # Errors
    ///
    /// If the file cannot be created or written.
    pub fn write_csv(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        file.write_all(self.to_csv().as_bytes())?;
        file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_recorded_before_start() {
        let mut log = PacingLog::with_capacity(8);
        log.frame();
        // The first `frame` after `start` opens the first interval; a `frame`
        // with no `start` has nothing to measure from and must not invent one.
        assert_eq!(log.len(), 0);
        log.frame();
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn n_frames_after_start_give_n_intervals() {
        let mut log = PacingLog::with_capacity(8);
        log.start();
        for _ in 0..5 {
            log.frame();
        }
        assert_eq!(log.len(), 5);
        assert!(!log.is_empty());
    }

    #[test]
    fn the_csv_says_what_it_is_and_which_mode_it_was() {
        let mut log = PacingLog::with_capacity(4);
        log.set_present_mode("immediate");
        log.note("map", "coil");
        log.start();
        log.frame();
        log.frame();

        let csv = log.to_csv();
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(MAGIC));
        let header = lines.next().unwrap();
        assert!(header.contains("present_mode=immediate"), "{header}");
        // The build is stamped by the compiler, not by the caller, so a
        // release number can never be published from a debug run by mistake.
        assert!(
            header.contains(if cfg!(debug_assertions) {
                "build=debug"
            } else {
                "build=release"
            }),
            "{header}"
        );
        assert_eq!(lines.next(), Some("# map=coil"));
        assert_eq!(lines.next(), Some("frame,delta_ns"));

        let rows: Vec<&str> = lines.collect();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].starts_with("0,"));
        assert!(rows[1].starts_with("1,"));
    }

    #[test]
    fn a_merely_requested_mode_is_marked_as_one() {
        // The distinction exists because `STRAF3_PRESENT_MODE=immediate` on an
        // adapter without Immediate silently runs in FIFO. A log that recorded
        // the request as though it were the outcome would attach a real number
        // to a mode that never happened.
        let mut log = PacingLog::with_capacity(2);
        log.set_requested_mode("immediate");
        let csv = log.to_csv();
        assert!(csv.contains("# present_mode_requested=immediate"), "{csv}");
        assert!(!csv.contains("# present_mode="), "{csv}");

        // Saying nothing at all must not look like a configured mode either.
        let mut log = PacingLog::with_capacity(2);
        assert!(log.to_csv().contains("present_mode_requested=unknown"));

        log.set_present_mode("fifo");
        let csv = log.to_csv();
        assert!(csv.contains("# present_mode=fifo"), "{csv}");
        assert!(!csv.contains("present_mode_requested"), "{csv}");
    }

    #[test]
    fn the_deltas_are_real_elapsed_nanoseconds() {
        let mut log = PacingLog::with_capacity(4);
        log.start();
        std::thread::sleep(std::time::Duration::from_millis(2));
        log.frame();
        // Two milliseconds is 2 000 000 ns; a millisecond-truncating recorder
        // would have produced 2, which is the bug this whole type avoids.
        assert!(log.deltas_ns()[0] >= 1_500_000, "{:?}", log.deltas_ns());
    }
}

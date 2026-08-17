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
//! # straf3 pacing log v2
//! # present_mode=fifo  build=release
//! # warmup_ns=50225900  frames=1164
//! frame,delta_ns
//! 1,6060606
//! 2,6048112
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
//! ## Why the warm-up is a header and not a row (this is what v2 changed)
//!
//! The first interval a session records spans swapchain warm-up, not a frame
//! anyone drew at a steady rate. Measured on this hardware it has come back at
//! 49 ms, 421 ms and 50 ms against a steady ~6 ms. Left in the data, a reader
//! that took every row at face value would publish a 421 ms worst-case frame
//! time on a 165 Hz display — which reads as a finding rather than as a
//! swapchain waking up.
//!
//! Putting it in `warmup_ns=` makes the wrong answer unreachable instead of
//! warned against: **every data row is a measurement**, and statistics can be
//! taken over all of them with no special case to forget. The number is not
//! hidden — it is still there, named for what it is.
//!
//! Data rows are numbered from 1, the true frame index, so they line up with
//! the session's own logs rather than with a renumbering.
//!
//! A session that draws fewer than three frames has no steady-state interval
//! at all — one interval, and that one is the warm-up — so it writes no file
//! and says why. An absent log means "nothing measurable happened", which is a
//! better answer than a file whose p99 is taken over an empty set.
//!
//! The tag went v1 → v2 rather than staying put with a note, because a v1
//! reader handed a v2 file would silently get one fewer sample than it
//! expected and never find out. Bumping makes it refuse.

use std::io::Write as _;
use std::path::Path;
use std::time::Instant;

/// The first line of every pacing log. A reader should refuse a file that does
/// not start with this rather than guess at the columns — including a v1 file,
/// whose first data row is a warm-up interval that v2 readers do not expect.
pub const MAGIC: &str = "# straf3 pacing log v2";

/// Fewer rendered frames than this and there is no steady-state interval to
/// report: the only interval recorded would be the warm-up.
pub const MIN_FRAMES: usize = 3;

/// Per-frame wall-clock deltas, collected in memory and written once.
#[derive(Debug)]
pub struct PacingLog {
    deltas_ns: Vec<u64>,
    previous: Option<Instant>,
    present_mode: String,
    /// The first interval recorded, which is swapchain warm-up rather than a
    /// frame time. Held out of `deltas_ns` and written as a header.
    warmup_ns: Option<u64>,
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
            warmup_ns: None,
            mode_key: "present_mode_requested",
            notes: Vec::new(),
        }
    }

    /// A log sized for `seconds` at `fps`, with headroom.
    #[must_use]
    pub fn for_session(seconds: u64, fps: u64) -> Self {
        Self::with_capacity(
            usize::try_from(seconds.saturating_mul(fps).saturating_add(1024)).unwrap_or(1 << 20),
        )
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
    ///
    /// The first interval is captured as the warm-up and does not become a
    /// data row. See the module docs: it spans swapchain warm-up and is not a
    /// frame time, and leaving it in the rows is how a 421 ms sample gets
    /// published as a worst-case frame.
    pub fn frame(&mut self) {
        let now = Instant::now();
        if let Some(previous) = self.previous {
            let ns = u64::try_from(now.duration_since(previous).as_nanos()).unwrap_or(u64::MAX);
            match self.warmup_ns {
                None => self.warmup_ns = Some(ns),
                Some(_) => self.deltas_ns.push(ns),
            }
        }
        self.previous = Some(now);
    }

    /// The warm-up interval, once one has been recorded.
    #[must_use]
    pub fn warmup_ns(&self) -> Option<u64> {
        self.warmup_ns
    }

    /// Whether there is enough here to be worth writing.
    ///
    /// False when the session drew too few frames to have a steady-state
    /// interval. A caller should log that rather than write a file whose
    /// statistics would be taken over nothing.
    #[must_use]
    pub fn is_measurable(&self) -> bool {
        !self.deltas_ns.is_empty()
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
        // `frames` is the writer's own row count, so a truncated file is
        // detectable by a reader rather than merely shorter than it should be.
        out.push_str(&format!(
            "# warmup_ns={}  frames={}\n",
            self.warmup_ns.unwrap_or(0),
            self.deltas_ns.len()
        ));
        out.push_str(
            "# warmup_ns is the first rendered interval: swapchain warm-up, not a frame \
             time, and NOT a data row below\n",
        );
        for (key, value) in &self.notes {
            out.push_str(&format!("# {key}={value}\n"));
        }
        out.push_str("frame,delta_ns\n");
        // Numbered from 1: the true frame index, so rows line up with the
        // session's own logs instead of with a renumbering.
        for (i, ns) in self.deltas_ns.iter().enumerate() {
            out.push_str(&format!("{},{ns}\n", i + 1));
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
        assert_eq!(log.warmup_ns(), None);
    }

    #[test]
    fn the_first_interval_becomes_the_warm_up_and_not_a_data_row() {
        // v2's whole point. Five frames after `start` give one warm-up
        // interval and four data rows, not five rows one of which is a lie.
        let mut log = PacingLog::with_capacity(8);
        log.start();
        for _ in 0..5 {
            log.frame();
        }
        assert!(log.warmup_ns().is_some());
        assert_eq!(log.len(), 4);
        assert!(log.is_measurable());

        let csv = log.to_csv();
        assert!(csv.contains("frames=4"), "{csv}");
        assert!(csv.contains("warmup_ns="), "{csv}");
        // Rows are the true frame index, starting at 1.
        let rows: Vec<&str> = csv
            .lines()
            .skip_while(|l| *l != "frame,delta_ns")
            .skip(1)
            .collect();
        assert_eq!(rows.len(), 4);
        assert!(rows[0].starts_with("1,"), "{:?}", rows[0]);
        assert!(rows[3].starts_with("4,"), "{:?}", rows[3]);
    }

    #[test]
    fn a_session_too_short_to_measure_says_so() {
        // One interval is the warm-up, so there is nothing left. A caller must
        // be able to tell that apart from "the measurement was zero".
        let mut log = PacingLog::with_capacity(4);
        log.start();
        log.frame();
        assert!(log.warmup_ns().is_some());
        assert!(!log.is_measurable());
        assert_eq!(log.len(), 0);

        log.frame();
        assert!(log.is_measurable());
    }

    #[test]
    fn the_csv_says_what_it_is_and_which_mode_it_was() {
        let mut log = PacingLog::with_capacity(4);
        log.set_present_mode("immediate");
        log.note("map", "coil");
        log.start();
        log.frame();
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
        assert!(lines.next().unwrap().starts_with("# warmup_ns="));
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with("# warmup_ns is the first")
        );
        assert_eq!(lines.next(), Some("# map=coil"));
        assert_eq!(lines.next(), Some("frame,delta_ns"));

        let rows: Vec<&str> = lines.collect();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].starts_with("1,"));
        assert!(rows[1].starts_with("2,"));
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
        log.frame(); // consumed as the warm-up
        std::thread::sleep(std::time::Duration::from_millis(2));
        log.frame();
        // Two milliseconds is 2 000 000 ns; a millisecond-truncating recorder
        // would have produced 2, which is the bug this whole type avoids.
        assert!(log.deltas_ns()[0] >= 1_500_000, "{:?}", log.deltas_ns());
    }
}

//! The wall clock, in whole milliseconds.
//!
//! # Why this is the only place time is read
//!
//! The simulation never asks what time it is (`straf3-sim`'s seam check
//! forbids `Instant` and `SystemTime` outright). Frames are converted to
//! whole-millisecond commands *above* the line, and this module is where that
//! conversion starts: the clock is read once per frame and immediately
//! truncated to an integer.
//!
//! # Why the truncation cannot drift
//!
//! Pacing uses the *difference of two truncated readings*, never a truncated
//! difference. Reading 16.7 ms and 33.4 ms as 16 and 33 gives deltas of 16 and
//! 17 — the fractional part is carried by the absolute reading, not discarded,
//! so a long session's tick count still tracks real elapsed time. Truncating
//! each delta independently would lose up to 1 ms per frame, which at 200 fps
//! is 12 % of the simulation running slow.

/// Where the wall clock and the simulation clock meet.
///
/// The simulation itself knows nothing about wall time (spec D2): frames are
/// converted to whole-millisecond commands here, above the seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct FrameTiming {
    /// Milliseconds elapsed since the process started.
    pub elapsed_ms: u64,
}

/// One frame's worth of wall time: where we are, and how far we moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameDelta {
    /// Absolute elapsed time at the moment the frame started.
    pub timing: FrameTiming,
    /// Whole milliseconds since the previous [`Clock::frame`] call.
    ///
    /// Zero is a normal value, not an error: a machine rendering faster than
    /// 1000 fps produces it, and so does the first frame.
    pub delta_ms: u64,
}

/// A monotonic millisecond clock.
///
/// Native reads [`std::time::Instant`]; the web build reads
/// `performance.now()`, because `Instant::now()` panics outright on
/// `wasm32-unknown-unknown`. That difference is the entire reason this type
/// exists rather than the caller reading a clock directly.
#[derive(Debug)]
pub struct Clock {
    inner: Inner,
    last_ms: u64,
}

impl Clock {
    /// Start a clock. Elapsed time is measured from this call.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Inner::new(),
            last_ms: 0,
        }
    }

    /// Whole milliseconds since [`Clock::new`], without advancing the frame.
    #[must_use]
    pub fn now(&self) -> FrameTiming {
        FrameTiming {
            elapsed_ms: self.inner.elapsed_ms(),
        }
    }

    /// Read the clock and report how far it moved since the last call.
    ///
    /// `saturating_sub` rather than a subtraction: a clock that appears to go
    /// backwards (a browser tab restored, a suspended VM) yields a zero delta
    /// and no ticks, instead of an underflow.
    pub fn frame(&mut self) -> FrameDelta {
        let elapsed_ms = self.inner.elapsed_ms();
        let delta_ms = elapsed_ms.saturating_sub(self.last_ms);
        self.last_ms = elapsed_ms;
        FrameDelta {
            timing: FrameTiming { elapsed_ms },
            delta_ms,
        }
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct Inner {
    start: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl Inner {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }

    fn elapsed_ms(&self) -> u64 {
        // `as_millis` truncates, which is what we want — see the module docs.
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
struct Inner {
    /// `performance.now()` at construction, in fractional milliseconds.
    ///
    /// `None` when there is no `Performance` object at all (a worker context
    /// with the API disabled). The clock then reports a frozen zero, which
    /// stalls the simulation rather than making up time.
    origin: Option<f64>,
}

#[cfg(target_arch = "wasm32")]
impl Inner {
    fn new() -> Self {
        Self {
            origin: performance_now(),
        }
    }

    fn elapsed_ms(&self) -> u64 {
        match (self.origin, performance_now()) {
            (Some(origin), Some(now)) => {
                let d = now - origin;
                if d <= 0.0 { 0 } else { d as u64 }
            }
            _ => 0,
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn performance_now() -> Option<f64> {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_is_whole_milliseconds() {
        assert_eq!(FrameTiming { elapsed_ms: 8 }.elapsed_ms, 8);
    }

    #[test]
    fn the_first_frame_has_no_delta_to_speak_of() {
        let mut clock = Clock::new();
        let first = clock.frame();
        // Whatever the machine's speed, constructing a clock and reading it
        // cannot have taken a second.
        assert!(first.delta_ms < 1_000);
    }

    #[test]
    fn elapsed_time_never_goes_backwards() {
        let mut clock = Clock::new();
        let mut previous = 0;
        for _ in 0..1_000 {
            let frame = clock.frame();
            assert!(frame.timing.elapsed_ms >= previous);
            previous = frame.timing.elapsed_ms;
        }
    }
}

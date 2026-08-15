//! Windowing, raw input, and timing.
//!
//! Above the seam: this is where `winit` is allowed to exist. Nothing below
//! the line may reach this crate.
//! Stub — the event loop and raw-input path land in a later wave.

/// Where the wall clock and the simulation clock meet.
///
/// The simulation itself knows nothing about wall time (spec D2): frames are
/// converted to whole-millisecond commands here, above the seam.
#[derive(Debug, Clone, Copy)]
pub struct FrameTiming {
    /// Milliseconds elapsed since the process started.
    pub elapsed_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_is_whole_milliseconds() {
        assert_eq!(FrameTiming { elapsed_ms: 8 }.elapsed_ms, 8);
    }
}

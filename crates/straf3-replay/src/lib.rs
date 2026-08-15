//! Input recording and playback.
//!
//! Below the seam. Depends on `straf3-sim` for the command type; `straf3-sim`
//! must never depend on this crate.
//! Stub — the demo format lands in a later wave.

/// A recorded run: the inputs, not the resulting positions.
///
/// Storing inputs rather than states is what makes a replay a regression test:
/// replaying it re-runs the simulation and must reproduce the same result.
#[derive(Debug, Clone, Default)]
pub struct Recording {
    /// Simulation command rate in hertz, recorded because it changes the
    /// physics (spec D2).
    pub command_rate_hz: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_recording_has_no_rate_yet() {
        assert_eq!(Recording::default().command_rate_hz, 0);
    }
}

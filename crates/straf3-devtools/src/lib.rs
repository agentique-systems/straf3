//! egui overlays and movement telemetry.
//!
//! Above the seam. Reads simulation state to display it; never feeds anything
//! back into the simulation.
//! Stub — the speedometer, strafe-efficiency graph and trace overlays land in
//! a later wave.

/// One sample of movement telemetry, as shown on the overlay.
#[derive(Debug, Clone, Copy, Default)]
pub struct TelemetrySample {
    /// Horizontal speed in units per second — the number that matters.
    pub horizontal_speed: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sample_is_stationary() {
        assert_eq!(TelemetrySample::default().horizontal_speed, 0.0);
    }
}

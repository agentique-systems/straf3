//! Swept convex queries against static geometry.
//!
//! Below the seam: no rendering, no windowing, no GPU, no filesystem.
//! Stub — the real trace routines land in a later wave.

/// Result of sweeping a convex shape through static geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TraceResult {
    /// Fraction of the requested motion actually travelled, in `0.0..=1.0`.
    pub fraction: f32,
    /// Surface normal at the impact point, or zero when nothing was hit.
    pub normal: glam::Vec3,
}

impl TraceResult {
    /// A trace that completed the whole sweep without touching anything.
    pub const fn clear() -> Self {
        Self {
            fraction: 1.0,
            normal: glam::Vec3::ZERO,
        }
    }

    /// Whether the sweep was interrupted by geometry.
    pub fn hit(&self) -> bool {
        self.fraction < 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_trace_does_not_hit() {
        assert!(!TraceResult::clear().hit());
    }
}

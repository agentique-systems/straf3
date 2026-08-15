//! wgpu + WGSL renderer.
//!
//! Above the seam: this is where the GPU is allowed to exist. Nothing below
//! the line may reach this crate — which is what lets the simulation run in
//! CI with no GPU at all (spec criterion 14).
//! Stub — device setup and the WGSL pipelines land in a later wave.

/// How far between two simulation states the rendered frame sits, `0.0..=1.0`.
///
/// Rendering interpolates; it never advances the simulation (spec D2).
#[derive(Debug, Clone, Copy)]
pub struct InterpolationAlpha(pub f32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_is_carried_verbatim() {
        assert_eq!(InterpolationAlpha(0.5).0, 0.5);
    }
}

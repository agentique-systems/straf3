//! Valve 220 `.map` loading and the compiled map format.
//!
//! Below the seam. Parsing is delegated to the `quake-map` crate; this crate's
//! own work is the **compile** step — turning brush planes into the convex
//! hulls collision wants and the mesh rendering wants. No BSP tree and no PVS:
//! Defrag maps are small and convex brush hulls are sufficient (spec D4).
//!
//! Note the signature: [`compile`] takes `&str`, not a path. Reading bytes off
//! disk is the caller's job, above the seam. That is what lets `straf3-sim`
//! consume map data while knowing nothing about files.
//!
//! Stub — the brush-to-hull compiler lands in a later wave.

/// A compiled map: convex hulls for collision, plus what rendering needs.
#[derive(Debug, Clone, Default)]
pub struct CompiledMap {
    /// Player spawn position, in world units.
    pub spawn: glam::Vec3,
}

/// Errors produced while compiling `.map` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The source was not valid Valve 220 text.
    Malformed,
}

/// Compile Valve 220 `.map` source text into a map ready for simulation.
///
/// Takes text, not a path, on purpose — see the module docs.
pub fn compile(_source: &str) -> Result<CompiledMap, CompileError> {
    Ok(CompiledMap::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_compiles_to_a_default_map() {
        assert_eq!(compile("").unwrap().spawn, glam::Vec3::ZERO);
    }
}

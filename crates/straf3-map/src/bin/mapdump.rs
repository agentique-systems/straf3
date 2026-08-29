//! Print a compiled map's geometry: every solid hull's extents, every trigger
//! volume, and the collision digest.
//!
//! # Why this exists
//!
//! Because nothing else in the tree prints per-solid hull extents. `hull.mins`
//! and `hull.maxs` are public on `CompiledMap::hulls`, and the only non-test
//! reader of them is `digest.rs`, which folds them into a hash rather than
//! showing them. `tools/straf3-agent`'s report prints `hulls.len()`, the map's
//! global bounds and per-*trigger* bounds — never a per-solid extent.
//!
//! That gap matters for one specific claim. A map that separates two route
//! branches with a wall can only demonstrate the separation from the compiled
//! solids: where the wall starts, where it stops, and how tall it is. Prose
//! cannot establish it and a global bounding box cannot either. So a reviewer
//! who has to confirm "these two branches are separated by solid geometry" has,
//! until now, had no instrument at all and could only return "unresolved, no
//! instrument".
//!
//! # What it is, deliberately
//!
//! A dump of public fields. It computes nothing, decides nothing, and has no
//! thresholds — every number it prints is read straight off the value
//! `straf3_map::compile` returned. That is what makes it usable as evidence by
//! someone who does not trust the person who wrote it: the output is
//! cross-checkable against the hull count and the digest, both of which other
//! tools already print, and against the `.map` source itself.
//!
//! The digest is printed with the hulls rather than separately because
//! `docs/movement-agent.md` requires a published stream to be quoted with the
//! map's `collision_digest`, and a verifier to check that *before* comparing
//! checksums — a command stream replayed against the wrong map does not fail
//! loudly, it just produces a different run.
//!
//! ```sh
//! cargo run -p straf3-map --bin mapdump -- assets/maps/cleave.map
//! cargo run -p straf3-map --bin mapdump -- assets/maps/cleave.map --hulls-only
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!(
            "usage: mapdump <file.map> [--hulls-only | --triggers-only]\n\
             \n\
             Prints every compiled solid hull's index, extents, size, surface\n\
             flags and plane count; every trigger volume's kind, target,\n\
             resolved classname and bounds; and the collision digest."
        );
        return ExitCode::FAILURE;
    };
    let hulls_only = args.iter().any(|a| a == "--hulls-only");
    let triggers_only = args.iter().any(|a| a == "--triggers-only");

    let source = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("mapdump: {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let map = match straf3_map::compile(&source) {
        Ok(map) => map,
        Err(e) => {
            eprintln!("mapdump: {path}: {e:?}");
            return ExitCode::FAILURE;
        }
    };

    println!("map              {path}");
    println!(
        "collision digest {:#018x}   full digest {:#018x}",
        map.collision_digest(),
        map.full_digest()
    );
    println!(
        "counts           {} solid hulls, {} trigger volumes, {} triangles",
        map.hulls.len(),
        map.triggers.len(),
        map.mesh.indices.len() / 3
    );
    println!(
        "bounds           {:?} .. {:?}",
        map.bounds.mins, map.bounds.maxs
    );
    println!("spawn            {:?} yaw {}", map.spawn, map.spawn_yaw);
    for w in &map.warnings {
        println!("warning          {w:?}");
    }

    if !triggers_only {
        println!("\n== solid hulls, in source order ==");
        println!(
            "{:>4}  {:>26}  {:>26}  {:>22}  {:>6}  surface",
            "idx", "mins", "maxs", "size", "planes"
        );
        for (i, h) in map.hulls.iter().enumerate() {
            let size = h.maxs - h.mins;
            println!(
                "{i:>4}  {:>8.1} {:>8.1} {:>8.1}  {:>8.1} {:>8.1} {:>8.1}  \
                 {:>6.0} {:>6.0} {:>6.0}  {:>6}  {:?}",
                h.mins.x,
                h.mins.y,
                h.mins.z,
                h.maxs.x,
                h.maxs.y,
                h.maxs.z,
                size.x,
                size.y,
                size.z,
                h.planes.len(),
                h.surface
            );
        }
    }

    if !hulls_only {
        println!("\n== trigger volumes, in source order ==");
        println!(
            "{:>4}  {:<16}  {:<20}  {:<22}  bounds",
            "idx", "target", "resolved classname", "kind"
        );
        for (i, t) in map.triggers.iter().enumerate() {
            println!(
                "{i:>4}  {:<16}  {:<20}  {:<22}  {:>8.1} {:>8.1} {:>8.1} .. {:>8.1} {:>8.1} {:>8.1}",
                t.target.as_deref().unwrap_or("<none>"),
                t.target_classname.as_deref().unwrap_or("<unresolved>"),
                format!("{:?}", t.kind),
                t.bounds.mins.x,
                t.bounds.mins.y,
                t.bounds.mins.z,
                t.bounds.maxs.x,
                t.bounds.maxs.y,
                t.bounds.maxs.z,
            );
        }
    }
    ExitCode::SUCCESS
}

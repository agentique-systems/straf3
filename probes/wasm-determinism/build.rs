//! Makes a *build-time* copy of `straf3-sim`'s sources with the three
//! `sin_cos` calls in `angle_vectors` redirected to [`crate::dettrig`], so the
//! probe can answer "would owning our trig fix it?" end to end — whole
//! physics, both platforms — instead of only at the level of one operation.
//!
//! The copy lives in `OUT_DIR`. Nothing under `crates/` is read for anything
//! but its bytes, and nothing under `crates/` is written.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let src = Path::new("../../crates/straf3-sim/src");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("patched");
    fs::create_dir_all(&out).unwrap();

    for entry in fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let text = fs::read_to_string(&path).unwrap();
        fs::write(out.join(&name), transform(&name, &text)).unwrap();
    }
}

fn transform(name: &str, text: &str) -> String {
    // `crate::` inside the copy must reach the copy, which is a module of this
    // crate rather than its root.
    let mut t = text.replace("crate::", "crate::patched_sim::");

    if name == "lib.rs" {
        // Inner attributes and inner doc comments are illegal in an
        // `include!`d module body, and the module declarations have to point
        // at OUT_DIR explicitly.
        t = t
            .lines()
            .filter(|l| {
                let s = l.trim_start();
                !s.starts_with("#![") && !s.starts_with("//!")
            })
            .map(|l| {
                let trimmed = l.trim_start();
                if let Some(rest) = trimmed.strip_prefix("pub mod ") {
                    let m = rest.trim_end_matches(';');
                    return format!("#[path = \"{m}.rs\"] pub mod {m};");
                }
                l.to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
    }

    if name == "step.rs" {
        // Matched against the call sites themselves rather than the bare text
        // `.sin_cos()`, which since C1 also appears in step.rs's prose about
        // why the call sites are gone.
        let remaining = CALL_SITES
            .iter()
            .filter(|(from, _)| t.contains(from))
            .count();

        // C1 has landed: `angle_vectors` calls `num::sin_cos`, the owned
        // implementation, and there is nothing left here to redirect. The
        // probe's question — "would owning our trig fix it?" — is answered and
        // shipped, so this build script has no work to do.
        //
        // Saying so out loud rather than skipping quietly, because with no
        // patch applied `patched_sim` is an exact twin of the real sim, and
        // any patched-vs-unpatched comparison the probe draws from it is a
        // tautology rather than a measurement. The results in README.md
        // describe the tree as it was *before* C1 and are not reproducible
        // against this one.
        if remaining == 0 && t.contains("num::sin_cos(") {
            println!(
                "cargo:warning=C1 has landed: straf3-sim's angle_vectors now calls \
                 num::sin_cos, the same algorithm as dettrig::det_sin_cos (verified \
                 bit-identical over 570,425,346 samples by probes/c1-owned-trig). \
                 `patched_sim` is therefore an exact copy of the real sim, and any \
                 patched-vs-unpatched comparison this probe draws is now vacuous. \
                 README.md's results describe the pre-C1 tree."
            );
            return t;
        }

        // The whole patch: three call sites, nothing else.
        assert_eq!(
            remaining,
            CALL_SITES.len(),
            "expected exactly {} sin_cos call sites in step.rs",
            CALL_SITES.len(),
        );
        for (from, to) in CALL_SITES {
            t = t.replace(from, to);
        }
        assert!(
            !CALL_SITES.iter().any(|(from, _)| t.contains(from)),
            "a sin_cos call site was not redirected — the patch is stale"
        );
    }

    t
}

/// The three lines the patch rewrites, and what it rewrites them to.
const CALL_SITES: [(&str, &str); 3] = [
    (
        "let (sy, cy) = (yaw * DEG_TO_RAD).sin_cos();",
        "let (sy, cy) = crate::dettrig::det_sin_cos(yaw * DEG_TO_RAD);",
    ),
    (
        "let (sp, cp) = (pitch * DEG_TO_RAD).sin_cos();",
        "let (sp, cp) = crate::dettrig::det_sin_cos(pitch * DEG_TO_RAD);",
    ),
    (
        "let (sr, cr) = (roll * DEG_TO_RAD).sin_cos();",
        "let (sr, cr) = crate::dettrig::det_sin_cos(roll * DEG_TO_RAD);",
    ),
];

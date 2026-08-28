//! Is this harness verifying the tree it claims to be verifying?
//!
//! `webcheck` is a standalone crate with its own `Cargo.lock` (see
//! `Cargo.toml` for why). That buys isolation and costs a guarantee: nothing
//! structurally forces `glam` here to be the `glam` the workspace resolved,
//! and `glam` is a floating-point library whose version is part of what makes
//! a digest what it is. A harness quietly built against a different resolution
//! would produce a disagreement that says nothing about the browser.
//!
//! So the guarantee is re-established as a check instead of assumed as a
//! property. Before reporting any verdict, `webcheck` reads the workspace's
//! `Cargo.lock` and its own, and refuses to speak if a package appearing in
//! both resolved to different versions.
//!
//! Both files are read as text. A `Cargo.lock` is TOML, but the only two
//! fields this needs are `name` and `version` inside a `[[package]]` table,
//! and pulling in a TOML parser to read them would add a dependency that this
//! very check would then have to police. Cargo writes the file, not a human,
//! and it writes it in exactly this shape.

use std::collections::BTreeMap;
use std::path::Path;

/// What the two lock files agree and disagree about.
pub struct Lockstep {
    /// How many package names appear in both files.
    pub shared: usize,
    /// The ones whose versions differ: name, workspace version, ours.
    pub conflicts: Vec<(String, String, String)>,
}

impl Lockstep {
    /// True when every shared package resolved to the same version.
    pub fn agrees(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// Compare the workspace lock file with this crate's.
///
/// # Errors
///
/// When either file cannot be read. That is a hard error rather than a
/// skipped check: an unreadable lock file means the harness cannot show it is
/// verifying the right tree, and a check that silently degrades to no check is
/// worse than one that stops.
pub fn compare(workspace_lock: &Path, own_lock: &Path) -> Result<Lockstep, String> {
    let theirs = packages(workspace_lock)?;
    let ours = packages(own_lock)?;

    let mut shared = 0;
    let mut conflicts = Vec::new();
    for (name, their_version) in &theirs {
        let Some(our_version) = ours.get(name) else {
            continue;
        };
        shared += 1;
        if our_version != their_version {
            conflicts.push((name.clone(), their_version.clone(), our_version.clone()));
        }
    }
    Ok(Lockstep { shared, conflicts })
}

/// Every `name = version` pair in a `Cargo.lock`.
///
/// A package may legitimately appear twice at two versions — cargo permits it
/// for semver-incompatible duplicates. When that happens both versions are
/// kept in one string, so a comparison against it can never accidentally match
/// only the one that happened to be listed first.
fn packages(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            name = None;
        } else if let Some(v) = field(line, "name") {
            name = Some(v);
        } else if let Some(v) = field(line, "version")
            && let Some(n) = name.take()
        {
            out.entry(n)
                .and_modify(|existing| {
                    if existing != &v {
                        *existing = format!("{existing} and {v}");
                    }
                })
                .or_insert(v);
        }
    }
    if out.is_empty() {
        return Err(format!(
            "{} contains no [[package]] entries — is it a Cargo.lock?",
            path.display()
        ));
    }
    Ok(out)
}

/// `key = "value"` → `value`.
fn field(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
    let quoted = rest.strip_prefix('"')?.strip_suffix('"')?;
    Some(quoted.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_is_read_out_of_a_cargo_lock_line() {
        assert_eq!(field(r#"name = "glam""#, "name").as_deref(), Some("glam"));
        assert_eq!(
            field(r#"version = "0.33.3""#, "version").as_deref(),
            Some("0.33.3")
        );
        // `name` must not match `name_of_something_else`.
        assert_eq!(field(r#"namespace = "x""#, "name"), None);
        assert_eq!(field(r#"source = "registry+..""#, "name"), None);
    }
}

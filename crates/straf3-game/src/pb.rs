//! The personal best: where it lives, when it is replaced, and what is
//! refused.
//!
//! # What a personal best is here
//!
//! A `.s3d` file — the command stream of the run, bound to the collision
//! geometry and the physics it was set under. Not a time in a text file: a
//! time nobody can replay is a claim, and the whole point of the format is
//! that a claim can be re-run and either reproduces or does not.
//!
//! # Why the file is per map *and* per profile
//!
//! CPM and VQ3 are different games on the same course, and a CPM time is not a
//! VQ3 time. Keeping them in one file would mean the first run under the other
//! profile either overwrites a record it did not beat, or is refused as a
//! physics mismatch every time it is loaded. Separate files, and the name says
//! which is which.
//!
//! # The browser has no personal best yet
//!
//! Persistence here is the filesystem, and `wasm32-unknown-unknown` has none.
//! IndexedDB is the browser's answer and it is deferred to a later wave with
//! the rest of the web client (spec rev 2 decision D1); [`store`] and
//! [`fetch`] say so out loud on that target rather than failing quietly, so a
//! web build races no ghost and saves no time instead of pretending to.

use straf3_replay::{LoadError, Recording};

/// Where a personal best for this map and profile is kept, under `dir`.
///
/// A flat file name rather than nested directories: a run is identified by two
/// short names, and a directory per map is a lot of empty directories for
/// somebody who plays one course.
#[must_use]
pub fn path_in(dir: &str, map_name: &str, profile_name: &str) -> String {
    let map = sanitise(map_name);
    let profile = sanitise(profile_name);
    if dir.is_empty() {
        format!("{map}.{profile}.s3d")
    } else {
        format!("{dir}/{map}.{profile}.s3d")
    }
}

/// Where personal bests go when nobody said otherwise.
pub const DEFAULT_DIR: &str = "runs";

/// Reduce a name to something safe to put in a path.
///
/// A map name comes from a file the player chose, so it can contain anything a
/// path can — including `..`, which would write a personal best somewhere that
/// is nobody's business. Everything outside a small alphabet becomes `_`.
fn sanitise(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_owned()
    } else {
        cleaned
    }
}

/// Whether `candidate` should replace `current` as the personal best.
///
/// A run that never finished is never a personal best, however fast it looked;
/// and an equal time does not replace the file, because rewriting a record
/// with a copy of itself is churn, not an achievement.
#[must_use]
pub const fn beats(candidate: Option<u32>, current: Option<u32>) -> bool {
    match (candidate, current) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(new), Some(old)) => new < old,
    }
}

/// Why a saved personal best could not be read.
#[derive(Debug)]
pub enum PbError {
    /// The file could not be read at all.
    Io(std::io::Error),
    /// The bytes are not a `.s3d` this build understands.
    Format(LoadError),
    /// This target has no filesystem.
    Unsupported,
}

impl core::fmt::Display for PbError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Format(e) => write!(f, "{e}"),
            Self::Unsupported => write!(
                f,
                "this build has no filesystem, so it cannot keep a personal best"
            ),
        }
    }
}

impl std::error::Error for PbError {}

/// Read a saved personal best.
///
/// Decoding is all this does: whether the recording may be *raced* depends on
/// the world and the physics of the session about to load it, and that check
/// belongs at the point of use — see [`crate::ghost::Ghost::from_recording`].
///
/// # Errors
///
/// [`PbError::Io`] if the file cannot be read, [`PbError::Format`] if it is
/// not a `.s3d`.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch(path: &str) -> Result<Recording, PbError> {
    let bytes = std::fs::read(path).map_err(PbError::Io)?;
    Recording::from_bytes(&bytes).map_err(PbError::Format)
}

/// Write `recording` to `path`, creating the directory if it is not there.
///
/// # Errors
///
/// [`PbError::Io`] if the directory cannot be created or the file cannot be
/// written.
#[cfg(not(target_arch = "wasm32"))]
pub fn store(path: &str, recording: &Recording) -> Result<(), PbError> {
    if let Some(dir) = std::path::Path::new(path).parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(PbError::Io)?;
    }
    // Without the per-command checksum trace: the run digest already proves
    // the run, and a ghost file does not need a better *diagnosis* of a
    // disagreement at eight bytes a command.
    std::fs::write(path, recording.to_bytes()).map_err(PbError::Io)
}

/// The browser cannot read a personal best yet — see the module docs.
///
/// # Errors
///
/// Always [`PbError::Unsupported`].
#[cfg(target_arch = "wasm32")]
pub fn fetch(_path: &str) -> Result<Recording, PbError> {
    Err(PbError::Unsupported)
}

/// The browser cannot save a personal best yet — see the module docs.
///
/// # Errors
///
/// Always [`PbError::Unsupported`].
#[cfg(target_arch = "wasm32")]
pub fn store(_path: &str, _recording: &Recording) -> Result<(), PbError> {
    Err(PbError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_personal_best_is_named_for_its_map_and_its_physics() {
        assert_eq!(path_in("runs", "coil", "cpm"), "runs/coil.cpm.s3d");
        assert_eq!(path_in("", "coil", "vq3"), "coil.vq3.s3d");
    }

    #[test]
    fn a_map_name_cannot_write_the_personal_best_somewhere_else() {
        // The name comes from whatever file the player passed to `--map`.
        assert_eq!(
            path_in("runs", "../../etc/passwd", "cpm"),
            "runs/______etc_passwd.cpm.s3d"
        );
        assert_eq!(path_in("runs", "", "cpm"), "runs/unnamed.cpm.s3d");
    }

    #[test]
    fn only_a_finished_run_that_is_actually_faster_replaces_the_record() {
        assert!(beats(Some(4_000), None));
        assert!(beats(Some(3_999), Some(4_000)));
        assert!(!beats(Some(4_000), Some(4_000)));
        assert!(!beats(Some(4_001), Some(4_000)));
        assert!(!beats(None, None), "an unfinished run is not a time");
        assert!(!beats(None, Some(4_000)));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_personal_best_round_trips_through_a_file() {
        use straf3_replay::{RunStart, WorldId};
        use straf3_sim::num::{s, vec3};
        use straf3_sim::world::FlatGround;
        use straf3_sim::{PhysicsProfile, TickRate, UserCmd};

        let dir = std::env::temp_dir().join(format!("straf3-pb-{}", std::process::id()));
        let path = path_in(&dir.to_string_lossy(), "coil", "cpm");

        let recording = Recording::record(
            RunStart {
                rate: TickRate::HZ_125,
                spawn: vec3(s(0.0), s(0.0), s(24.0)),
                yaw: s(90.0),
            },
            vec![UserCmd::still_at(TickRate::HZ_125); 16],
            &FlatGround::at(s(0.0)),
            WorldId::flat(s(0.0)),
            &PhysicsProfile::cpm(),
            "cpm",
        );

        store(&path, &recording).expect("the directory is created if missing");
        let back = fetch(&path).expect("what was written reads back");
        assert_eq!(back.claimed(), recording.claimed());
        assert_eq!(back.command_count(), recording.command_count());
        assert_eq!(back.world(), recording.world());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

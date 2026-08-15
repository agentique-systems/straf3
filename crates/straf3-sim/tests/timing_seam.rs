//! Spec rev 3, acceptance criterion 3, made mechanical.
//!
//! > Timing never travels as float seconds. No float-seconds duration may cross
//! > the public API, be stored in `SimState`, or appear in a recording.
//! > Conversion from integer milliseconds to the working scalar must occur at a
//! > single auditable function, mirroring Q3's own
//! > `pml.frametime = pml.msec * 0.001`, and nowhere else. **A second
//! > conversion site is a defect.**
//!
//! A criterion phrased as "and nowhere else" is a claim about the whole crate,
//! and a reviewer re-checking it by hand every wave is a reviewer who will
//! eventually not. So it is checked here by reading the source.
//!
//! This lives in `tests/` because it reads files, and nothing under `src/` is
//! permitted to — the same rule that puts the headless runner in `bin/`.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![src_dir()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src/") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let name = path
                    .strip_prefix(src_dir())
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((name, std::fs::read_to_string(&path).expect("read source")));
            }
        }
    }
    assert!(out.len() >= 6, "found only {} sources", out.len());
    out
}

/// Lines that are code rather than documentation.
///
/// Doc comments name `seconds_from_millis` and quote Q3's `* 0.001` constantly
/// — that is the point of them — so counting raw occurrences would measure the
/// documentation. This strips comments so the check measures the code.
fn code_lines(source: &str) -> impl Iterator<Item = &str> {
    source.lines().map(str::trim).filter(|l| {
        !l.is_empty() && !l.starts_with("//") && !l.starts_with("/*") && !l.starts_with('*')
    })
}

#[test]
fn there_is_exactly_one_milliseconds_to_scalar_conversion_site() {
    let mut callers: Vec<(String, String)> = Vec::new();

    for (name, source) in rust_sources() {
        for line in code_lines(&source) {
            if !line.contains("seconds_from_millis") {
                continue;
            }
            // The definition itself, and the unit test that pins it to Q3's
            // expression, both live in `num.rs` and are not call sites in the
            // sense the criterion means.
            if name == "num.rs" {
                continue;
            }
            callers.push((name.clone(), line.to_string()));
        }
    }

    assert_eq!(
        callers.len(),
        1,
        "criterion 3: expected exactly one conversion site, found {}:\n{}",
        callers.len(),
        callers
            .iter()
            .map(|(f, l)| format!("  {f}: {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        callers[0].0, "step.rs",
        "the conversion moved out of step.rs, to {}",
        callers[0].0
    );
}

/// The other half of "and nowhere else": a second site could be written without
/// naming `seconds_from_millis` at all, by open-coding Q3's `msec * 0.001`.
///
/// The scan is for the *shape* of that defect — a millisecond-named value in
/// the same expression as a thousandth — rather than for the constant alone.
/// `0.001` and `1000.0` are ordinary numbers that appear as coordinates, hull
/// sizes and tolerances all over the crate; flagging every one of them would
/// produce a check that has to be suppressed, and a check that is routinely
/// suppressed protects nothing.
#[test]
fn no_module_open_codes_the_millisecond_conversion() {
    const SCALES: [&str; 4] = ["0.001", "1000.0", "1e-3", "1_000.0"];
    const DURATIONS: [&str; 5] = ["_ms", "ms ", "msec", "milli", "duration"];

    let mut offenders = Vec::new();
    for (name, source) in rust_sources() {
        if name == "num.rs" {
            continue; // the one permitted home for the conversion
        }
        for line in code_lines(&source) {
            let lower = line.to_ascii_lowercase();
            if SCALES.iter().any(|c| line.contains(c))
                && DURATIONS.iter().any(|d| lower.contains(d))
            {
                offenders.push(format!("  {name}: {line}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "criterion 3: a millisecond duration is being scaled outside num.rs:\n{}",
        offenders.join("\n")
    );

    // The scan has to be able to fail, or it is decorative. This is the line a
    // second conversion site would look like, and the matcher must catch it.
    let planted = "let dt = ms as f32 * 0.001;";
    let lower = planted.to_ascii_lowercase();
    assert!(
        SCALES.iter().any(|c| planted.contains(c))
            && DURATIONS.iter().any(|d| lower.contains(d)),
        "the scan would not notice a second conversion site"
    );
}

/// The rest of the criterion: durations stay integers everywhere they are
/// stored or handed across the API.
///
/// A type check rather than a text scan, so it fails at compile time if any of
/// these ever widens.
#[test]
fn every_stored_duration_is_an_integer() {
    use straf3_sim::{SimState, UserCmd};

    let cmd = UserCmd::still(8);
    let _: u16 = cmd.duration_ms;

    let st = SimState::default();
    let _: u32 = st.time_ms;
    let _: u16 = st.player.timers.movement_locked_ms;
    let _: u16 = st.player.timers.since_landed_ms;
    let _: u16 = st.player.timers.since_jumped_ms;
    let _: u16 = st.player.timers.double_jump_ms;

    // And the one profile field that is a duration is an integer too, so a
    // recorded profile cannot smuggle float seconds in either.
    let _: u16 = straf3_sim::PhysicsProfile::cpm().double_jump_window_ms;

    // Simulation time is the exact integer sum of the durations applied, with
    // no float anywhere in the path — 13 ms is the awkward one, since it is
    // what 76 Hz truncates to and it divides nothing evenly.
    let end = straf3_sim::run(
        &SimState::default(),
        &vec![UserCmd::still(13); 10_000],
        &straf3_sim::world::EmptyWorld,
        &straf3_sim::PhysicsProfile::cpm(),
    );
    assert_eq!(end.time_ms, 130_000);
}

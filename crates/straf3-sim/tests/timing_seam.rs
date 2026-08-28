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

fn workspace_root() -> PathBuf {
    // `crates/straf3-sim` -> the workspace root two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root above crates/straf3-sim")
        .to_path_buf()
}

/// Every Rust source the criterion applies to, named relative to the workspace
/// root.
///
/// Scope is the whole workspace, not just this crate's `src/`. The criterion
/// says a float-seconds duration may not "appear in a recording", and
/// `straf3-replay` is the crate that will own the recording format — so a scan
/// that could not see it was checking the easy half. `bin/` is included for the
/// same reason: the headless runner parses durations from a file, which is
/// exactly where a second conversion would be convenient to write.
fn rust_sources() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut roots: Vec<PathBuf> = Vec::new();

    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .expect("read crates/")
        .filter_map(|e| {
            let p = e.ok()?.path();
            p.is_dir().then_some(p)
        })
        .collect();
    crate_dirs.push(root.join("xtask"));
    crate_dirs.sort();

    for dir in crate_dirs {
        for sub in ["src", "bin"] {
            let d = dir.join(sub);
            if d.is_dir() {
                roots.push(d);
            }
        }
    }

    let mut out = Vec::new();
    let mut stack = roots;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read source dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let name = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((name, std::fs::read_to_string(&path).expect("read source")));
            }
        }
    }
    out.sort();

    // The scan is worthless if it silently finds nothing. Both bounds matter:
    // the count, and the presence of the specific files the criterion is about.
    assert!(out.len() >= 12, "found only {} sources", out.len());
    for required in [
        "crates/straf3-sim/src/step.rs",
        "crates/straf3-sim/src/num.rs",
        "crates/straf3-sim/bin/headless.rs",
        "crates/straf3-replay/src/lib.rs",
    ] {
        assert!(
            out.iter().any(|(n, _)| n == required),
            "{required} was not scanned; the scan's scope has regressed"
        );
    }
    out
}

/// The one file allowed to hold the conversion.
fn is_permitted_home(name: &str) -> bool {
    name == "crates/straf3-sim/src/num.rs"
}

/// Strip `//` and `/* */` comments from Rust source, tracking actual comment
/// state rather than guessing from a line's leading characters.
///
/// A leading-character heuristic (skip any line starting with `*`, on the
/// theory that it is a javadoc-style block-comment continuation) is a second
/// evasion in itself: this crate writes no `/* */` comments anywhere, so the
/// heuristic never protects anything, and it silently drops a real code line
/// that happens to start with a multiplication operator — exactly the shape
/// `* 0.001` takes when rustfmt wraps a long expression.
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut depth = 1u32;
                while depth > 0 {
                    match chars.next() {
                        Some('*') if chars.peek() == Some(&'/') => {
                            chars.next();
                            depth -= 1;
                        }
                        Some('/') if chars.peek() == Some(&'*') => {
                            chars.next();
                            depth += 1;
                        }
                        Some('\n') => out.push('\n'),
                        Some(_) => {}
                        None => break,
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Lines that are code rather than documentation, comments stripped.
///
/// Doc comments name `seconds_from_millis` and quote Q3's `* 0.001` constantly
/// — that is the point of them — so counting raw occurrences would measure the
/// documentation. This strips comments so the check measures the code.
fn code_lines(source: &str) -> Vec<String> {
    strip_comments(source)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
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
            if is_permitted_home(&name) {
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
        callers[0].0, "crates/straf3-sim/src/step.rs",
        "the conversion moved out of step.rs, to {}",
        callers[0].0
    );
}

/// The conversion factors this criterion cares about. Written once here and
/// reused by both the literal check and the alias check below, so the two
/// halves of the scan agree on what a "thousandth" looks like.
const SCALES: [&str; 4] = ["0.001", "1000.0", "1e-3", "1_000.0"];

/// A source file's code, with line breaks collapsed inside each `;`-terminated
/// statement.
///
/// `code_lines` strips comments but is still line-oriented, so an expression
/// split across lines — `s(f32::from(ms))\n    * THOUSANDTH;` — never puts the
/// duration and the scale in the same string the scanner looks at. Joining on
/// `;` is a rough model of "one statement", but it is the boundary the actual
/// defect (a value multiplied by a thousandth) has to cross, and Rust code
/// makes `;` cheap to rely on for this.
fn statements(source: &str) -> Vec<String> {
    let joined = code_lines(source).join(" ");
    joined
        .split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The name bound by an assignment-shaped statement — `const NAME: T = …`,
/// `let NAME = …` or `let mut NAME = …` — together with its right-hand side.
/// `None` for anything else, including comparisons (`==`, `!=`, `<=`, `>=`),
/// which share the byte `=` but are not bindings.
fn binding(stmt: &str) -> Option<(String, &str)> {
    let eq = stmt.rfind('=')?;
    if matches!(
        stmt.as_bytes().get(eq.wrapping_sub(1)),
        Some(b'=' | b'!' | b'<' | b'>')
    ) {
        return None;
    }
    let head = stmt[..eq].trim();
    let name = if let Some(rest) = head.strip_prefix("const ") {
        rest
    } else if let Some(rest) = head.strip_prefix("let mut ") {
        rest
    } else {
        head.strip_prefix("let ")?
    }
    .split(':')
    .next()
    .unwrap_or("")
    .trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_ascii_lowercase(), stmt[eq + 1..].trim()))
}

/// Whether a right-hand side is a *constant expression*: one built from
/// numeric literals, casts and other named constants, with no free operand.
///
/// This is the line between `s(1.0 / 1000.0)`, which is a name for the
/// thousandth written the long way, and `mhz as f64 / 1000.0`, which is a
/// *measured value that has been divided by* the thousandth. Only the first is
/// an alias for the constant. Reading the second as one is how a
/// millihertz-to-hertz conversion in a reporting xtask came to be reported as
/// a second site for Q3's `msec * 0.001`: `refresh_hz` was bound as if it were
/// the scale itself, and every later statement mentioning it inherited that.
///
/// Allowed: numeric literals (`1000`, `0f32`, and the `1e`/`3` halves that
/// [`tokens`] splits `1e-3` into), the scalar constructor and the types a cast
/// can name, and SCREAMING_CASE identifiers — Rust's spelling for a `const`,
/// so an alias folded out of *another* named constant stays tracked.
fn is_constant_expression(rhs: &str) -> bool {
    tokens(rhs).all(|t| {
        t.starts_with(|c: char| c.is_ascii_digit())
            || matches!(t, "s" | "as" | "f32" | "f64" | "Scalar")
            || t.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    })
}

/// Identifiers whose right-hand side *is* one of [`SCALES`] — `const
/// MS_TO_SEC: Scalar = s(0.001);` or `const INV_THOUSAND: Scalar = s(1.0 /
/// 1000.0);` — so the scan can treat a later `* MS_TO_SEC` the same as it
/// would treat `* 0.001` written out. The RHS does not have to be one bare
/// literal, because a reciprocal or any other constant expression over the
/// same literal is still the same constant in disguise; but it does have to be
/// constant, which is what [`is_constant_expression`] decides.
fn scale_aliases(statements: &[String]) -> Vec<String> {
    statements
        .iter()
        .filter_map(|stmt| binding(stmt))
        .filter(|(_, rhs)| SCALES.iter().any(|c| rhs.contains(c)) && is_constant_expression(rhs))
        .map(|(name, _)| name)
        .collect()
}

/// Whether a token names a millisecond-flavoured value: `ms` itself,
/// `duration_ms`, `ms_to_sec`, `msec`, `milliseconds`, `duration`, and so on.
/// Word-bounded (via [`tokens`]) so it does not fire on `params` or `comment`,
/// which merely contain the letters `ms`.
fn is_duration_token(token: &str) -> bool {
    token == "ms"
        || token.ends_with("_ms")
        || token.starts_with("ms_")
        || token.contains("msec")
        || token.contains("milli")
        || token.contains("duration")
}

/// Identifier-ish tokens in a lowercased statement, splitting on anything that
/// cannot appear inside a Rust identifier.
fn tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
}

/// Identifiers whose right-hand side already names a duration — directly
/// (`let raw = f32::from(ms);`) or transitively through an earlier alias
/// (`let alt = s(raw * 0.001);`, once `raw` is known) — so a value can be
/// renamed away from its `ms`-bearing source without the scan losing track of
/// it. Statements are walked in file order, which is also binding order:
/// Rust cannot use a local before it is declared, so one pass is enough to
/// carry an alias forward through a chain of renames.
fn duration_aliases(statements: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for stmt in statements {
        let Some((name, rhs)) = binding(stmt) else {
            continue;
        };
        let lower = rhs.to_ascii_lowercase();
        if tokens(&lower).any(|t| is_duration_token(t) || out.iter().any(|a| a == t)) {
            out.push(name);
        }
    }
    out
}

/// The matcher itself: the statements in one source that carry both a scale
/// and a duration.
///
/// Written once and used by both the scan over the workspace and the
/// self-tests below, so the thing the self-tests certify cannot drift away
/// from the thing that actually guards the tree.
fn offending_statements(source: &str) -> Vec<String> {
    let stmts = statements(source);
    let scale_al = scale_aliases(&stmts);
    let duration_al = duration_aliases(&stmts);
    stmts
        .iter()
        .filter(|stmt| {
            let lower = stmt.to_ascii_lowercase();
            let has_scale = SCALES.iter().any(|c| stmt.contains(c))
                || tokens(&lower).any(|t| scale_al.iter().any(|a| a == t));
            let has_duration =
                tokens(&lower).any(|t| is_duration_token(t) || duration_al.iter().any(|a| a == t));
            has_scale && has_duration
        })
        .cloned()
        .collect()
}

/// The other half of "and nowhere else": a second site could be written without
/// naming `seconds_from_millis` at all, by open-coding Q3's `msec * 0.001` —
/// directly, through a line split, or through a renamed alias of the constant.
///
/// The scan is for the *shape* of that defect — a millisecond-named value
/// multiplied by a thousandth, in the same statement — rather than for the
/// literal alone. `0.001` and `1000.0` are ordinary numbers that appear as
/// coordinates, hull sizes and tolerances all over the crate; flagging every
/// one of them would produce a check that has to be suppressed, and a check
/// that is routinely suppressed protects nothing.
#[test]
fn no_module_open_codes_the_millisecond_conversion() {
    let mut offenders = Vec::new();
    for (name, source) in rust_sources() {
        if is_permitted_home(&name) {
            continue; // the one permitted home for the conversion
        }
        offenders.extend(
            offending_statements(&source)
                .into_iter()
                .map(|stmt| format!("  {name}: {stmt}")),
        );
    }
    assert!(
        offenders.is_empty(),
        "criterion 3: a millisecond duration is being scaled outside num.rs:\n{}",
        offenders.join("\n")
    );

    // The scan has to be able to fail, or it is decorative. These are the
    // shapes a second conversion site could take — direct, line-split,
    // aliased by a bare-equal constant, aliased by an expression that only
    // *contains* the scale (a folded reciprocal), aliased by renaming the
    // duration itself through an intermediate local, aliased one step further
    // out through another named constant, and written in the same statement as
    // the prose that describes it — and the matcher must catch all of them.
    //
    // The last two are the ones that pin the alias rule's tightening
    // ([`is_constant_expression`]). A `const` folded out of another `const` is
    // still a constant and must stay tracked; and a real conversion sitting
    // beside a string literal must stay caught, because the tempting way to
    // silence a false positive in a *reporting* module is to exempt formatting
    // statements or strip string literals wholesale, and that would open a
    // hole a conversion could be written through.
    for planted in [
        "let dt = ms as f32 * 0.001;",
        "let dt = ms as f32\n    * 0.001;",
        "const MS_TO_SEC: Scalar = s(0.001);\nlet dt = s(f32::from(ms)) * MS_TO_SEC;",
        "const INV_THOUSAND: Scalar = s(1.0 / 1000.0);\nlet dt = s(f32::from(ms)) * INV_THOUSAND;",
        "let raw = f32::from(ms);\nlet dt = s(raw * 0.001);",
        "let raw = f32::from(ms);\nlet hop = raw;\nlet dt = s(hop * 0.001);",
        "const ONE_UNIT: Scalar = s(1.0);\nconst TO_SECONDS: Scalar = ONE_UNIT * 0.001;\n\
         let dt = s(f32::from(ms)) * TO_SECONDS;",
        "let _ = writeln!(out, \"a frame takes {} ms to scan out\", ms as f32 * 0.001);",
    ] {
        assert!(
            !offending_statements(planted).is_empty(),
            "the scan would not notice: {planted}"
        );
    }

    // And the shape that must NOT be reported, pinned so a later widening
    // cannot quietly reintroduce it. `refresh_hz` is millihertz divided down
    // to hertz — a frequency read back from the monitor handle, not a
    // float-seconds duration — and `ms(..)` here is a formatting helper. Two
    // real code tokens, neither of them a conversion. Criterion 3 is about
    // durations crossing the API, and nothing here is one.
    for benign in [
        "let refresh_hz = measured_refresh_mhz(log).map(|mhz| mhz as f64 / 1000.0);\n\
         let _ = writeln!(out, \"a whole frame takes {} ms to scan out\", ms(interval));",
        "let per_second = samples as f64 / 1000.0;\nlet _ = writeln!(out, \"{per_second} k/ms\");",
    ] {
        assert!(
            offending_statements(benign).is_empty(),
            "the scan reports a false positive on: {benign}"
        );
    }
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

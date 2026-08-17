//! Argument parsing, kept away from the Win32 code so it is unit-tested on
//! Linux by `cargo test --workspace` even though the capture itself is not.

use crate::BlankPolicy;
use std::path::PathBuf;

pub const USAGE: &str = "\
straf3-capture — screenshot the running straf3 window, and prove it is not blank

usage: straf3-capture --out <file.png> [options]
       straf3-capture --list
       straf3-capture --desktop --out target/<file.png>      (diagnostic only)

  --out <file.png>       where to write the capture. Required unless --list.
  --title <substring>    match a top-level window whose title contains this,
                         case-insensitively (default: straf3)
  --wait-ms <ms>         keep looking for the window for up to this long
                         before giving up (default: 0, look once)
  --settle-ms <ms>       wait this long after finding the window, so the
                         client has presented a frame (default: 300)
  --no-raise             do not try to bring the window to the front first
  --min-visible <n>      refuse to capture unless at least n/1000 of the
                         window's area hit-tests as that window and not as
                         something covering it (default: 900, i.e. 90 %)
  --desktop              capture the whole virtual desktop. Diagnostic only —
                         see below. --out must be under target/.
  --list                 list visible top-level windows and exit
  --min-colours <n>      fewer distinct colours than this means blank
                         (default: 16)
  --max-dominant <n>     one colour covering more than n/1000 of the image
                         means blank (default: 995, i.e. 99.5 %)
  -h, --help             this text

THIS TOOL CAPTURES A WINDOW, NEVER THE SCREEN. There is no fallback: if the
straf3 window is not found, or is covered by something else, the tool fails
and writes nothing. Both refusals are deliberate. The capture is read from the
desktop device context — a GPU window's own DC comes back black — so it reads
whatever is on screen over the window's rectangle. Without those checks, a
missing or occluded window would produce a perfectly valid PNG of somebody's
browser, and that image would then be committed as evidence about straf3.

--desktop exists only to answer \"was the window even up?\". It grabs the whole
virtual screen, including everything else on it, so it is restricted to paths
under target/ — which .gitignore excludes — and cannot be written anywhere a
commit could pick it up.

Exit codes:
  0  captured, written, and not blank
  1  bad arguments, or the capture failed outright
  2  not running on Windows
  3  the capture was written but is blank (see the reason on stderr)
  4  no window matched; nothing was written
  5  the window is covered by something else; nothing was written
";

/// Where the pixels should come from.
///
/// There is deliberately no "window, or the desktop if that fails" variant.
/// See [`USAGE`]: a silent desktop fallback turns a missing window into a
/// screenshot of whatever the operator had open, and that image then travels
/// as evidence about straf3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A top-level window whose title contains this substring. If no such
    /// window is on screen, the capture fails; it does not become something
    /// else.
    Window {
        title: String,
        /// Try to bring it to the front before capturing.
        raise: bool,
        /// Minimum fraction of the window's area, in tenths of a percent,
        /// that must hit-test as the window itself. Below this the capture
        /// would be a picture of whatever is covering it, so it is refused.
        min_visible_permille: u32,
    },
    /// The whole virtual desktop, asked for explicitly with `--desktop`, for
    /// diagnosing why a window was not found. Restricted by [`parse`] to
    /// paths under `target/`.
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub out: PathBuf,
    pub source: Source,
    pub wait_ms: u64,
    pub settle_ms: u64,
    pub policy: BlankPolicy,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Print usage and exit 0.
    Help,
    /// List windows and exit 0.
    List,
    /// Take a capture.
    Capture(Box<Options>),
}

/// Parse `argv` (without the program name).
///
/// # Errors
///
/// Returns a message suitable for stderr when the arguments do not make sense.
pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Request, String> {
    let mut out: Option<PathBuf> = None;
    let mut title = "straf3".to_owned();
    let mut wait_ms = 0u64;
    let mut settle_ms = 300u64;
    let mut desktop = false;
    let mut raise = true;
    let mut min_visible_permille = 900u32;
    let mut list = false;
    let mut policy = BlankPolicy::default();

    let mut args = argv.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("`{arg}` needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(Request::Help),
            "--list" => list = true,
            "--desktop" => desktop = true,
            "--no-raise" => raise = false,
            "--min-visible" => {
                let n = number(&value()?, "--min-visible")?;
                if n > 1000 {
                    return Err("`--min-visible` is in tenths of a percent, so at most 1000".into());
                }
                min_visible_permille =
                    u32::try_from(n).map_err(|_| "`--min-visible` is out of range".to_owned())?;
            }
            "--out" => out = Some(PathBuf::from(value()?)),
            "--title" => title = value()?,
            "--wait-ms" => wait_ms = number(&value()?, "--wait-ms")?,
            "--settle-ms" => settle_ms = number(&value()?, "--settle-ms")?,
            "--min-colours" | "--min-colors" => {
                policy.min_colours = usize::try_from(number(&value()?, "--min-colours")?)
                    .map_err(|_| "`--min-colours` is out of range".to_owned())?;
            }
            "--max-dominant" => {
                let n = number(&value()?, "--max-dominant")?;
                if n > 1000 {
                    return Err(
                        "`--max-dominant` is in tenths of a percent, so at most 1000".into(),
                    );
                }
                policy.max_dominant_permille =
                    u32::try_from(n).map_err(|_| "`--max-dominant` is out of range".to_owned())?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if list {
        return Ok(Request::List);
    }
    let Some(out) = out else {
        return Err("`--out <file.png>` is required".into());
    };
    if out
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("png"))
    {
        // Not fatal in principle, but a .jpg that is secretly a PNG is the
        // kind of small lie that costs someone an afternoon.
        return Err(format!(
            "`--out {}` should end in .png — that is the only format written",
            out.display()
        ));
    }
    if desktop && !under_target(&out) {
        // The structural half of the window-only rule. A comment asking a
        // contributor not to commit a screen grab is weaker than a path that
        // refuses to hold one: `target/` is in .gitignore, so a diagnostic
        // capture written there cannot be committed by accident.
        return Err(format!(
            "`--desktop --out {}` is refused: a whole-screen capture contains \
             whatever else is on the operator's desktop, so it may only be \
             written under `target/`, which .gitignore excludes.\n\
             Use `--out target/{}` — or drop `--desktop` and capture the \
             window, which is what this tool is for.",
            out.display(),
            out.file_name()
                .map_or_else(|| "desktop.png".into(), |n| n.to_string_lossy())
        ));
    }

    Ok(Request::Capture(Box::new(Options {
        out,
        source: if desktop {
            Source::Desktop
        } else {
            Source::Window {
                title,
                raise,
                min_visible_permille,
            }
        },
        wait_ms,
        settle_ms,
        policy,
    })))
}

/// Whether `path` has a `target` directory component.
///
/// Component-wise rather than a string prefix, so `target/shots/x.png`,
/// `./target/x.png` and an absolute path into a build directory all pass,
/// while `targeted.png` and `docs/target-audience/x.png` do not.
fn under_target(path: &std::path::Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("target"))
}

fn number(raw: &str, flag: &str) -> Result<u64, String> {
    raw.parse::<u64>()
        .map_err(|_| format!("`{flag} {raw}` is not a number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Request, String> {
        parse(args.iter().map(|s| (*s).to_owned()))
    }

    fn options(args: &[&str]) -> Options {
        match parse_args(args).unwrap() {
            Request::Capture(o) => *o,
            other => panic!("expected a capture, got {other:?}"),
        }
    }

    #[test]
    fn the_default_target_is_the_straf3_window() {
        let o = options(&["--out", "shot.png"]);
        assert_eq!(
            o.source,
            Source::Window {
                title: "straf3".to_owned(),
                raise: true,
                min_visible_permille: 900,
            }
        );
        assert_eq!(o.settle_ms, 300);
        assert_eq!(o.wait_ms, 0);
    }

    #[test]
    fn a_whole_screen_capture_can_only_be_written_where_git_ignores_it() {
        // The rule, enforced rather than requested. A full-screen grab of this
        // project's host contains the operator's browser, accounts and
        // whatever is playing; one such image is already in this repository's
        // history, which is why the restriction is structural.
        assert!(parse_args(&["--desktop", "--out", "shot.png"]).is_err());
        assert!(parse_args(&["--desktop", "--out", "docs/evidence/shot.png"]).is_err());
        assert!(parse_args(&["--desktop", "--out", "target/diagnose.png"]).is_ok());
        assert!(parse_args(&["--desktop", "--out", "./target/shots/diagnose.png"]).is_ok());
        // A directory that merely starts with the same letters is not target/.
        assert!(parse_args(&["--desktop", "--out", "targeted/shot.png"]).is_err());
        // The restriction is only on whole-screen captures. A window capture
        // is the tool's normal output and belongs wherever the caller says.
        assert!(parse_args(&["--out", "docs/evidence/shot.png"]).is_ok());
    }

    #[test]
    fn there_is_no_way_to_ask_for_a_desktop_fallback() {
        // If such a flag is ever added back, this test is the thing that
        // notices. A missing window must fail, not silently become a
        // screenshot of something else.
        assert!(parse_args(&["--out", "s.png", "--require-window"]).is_err());
        assert!(parse_args(&["--out", "s.png", "--fallback-desktop"]).is_err());
        match options(&["--out", "s.png"]).source {
            Source::Window { .. } => {}
            other => panic!("the default source must be a window, got {other:?}"),
        }
    }

    #[test]
    fn the_occlusion_check_is_on_by_default_and_can_be_tightened() {
        // It defaults on because the first real capture this tool took was a
        // picture of a browser sitting in front of the game: non-blank, valid,
        // and completely wrong. Off-by-default would put that back.
        let o = options(&["--out", "s.png", "--min-visible", "1000", "--no-raise"]);
        match o.source {
            Source::Window {
                raise,
                min_visible_permille,
                ..
            } => {
                assert!(!raise);
                assert_eq!(min_visible_permille, 1000);
            }
            other => panic!("expected a window source, got {other:?}"),
        }
        assert!(parse_args(&["--out", "s.png", "--min-visible", "1001"]).is_err());
    }

    #[test]
    fn out_is_required_and_must_be_a_png() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["--out", "shot.bmp"]).is_err());
        assert!(parse_args(&["--out", "shot"]).is_err());
        assert!(parse_args(&["--out", "shot.PNG"]).is_ok());
    }

    #[test]
    fn list_and_help_need_no_out() {
        assert_eq!(parse_args(&["--list"]).unwrap(), Request::List);
        assert_eq!(parse_args(&["--help"]).unwrap(), Request::Help);
        assert_eq!(parse_args(&["-h"]).unwrap(), Request::Help);
    }

    #[test]
    fn thresholds_come_through_and_are_range_checked() {
        let o = options(&[
            "--out",
            "s.png",
            "--min-colours",
            "64",
            "--max-dominant",
            "500",
        ]);
        assert_eq!(o.policy.min_colours, 64);
        assert_eq!(o.policy.max_dominant_permille, 500);
        assert!(parse_args(&["--out", "s.png", "--max-dominant", "1001"]).is_err());
        assert!(parse_args(&["--out", "s.png", "--wait-ms", "soon"]).is_err());
    }

    #[test]
    fn a_missing_value_is_an_error_not_a_silent_default() {
        assert!(parse_args(&["--out"]).is_err());
        assert!(parse_args(&["--out", "s.png", "--title"]).is_err());
    }

    #[test]
    fn unknown_arguments_are_refused() {
        assert!(parse_args(&["--out", "s.png", "--fullscreen"]).is_err());
    }
}

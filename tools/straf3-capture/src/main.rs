//! `straf3-capture` — screenshot the running straf3 window on Windows.
//!
//! See the crate docs in `lib.rs` for why it reads the desktop rather than the
//! window, and why it verifies what it captured.

use std::process::ExitCode;

use straf3_capture::cli::{self, Request};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let request = match cli::parse(argv) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("straf3-capture: {e}\n");
            eprint!("{}", cli::USAGE);
            return ExitCode::FAILURE;
        }
    };
    if matches!(request, Request::Help) {
        print!("{}", cli::USAGE);
        return ExitCode::SUCCESS;
    }
    run(request)
}

#[cfg(not(windows))]
fn run(_request: Request) -> ExitCode {
    // Compiled on Linux so `cargo test --workspace` and `cargo clippy
    // --workspace` cover the argument parsing and the verification, but the
    // capture itself only means something on the host with the real display.
    // Exiting 2 rather than 1 lets a script tell "wrong platform" apart from
    // "the capture failed".
    eprintln!(
        "straf3-capture: this is a Windows tool and this is not Windows.\n\
         \n\
         Build it for the host and run it through WSL interop:\n\
         \n\
         \x20   cargo build --release --target x86_64-pc-windows-gnu -p straf3-capture\n\
         \x20   ./target/x86_64-pc-windows-gnu/release/straf3-capture.exe --out shot.png\n\
         \n\
         A capture taken on the WSL2 side would be of WSLg's software-rendered\n\
         output, which is not the client this project is measured on."
    );
    ExitCode::from(2)
}

#[cfg(windows)]
fn run(request: Request) -> ExitCode {
    use std::time::{Duration, Instant};
    use straf3_capture::win;

    win::become_dpi_aware();

    let options = match request {
        Request::List => {
            let windows = win::list_windows();
            println!("{} visible top-level window(s):", windows.len());
            for w in windows {
                println!(
                    "  hwnd 0x{:08x}  {:<24}  {:<48}  {}{}",
                    w.hwnd,
                    // The process is what --process matches on, so it is shown
                    // beside the title rather than left to be guessed at.
                    w.process.as_deref().unwrap_or("<would not say>"),
                    truncate(&w.title, 48),
                    w.rect,
                    if w.from_dwm { "" } else { "  (GetWindowRect)" }
                );
            }
            return ExitCode::SUCCESS;
        }
        Request::Help => unreachable!("handled in main"),
        Request::Capture(o) => *o,
    };

    // Find the target rectangle.
    let (rect, description, already_settled) = match &options.source {
        cli::Source::Desktop => {
            println!(
                "capture: WHOLE-SCREEN DIAGNOSTIC. This image contains everything on \
                 the desktop, not just straf3. The argument parser has already \
                 confirmed the destination is under target/, which .gitignore \
                 excludes, so it cannot be committed — do not move it out."
            );
            (
                win::virtual_screen(),
                "the whole virtual screen (asked for with --desktop)".to_owned(),
                false,
            )
        }
        cli::Source::Window {
            title,
            process,
            raise,
            min_visible_permille,
        } => {
            let deadline = Instant::now() + Duration::from_millis(options.wait_ms);
            let hits = loop {
                let hits = win::find_windows(title, process.as_deref());
                if !hits.is_empty() || Instant::now() >= deadline {
                    break hits;
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            match hits.split_first() {
                Some((best, rest)) => {
                    if !rest.is_empty() {
                        println!(
                            "capture: {} windows match {title:?}; taking the largest. \
                             Others: {}",
                            rest.len() + 1,
                            rest.iter()
                                .map(|w| format!("{:?} {}", truncate(&w.title, 32), w.rect))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }

                    // Held for the rest of the capture; dropping it restores
                    // the window's z-order so the operator is not left with
                    // straf3 pinned over everything they were doing.
                    let _raised = raise.then(|| win::raise(best.hwnd));
                    // Settle before the hit test, not after: raising the
                    // window and testing in the same instant tests the old
                    // z-order, and the answer would be wrong in the direction
                    // that lets a bad capture through.
                    if options.settle_ms > 0 {
                        std::thread::sleep(Duration::from_millis(options.settle_ms));
                    }

                    // Re-read the rectangle: raising a window can move it, and
                    // a restored-from-minimised window's rect is entirely
                    // different from the parked one.
                    let rect = win::find_windows(title, process.as_deref())
                        .into_iter()
                        .find(|w| w.hwnd == best.hwnd)
                        .map_or(best.rect, |w| w.rect);

                    let visible = win::occlusion(best.hwnd, rect, win::SAMPLE_STEPS);
                    println!(
                        "capture: {} of {} hit-test points inside the window are the window \
                         ({}.{} %)",
                        visible.ours,
                        visible.sampled,
                        visible.visible_permille() / 10,
                        visible.visible_permille() % 10,
                    );
                    if visible.visible_permille() < *min_visible_permille {
                        let blocker = visible
                            .blocker
                            .map(|h| format!("{:?} (hwnd 0x{h:08x})", win::title_of(h)))
                            .unwrap_or_else(|| "something with no title".to_owned());
                        eprintln!(
                            "straf3-capture: {:?} is covered by {blocker} — only {}.{} % of it \
                             is on top, and the minimum is {}.{} %.\n\
                             Nothing was written. A capture here would be a picture of the \
                             covering window, not of straf3: this tool reads the desktop, \
                             because a GPU window's own device context comes back black.\n\
                             Bring straf3 to the front, or pass --min-visible 0 if you really \
                             want whatever is on screen there.",
                            best.title,
                            visible.visible_permille() / 10,
                            visible.visible_permille() % 10,
                            min_visible_permille / 10,
                            min_visible_permille % 10,
                        );
                        return ExitCode::from(5);
                    }

                    (
                        // Stop one pixel short of the reported edge: that row
                        // belongs to whatever is behind the window, not to the
                        // window. See win::EDGE_BLEED_PX.
                        rect.inset(win::EDGE_BLEED_PX),
                        format!(
                            "window {:?} of {} (hwnd 0x{:08x}, rect from {})",
                            best.title,
                            best.process.as_deref().unwrap_or("<would not say>"),
                            best.hwnd,
                            if best.from_dwm {
                                "DWM extended frame bounds"
                            } else {
                                "GetWindowRect"
                            }
                        ),
                        // Already settled above, before the hit test.
                        true,
                    )
                }
                // No fallback, on purpose. Capturing the desktop here would
                // produce a valid, non-blank PNG of whatever the operator had
                // open, and it would then be filed as evidence about straf3.
                // Failing is the useful answer; --desktop is how someone asks
                // for the diagnostic deliberately, and it can only be written
                // where git will not take it.
                None => {
                    let waited = if options.wait_ms > 0 {
                        format!(" after waiting {} ms", options.wait_ms)
                    } else {
                        String::new()
                    };
                    eprintln!(
                        "straf3-capture: no visible on-screen window{}{waited}.\n\
                         Nothing was written — this tool captures a window, never the \
                         screen.",
                        match &process {
                            Some(exe) =>
                                format!(" whose title contains {title:?} and which belongs to {exe}"),
                            None => format!(" whose title contains {title:?}"),
                        }
                    );

                    // The near-miss is the whole diagnostic. Saying only "not
                    // found" when three windows matched the title and were
                    // refused for their process reads as a broken tool, and
                    // the operator's next move is to reach for a system
                    // screenshot key — the one outcome this tool exists to
                    // prevent. Name them instead.
                    if let Some(exe) = &process {
                        let by_title: Vec<_> = win::find_windows(title, None)
                            .into_iter()
                            .filter(|w| w.process.as_deref() != Some(exe.as_str()))
                            .collect();
                        if !by_title.is_empty() {
                            eprintln!(
                                "\n{} window(s) DID match the title and were refused because \
                                 they are not {exe}: {}.\n\
                                 A title is not an identity — an editor with a straf3 file \
                                 open matches {title:?} too, and capturing it would have \
                                 produced a valid, non-blank picture of somebody's document \
                                 filed as evidence about straf3. If one of these really is \
                                 the client, pass --process <exe> or --any-process.",
                                by_title.len(),
                                by_title
                                    .iter()
                                    .map(|w| format!(
                                        "{:?} ({})",
                                        truncate(&w.title, 32),
                                        w.process.as_deref().unwrap_or("<would not say>")
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                    }

                    eprintln!(
                        "\n`straf3-capture --list` shows what is open, with the process \
                         behind each window. If you need to see why the window is missing, \
                         `straf3-capture --desktop --out target/diagnose.png` grabs the \
                         whole screen, but only under target/ because it captures \
                         everything else too."
                    );
                    return ExitCode::from(4);
                }
            }
        }
    };

    if options.settle_ms > 0 && !already_settled {
        std::thread::sleep(Duration::from_millis(options.settle_ms));
    }

    println!("capture: source is {description}");
    println!(
        "capture: rect {rect} on virtual screen {}",
        win::virtual_screen()
    );

    let (image, actual) = match win::capture_rect(rect) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("straf3-capture: {e}");
            return ExitCode::FAILURE;
        }
    };
    if actual != rect {
        println!("capture: rect was clipped to the virtual screen: {actual}");
    }

    // The image is written before it is judged, on purpose: when a capture
    // comes back blank, the blank image is the evidence of what went wrong,
    // and deleting it would leave nothing to look at.
    let bytes = match image.write_png(&options.out) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("straf3-capture: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "capture: wrote {} — {}x{} rgb8, {} bytes",
        options.out.display(),
        image.width(),
        image.height(),
        bytes
    );

    let verdict = image.verify();
    println!("verify: {}", verdict.describe());
    if let Some(reason) = verdict.blank_reason(options.policy) {
        eprintln!(
            "straf3-capture: THIS CAPTURE IS BLANK — {reason}.\n\
             The PNG was still written so it can be looked at, but it is not \
             evidence that anything was on screen.\n\
             Usual causes: the window is minimised or fully occluded, the \
             desktop is locked, or the region is off-screen."
        );
        return ExitCode::from(3);
    }
    println!("verify: not blank — this is a picture of something.");
    ExitCode::SUCCESS
}

#[cfg(windows)]
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

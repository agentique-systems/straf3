//! Choosing a present mode explicitly, instead of taking whatever the adapter
//! happened to list first.
//!
//! # Why this is not a detail
//!
//! The surface used to be configured with `caps.present_modes[0]`. That is not
//! a choice, it is an accident of driver enumeration — and it is the single
//! knob that decides whether the frame loop is pinned to the display's refresh
//! or runs free. Acceptance criterion 7 asks for measured frame times *both*
//! vsynced and uncapped, so the mode has to be selectable, and a published
//! number has to say which mode produced it.
//!
//! Worse than being unselectable, it was unreported: a suspiciously flat
//! 165 fps was observed and could not be attributed, because nothing recorded
//! whether the surface was in FIFO. That is the specific hole this closes.
//!
//! # Why an environment variable and not a flag
//!
//! The flag parser lives in `straf3-game`, which another seat owns this wave.
//! `STRAF3_PRESENT_MODE` is entirely inside this crate, so the two changes do
//! not touch the same file. It is read once, at surface configuration.
//!
//! # What it does not do
//!
//! It does not silently substitute. If the requested mode is unsupported the
//! fallback is logged as a fallback, and [`Selection::actual`] is what was
//! configured, not what was asked for. A measurement that reports the mode it
//! requested rather than the mode it got is a measurement of nothing.

/// The environment variable that selects the present mode.
pub const ENV_PRESENT_MODE: &str = "STRAF3_PRESENT_MODE";

/// The environment variable that overrides `desired_maximum_frame_latency`.
pub const ENV_FRAME_LATENCY: &str = "STRAF3_FRAME_LATENCY";

/// The frame latency used when [`ENV_FRAME_LATENCY`] is unset.
///
/// Two is wgpu's own conventional default and what this renderer used before
/// the variable existed; keeping it means turning the knob is a deliberate act
/// and the default behaviour is unchanged.
pub const DEFAULT_FRAME_LATENCY: u32 = 2;

/// What the caller asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// No preference expressed; take the adapter's first mode, which is what
    /// the renderer did before this module existed.
    Adapter,
    /// A specific mode, which may or may not be supported.
    Exact(wgpu::PresentMode),
}

/// Parse a present mode name.
///
/// Accepts the wgpu names and the two words a person actually says — `vsync`
/// for FIFO and `uncapped` for Immediate — because those are the terms the
/// measurement is published in.
#[must_use]
pub fn parse(raw: &str) -> Option<Request> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "auto" | "adapter" => Some(Request::Adapter),
        "fifo" | "vsync" => Some(Request::Exact(wgpu::PresentMode::Fifo)),
        "fiforelaxed" | "fifo_relaxed" | "fifo-relaxed" => {
            Some(Request::Exact(wgpu::PresentMode::FifoRelaxed))
        }
        "immediate" | "uncapped" | "novsync" | "no-vsync" => {
            Some(Request::Exact(wgpu::PresentMode::Immediate))
        }
        "mailbox" => Some(Request::Exact(wgpu::PresentMode::Mailbox)),
        _ => None,
    }
}

/// The short name this project publishes a mode under. Stable — the pacing
/// log's header and the measurement document both use these words.
#[must_use]
pub fn name(mode: wgpu::PresentMode) -> &'static str {
    match mode {
        wgpu::PresentMode::Fifo => "fifo",
        wgpu::PresentMode::FifoRelaxed => "fifo_relaxed",
        wgpu::PresentMode::Immediate => "immediate",
        wgpu::PresentMode::Mailbox => "mailbox",
        wgpu::PresentMode::AutoVsync => "auto_vsync",
        wgpu::PresentMode::AutoNoVsync => "auto_no_vsync",
    }
}

/// What was requested, what was configured, and whether those differ.
#[derive(Debug, Clone)]
pub struct Selection {
    /// What the environment asked for.
    pub requested: Request,
    /// What the surface was actually configured with. **This** is the number
    /// a measurement may quote.
    pub actual: wgpu::PresentMode,
    /// Everything the surface offered, for the log.
    pub available: Vec<wgpu::PresentMode>,
    /// Set when the requested mode was not available and something else was
    /// used instead.
    pub fell_back: bool,
    /// `desired_maximum_frame_latency` as configured.
    pub frame_latency: u32,
}

impl Selection {
    /// One line naming the mode actually in force, its provenance, and the
    /// alternatives — enough for a reader to check a published number against.
    #[must_use]
    pub fn describe(&self) -> String {
        let available = self
            .available
            .iter()
            .map(|m| name(*m))
            .collect::<Vec<_>>()
            .join(",");
        let asked = match self.requested {
            Request::Adapter => "adapter's first".to_owned(),
            Request::Exact(m) => name(m).to_owned(),
        };
        if self.fell_back {
            format!(
                "present_mode={} (FELL BACK — {asked} is not supported here) \
                 frame_latency={} available=[{available}]",
                name(self.actual),
                self.frame_latency
            )
        } else {
            format!(
                "present_mode={} (requested {asked}) frame_latency={} available=[{available}]",
                name(self.actual),
                self.frame_latency
            )
        }
    }
}

/// Decide the present mode for a surface with these capabilities.
///
/// `available` is `SurfaceCapabilities::present_modes`. It is never empty in
/// practice — every backend supports at least FIFO — but an empty slice
/// resolves to FIFO rather than panicking, because a renderer that aborts on
/// a surprising capability list is worse than one that vsyncs.
#[must_use]
pub fn choose(
    available: &[wgpu::PresentMode],
    requested: Request,
    frame_latency: u32,
) -> Selection {
    let first = available
        .first()
        .copied()
        .unwrap_or(wgpu::PresentMode::Fifo);
    let (actual, fell_back) = match requested {
        Request::Adapter => (first, false),
        Request::Exact(want) if available.contains(&want) => (want, false),
        // FIFO is the fallback rather than `first`: it is the mode the spec
        // guarantees, and falling back to something that is itself an accident
        // of enumeration would make the failure harder to reason about.
        Request::Exact(_) => (
            if available.contains(&wgpu::PresentMode::Fifo) {
                wgpu::PresentMode::Fifo
            } else {
                first
            },
            true,
        ),
    };
    Selection {
        requested,
        actual,
        available: available.to_vec(),
        fell_back,
        frame_latency,
    }
}

/// Read [`ENV_PRESENT_MODE`], reporting an unparseable value rather than
/// ignoring it.
///
/// Returns the request and, when the variable held something meaningless, a
/// complaint for the log. A typo that silently vsyncs the run would poison a
/// published number, which is exactly the failure this whole module exists to
/// prevent.
#[must_use]
pub fn request_from_env() -> (Request, Option<String>) {
    match std::env::var(ENV_PRESENT_MODE) {
        Err(_) => (Request::Adapter, None),
        Ok(raw) => match parse(&raw) {
            Some(request) => (request, None),
            None => (
                Request::Adapter,
                Some(format!(
                    "{ENV_PRESENT_MODE}={raw:?} is not a present mode \
                     (fifo|vsync, fifo_relaxed, immediate|uncapped, mailbox, auto); \
                     using the adapter's first mode"
                )),
            ),
        },
    }
}

/// Read [`ENV_FRAME_LATENCY`], clamped to wgpu's accepted 1..=3.
#[must_use]
pub fn frame_latency_from_env() -> (u32, Option<String>) {
    match std::env::var(ENV_FRAME_LATENCY) {
        Err(_) => (DEFAULT_FRAME_LATENCY, None),
        Ok(raw) => match raw.trim().parse::<u32>() {
            Ok(n) if (1..=3).contains(&n) => (n, None),
            _ => (
                DEFAULT_FRAME_LATENCY,
                Some(format!(
                    "{ENV_FRAME_LATENCY}={raw:?} is not 1, 2 or 3; \
                     using {DEFAULT_FRAME_LATENCY}"
                )),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::PresentMode as P;

    #[test]
    fn the_words_a_person_says_map_to_the_modes_they_mean() {
        assert_eq!(parse("vsync"), Some(Request::Exact(P::Fifo)));
        assert_eq!(parse("FIFO"), Some(Request::Exact(P::Fifo)));
        assert_eq!(parse(" uncapped "), Some(Request::Exact(P::Immediate)));
        assert_eq!(parse("immediate"), Some(Request::Exact(P::Immediate)));
        assert_eq!(parse("mailbox"), Some(Request::Exact(P::Mailbox)));
        assert_eq!(parse("auto"), Some(Request::Adapter));
        assert_eq!(parse("fastest"), None);
    }

    #[test]
    fn an_available_mode_is_used_as_asked() {
        let selection = choose(&[P::Fifo, P::Immediate], Request::Exact(P::Immediate), 2);
        assert_eq!(selection.actual, P::Immediate);
        assert!(!selection.fell_back);
        assert!(selection.describe().contains("present_mode=immediate"));
    }

    #[test]
    fn an_unavailable_mode_falls_back_loudly_to_fifo() {
        // The point of the test: the fallback must be visible in `actual` and
        // in the log line, because a run that quietly vsynced when it was
        // asked to run uncapped would publish a wrong number.
        let selection = choose(&[P::Fifo], Request::Exact(P::Mailbox), 2);
        assert_eq!(selection.actual, P::Fifo);
        assert!(selection.fell_back);
        let line = selection.describe();
        assert!(line.contains("FELL BACK"), "{line}");
        assert!(line.contains("mailbox"), "{line}");
    }

    #[test]
    fn no_preference_keeps_the_pre_existing_behaviour() {
        // Before this module, the surface took `caps.present_modes[0]`. With
        // no environment variable set, it still does — so adding selection
        // did not silently change what an unconfigured run measures.
        let selection = choose(&[P::Immediate, P::Fifo], Request::Adapter, 2);
        assert_eq!(selection.actual, P::Immediate);
        assert!(!selection.fell_back);
    }

    #[test]
    fn an_empty_capability_list_vsyncs_rather_than_panicking() {
        assert_eq!(choose(&[], Request::Adapter, 2).actual, P::Fifo);
        assert_eq!(choose(&[], Request::Exact(P::Mailbox), 2).actual, P::Fifo);
    }

    #[test]
    fn published_names_are_the_ones_the_pacing_log_uses() {
        assert_eq!(name(P::Fifo), "fifo");
        assert_eq!(name(P::Immediate), "immediate");
        assert_eq!(name(P::Mailbox), "mailbox");
    }
}

//! Screenshot the running straf3 client, and refuse to hand back a blank frame.
//!
//! # Why this exists
//!
//! Acceptance criterion 8 asks for a repeatable command that captures the
//! running client, explicitly instead of hand-written PowerShell. The one
//! screenshot this project had was taken with an ad-hoc `CopyFromScreen`
//! script; this replaces it with something that can be run the same way twice.
//!
//! # The rule: this tool captures a window, never the screen
//!
//! **A whole-screen capture is not an acceptable output of this tool.** Not as
//! a fallback, not as a convenience, not "just this once because the window
//! was not up". This is a requirement, not a consequence of how it happens to
//! be written today.
//!
//! The reason is concrete. This project's host is somebody's actual desktop.
//! The one screenshot straf3 had before this tool was a full 1920×1080
//! primary-screen grab, and it contains the operator's browser tabs, a video
//! playing, and their accounts — committed to a repository whose history is
//! not something anyone quietly cleans up later. A screenshot of the game is
//! evidence; a screenshot of the screen the game was on is a privacy leak
//! wearing evidence's clothes.
//!
//! So the guarantee is structural rather than promised:
//!
//! - The only normal output is the matched window's rectangle.
//! - A missing window is a **failure** (exit 4), not a desktop grab.
//! - An occluded window is a **failure** (exit 5), not a picture of whatever
//!   is covering it — see [`win::occlusion`].
//! - `--desktop` exists purely to answer "was the window even up?", and
//!   [`cli::parse`] refuses to write it anywhere but under `target/`, which
//!   `.gitignore` excludes. A path that cannot hold the file is a stronger
//!   rule than a comment asking a contributor not to commit it.
//!
//! If you are here to add a fallback, that is the change this paragraph exists
//! to stop.
//!
//! # Why it captures from the desktop DC, not from the window's own
//!
//! `BitBlt` or `PrintWindow` against a DWM-composited window that owns a
//! Vulkan swapchain routinely returns black: the window's own DC never sees
//! the pixels the GPU presented. Reading the *desktop* DC over the window's
//! rectangle reads what the compositor actually put on the display, which is
//! the claim a screenshot is supposed to support. That path is also the one
//! already demonstrated to work on this host.
//!
//! The cost of that choice is exactly what the rule above guards: the read is
//! of a screen region, so anything in front of the window lands in the image
//! instead. That is not hypothetical — the first live capture this tool took
//! came back as a picture of a browser sitting over the game. [`Image::verify`]
//! cannot catch it, because such an image is perfectly valid and perfectly
//! non-blank. [`win::occlusion`] is what catches it.
//!
//! # Why the verification is not optional
//!
//! A capture tool that silently writes an all-black PNG launders a failure
//! into evidence. Everything here is arranged so that cannot happen quietly:
//! [`Image::verify`] measures how uniform the pixels are, the binary exits
//! non-zero when they are too uniform, and the numbers it measured are
//! printed either way.
//!
//! The platform-independent half — the image buffer, the verification and the
//! PNG encode — lives here so it is compiled and unit-tested by
//! `cargo test --workspace` on Linux, where the Win32 half cannot even link.

use std::collections::HashMap;
use std::path::Path;

pub mod cli;

#[cfg(windows)]
pub mod win;

/// A captured screen region: 8-bit RGB, top row first, no padding.
///
/// RGB rather than RGBA on purpose. `BitBlt` writes nothing into the alpha
/// channel of a 32-bit DIB, so it comes back zero, and a PNG whose alpha is
/// zero everywhere is a fully transparent image that most viewers draw as
/// black or as a checkerboard. Dropping alpha at the source removes a way for
/// a correct capture to *look* like the failure this tool exists to detect.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    /// `width * height * 3` bytes, row-major from the top.
    rgb: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgb.len())
            .finish()
    }
}

impl Image {
    /// Wrap an existing RGB buffer.
    ///
    /// # Errors
    ///
    /// If the buffer is not exactly `width * height * 3` bytes, or either
    /// dimension is zero.
    pub fn from_rgb(width: u32, height: u32, rgb: Vec<u8>) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err(format!("a {width}x{height} image has no pixels"));
        }
        let want = width as usize * height as usize * 3;
        if rgb.len() != want {
            return Err(format!(
                "{width}x{height} needs {want} bytes of RGB, got {}",
                rgb.len()
            ));
        }
        Ok(Self { width, height, rgb })
    }

    /// Convert a top-down 32-bit BGRX DIB — what `GetDIBits` hands back — to RGB.
    ///
    /// # Errors
    ///
    /// If the buffer is not exactly `width * height * 4` bytes.
    pub fn from_bgrx_top_down(width: u32, height: u32, bgrx: &[u8]) -> Result<Self, String> {
        let want = width as usize * height as usize * 4;
        if bgrx.len() != want {
            return Err(format!(
                "{width}x{height} needs {want} bytes of BGRX, got {}",
                bgrx.len()
            ));
        }
        let mut rgb = Vec::with_capacity(want / 4 * 3);
        for px in bgrx.chunks_exact(4) {
            rgb.push(px[2]);
            rgb.push(px[1]);
            rgb.push(px[0]);
        }
        Self::from_rgb(width, height, rgb)
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }

    /// Measure how uniform the image is. See [`Uniformity`].
    #[must_use]
    pub fn verify(&self) -> Uniformity {
        let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
        let mut min_luma = u32::MAX;
        let mut max_luma = 0u32;
        let mut pure_black = 0u32;

        for px in self.rgb.chunks_exact(3) {
            let key = [px[0], px[1], px[2]];
            *counts.entry(key).or_insert(0) += 1;
            // Integer luma, weights x1000, so the check needs no float and
            // cannot drift between targets. Not a colour-science claim —
            // it only has to order "dark" against "light".
            let luma = 299 * u32::from(px[0]) + 587 * u32::from(px[1]) + 114 * u32::from(px[2]);
            min_luma = min_luma.min(luma);
            max_luma = max_luma.max(luma);
            if key == [0, 0, 0] {
                pure_black += 1;
            }
        }

        let pixels = (self.rgb.len() / 3) as u32;
        let (dominant, dominant_count) = counts
            .iter()
            .max_by_key(|(colour, count)| (**count, **colour))
            .map_or(([0, 0, 0], 0), |(colour, count)| (*colour, *count));

        Uniformity {
            pixels,
            distinct_colours: counts.len(),
            dominant,
            dominant_count,
            pure_black,
            min_luma: min_luma / 1000,
            max_luma: max_luma / 1000,
        }
    }

    /// Write the image as an 8-bit RGB PNG.
    ///
    /// # Errors
    ///
    /// If the file cannot be created or the encode fails.
    pub fn write_png(&self, path: &Path) -> Result<u64, String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        let file = std::fs::File::create(path)
            .map_err(|e| format!("could not create {}: {e}", path.display()))?;
        let writer = std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG header: {e}"))?;
        writer
            .write_image_data(&self.rgb)
            .map_err(|e| format!("PNG data: {e}"))?;
        writer.finish().map_err(|e| format!("PNG finish: {e}"))?;
        std::fs::metadata(path)
            .map(|m| m.len())
            .map_err(|e| format!("could not stat {}: {e}", path.display()))
    }
}

/// What the pixels of a capture actually look like.
///
/// This is the tool's evidence that it captured a frame rather than a blank
/// surface. It is reported whether or not the capture passes, so a reader can
/// see the numbers and not just the verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uniformity {
    /// Total pixels examined.
    pub pixels: u32,
    /// How many distinct RGB triples appear.
    pub distinct_colours: usize,
    /// The most common colour.
    pub dominant: [u8; 3],
    /// How many pixels are that colour.
    pub dominant_count: u32,
    /// How many pixels are exactly `#000000` — the specific failure signature
    /// of reading a GPU window's own DC.
    pub pure_black: u32,
    /// Darkest and brightest luma seen, 0..=255.
    pub min_luma: u32,
    pub max_luma: u32,
}

/// How uniform an image is allowed to be before it is called blank.
///
/// Deliberately loose. The failure this guards against is *degenerate* — one
/// colour everywhere — so the thresholds only have to separate "a frame" from
/// "a flat fill". Tightening them to something that also rejects, say, a
/// mostly-sky screenshot would start rejecting real captures, which is a worse
/// failure than the one being prevented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlankPolicy {
    /// Fewer distinct colours than this is blank.
    pub min_colours: usize,
    /// A single colour covering more than this fraction is blank. Expressed
    /// in tenths of a percent (`995` = 99.5 %) so the CLI takes an integer and
    /// no float parsing enters the verdict.
    pub max_dominant_permille: u32,
}

impl Default for BlankPolicy {
    fn default() -> Self {
        Self {
            min_colours: 16,
            max_dominant_permille: 995,
        }
    }
}

impl Uniformity {
    /// The fraction of the image covered by its most common colour, in
    /// tenths of a percent.
    #[must_use]
    pub fn dominant_permille(&self) -> u32 {
        if self.pixels == 0 {
            return 1000;
        }
        // u64 so a 4K capture cannot overflow the numerator.
        u32::try_from(u64::from(self.dominant_count) * 1000 / u64::from(self.pixels))
            .unwrap_or(1000)
    }

    /// `Some(reason)` if this image should be treated as a failed capture.
    #[must_use]
    pub fn blank_reason(&self, policy: BlankPolicy) -> Option<String> {
        let dominant = format!(
            "#{:02x}{:02x}{:02x}",
            self.dominant[0], self.dominant[1], self.dominant[2]
        );
        let permille = self.dominant_permille();
        if self.distinct_colours < policy.min_colours {
            return Some(format!(
                "only {} distinct colour(s) in {} pixels (need {}); most common is {dominant}",
                self.distinct_colours, self.pixels, policy.min_colours
            ));
        }
        if permille > policy.max_dominant_permille {
            return Some(format!(
                "{dominant} covers {}.{} % of {} pixels (limit {}.{} %)",
                permille / 10,
                permille % 10,
                self.pixels,
                policy.max_dominant_permille / 10,
                policy.max_dominant_permille % 10,
            ));
        }
        None
    }

    /// A one-line summary for the log.
    #[must_use]
    pub fn describe(&self) -> String {
        let permille = self.dominant_permille();
        format!(
            "{} distinct colours in {} px; most common #{:02x}{:02x}{:02x} at {}.{} %; \
             pure black {}.{} %; luma {}..{}",
            self.distinct_colours,
            self.pixels,
            self.dominant[0],
            self.dominant[1],
            self.dominant[2],
            permille / 10,
            permille % 10,
            self.black_permille() / 10,
            self.black_permille() % 10,
            self.min_luma,
            self.max_luma,
        )
    }

    /// Fraction of pure `#000000` pixels, in tenths of a percent.
    #[must_use]
    pub fn black_permille(&self) -> u32 {
        if self.pixels == 0 {
            return 1000;
        }
        u32::try_from(u64::from(self.pure_black) * 1000 / u64::from(self.pixels)).unwrap_or(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, colour: [u8; 3]) -> Image {
        let rgb = colour
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 3)
            .collect();
        Image::from_rgb(width, height, rgb).unwrap()
    }

    fn gradient(width: u32, height: u32) -> Image {
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.push((x % 256) as u8);
                rgb.push((y % 256) as u8);
                rgb.push(((x + y) % 256) as u8);
            }
        }
        Image::from_rgb(width, height, rgb).unwrap()
    }

    #[test]
    fn an_all_black_capture_is_rejected() {
        // This is the exact failure mode a GPU window's own DC produces, and
        // the one the tool exists to refuse to launder into evidence.
        let verdict = solid(64, 64, [0, 0, 0]).verify();
        assert_eq!(verdict.distinct_colours, 1);
        assert_eq!(verdict.pure_black, 64 * 64);
        assert_eq!(verdict.dominant_permille(), 1000);
        let reason = verdict.blank_reason(BlankPolicy::default()).unwrap();
        assert!(reason.contains("#000000"), "{reason}");
    }

    #[test]
    fn a_single_non_black_colour_is_rejected_too() {
        // A cleared-but-never-drawn swapchain is not always black; a flat
        // clear colour is just as much a failed capture.
        let verdict = solid(32, 32, [40, 44, 52]).verify();
        assert!(verdict.blank_reason(BlankPolicy::default()).is_some());
        assert_eq!(verdict.pure_black, 0);
    }

    #[test]
    fn a_real_looking_frame_passes() {
        let verdict = gradient(128, 96).verify();
        assert!(verdict.distinct_colours > 1000, "{verdict:?}");
        assert_eq!(verdict.blank_reason(BlankPolicy::default()), None);
    }

    #[test]
    fn a_frame_that_is_mostly_sky_is_not_called_blank() {
        // Guards the threshold itself: 99 % one colour is unusual but it is
        // a real frame, and rejecting it would make the tool worse than
        // useless on a screenshot of an open map.
        let w = 1000;
        let h = 100;
        let mut rgb = vec![0u8; w * h * 3];
        for (i, px) in rgb.chunks_exact_mut(3).enumerate() {
            let colour: [u8; 3] = if i < w * h / 100 {
                [(i % 251) as u8, 90, 20]
            } else {
                [120, 160, 220]
            };
            px.copy_from_slice(&colour);
        }
        let verdict = Image::from_rgb(w as u32, h as u32, rgb).unwrap().verify();
        assert_eq!(verdict.dominant, [120, 160, 220]);
        assert_eq!(verdict.dominant_permille(), 990);
        assert_eq!(verdict.blank_reason(BlankPolicy::default()), None);
    }

    #[test]
    fn bgrx_is_reordered_and_alpha_dropped() {
        // GetDIBits hands back B,G,R,X. X is whatever BitBlt left behind —
        // usually zero, which is why it must not reach the PNG.
        let bgrx = vec![10, 20, 30, 0, 40, 50, 60, 0];
        let image = Image::from_bgrx_top_down(2, 1, &bgrx).unwrap();
        assert_eq!(image.rgb(), &[30, 20, 10, 60, 50, 40]);
    }

    #[test]
    fn a_wrongly_sized_buffer_is_an_error_not_a_panic() {
        assert!(Image::from_rgb(4, 4, vec![0; 10]).is_err());
        assert!(Image::from_bgrx_top_down(4, 4, &[0; 10]).is_err());
        assert!(Image::from_rgb(0, 4, vec![]).is_err());
    }

    #[test]
    fn the_png_it_writes_reads_back_identical() {
        // Round-trip through the encoder, so a corrupt PNG cannot pass as
        // evidence either. Decoding uses the same crate, which does not prove
        // interoperability — `file(1)` on the real capture does that.
        let dir = std::env::temp_dir().join(format!(
            "straf3-capture-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("round-trip.png");
        let source = gradient(37, 11);
        let bytes = source.write_png(&path).unwrap();
        assert!(bytes > 0);

        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (37, 11));
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(&buf[..info.buffer_size()], source.rgb());

        std::fs::remove_dir_all(&dir).ok();
    }
}

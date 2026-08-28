//! The Win32 half: find the window, read the desktop, hand back pixels.
//!
//! Compiled only on Windows. Everything that can be tested without a desktop
//! lives in [`crate`] and [`crate::cli`] instead, so a Linux
//! `cargo test --workspace` still covers the parts that decide whether a
//! capture is believed.

use crate::Image;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HDC, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GA_ROOT, GetAncestor, GetSystemMetrics, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic,
    IsWindowVisible, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetProcessDPIAware, SetWindowPos, ShowWindow,
    WindowFromPoint,
};

/// A rectangle in virtual-screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    #[must_use]
    pub fn width(self) -> i32 {
        self.right - self.left
    }
    #[must_use]
    pub fn height(self) -> i32 {
        self.bottom - self.top
    }
    /// Shrink by `n` pixels on every side, unless that would leave nothing.
    #[must_use]
    pub fn inset(self, n: i32) -> Self {
        let shrunk = Self {
            left: self.left + n,
            top: self.top + n,
            right: self.right - n,
            bottom: self.bottom - n,
        };
        if shrunk.width() > 0 && shrunk.height() > 0 {
            shrunk
        } else {
            self
        }
    }

    #[must_use]
    fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({},{})-({},{}) {}x{}",
            self.left,
            self.top,
            self.right,
            self.bottom,
            self.width(),
            self.height()
        )
    }
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Self {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
}

/// How far inside its own reported rectangle a window's pixels actually start.
///
/// A window's bounds and the pixels the compositor puts on screen do not agree
/// exactly at the boundary. [`occlusion`] already says so — its hit-test grid
/// "deliberately avoids the outermost edge, where DWM's extended frame bounds
/// and the hit-test region disagree by a pixel or two" — but the capture used
/// the rectangle verbatim, so the two disagreed about the same edge in
/// opposite directions.
///
/// Measured on the straf3 window at 1282x752: with no inset, the top row and
/// the bottom row of the PNG were foreign — 338 of 1282 pixels in row 0 and
/// 1257 of 1282 in row 751 differed sharply from the row two inside them,
/// while every other row near the edge was continuous with its neighbours.
/// That is one pixel of whatever happened to be behind the window, in an image
/// this repository's standing rule says shows nothing but the window.
///
/// One pixel of somebody's desktop is not a privacy incident on its own. It is
/// still the wrong pixels, it is in a file that gets committed, and the fix is
/// to stop one pixel short — which costs a border of the window nobody looks
/// at.
pub const EDGE_BLEED_PX: i32 = 1;

/// Points per axis in the occlusion hit-test grid, so `SAMPLE_STEPS.pow(2)`
/// points in all.
///
/// # Why this is not a small number
///
/// It was 7 — a 49-point grid — and that is how the operator's Start menu got
/// into a capture that exited 0.
///
/// Measured on this project's own host: the straf3 window at 1280x750 with the
/// Start menu open over its lower-left corner. The menu covered roughly 13 % of
/// the window's area, which the 90 % floor is meant to refuse. It landed on 4 of
/// the 49 grid points, scored 91.8 %, passed, and wrote a PNG with the
/// operator's mail, LinkedIn and WhatsApp tiles composited into the frame.
///
/// The failure is not that 49 points give an imprecise estimate. It is that a
/// compact occluder falls *between* the points of a coarse grid, so the error
/// is biased towards reporting the window as clear — the one direction in which
/// being wrong lets a bad capture through. Sample spacing has to be small
/// against the things being detected: a notification toast, a menu, a tooltip.
///
/// At 127 the spacing on a 1280-wide window is ten pixels, so nothing large
/// enough to carry readable text can hide between samples. The cost is
/// `WindowFromPoint` 16 129 times, which measures well under the frame the
/// tool already spends settling.
pub const SAMPLE_STEPS: u32 = 127;

/// A top-level window found by enumeration.
#[derive(Debug, Clone)]
pub struct Found {
    pub hwnd: isize,
    pub title: String,
    pub rect: Rect,
    /// True when the rectangle came from DWM's extended frame bounds rather
    /// than `GetWindowRect`. The two differ by the invisible resize border
    /// Windows 10 and later put around a window; DWM's is what is on screen.
    pub from_dwm: bool,
    /// Lower-cased file name of the executable that owns this window, e.g.
    /// `straf3.exe`. `None` when the owning process would not answer — see
    /// [`owner_exe`], and note that [`find_windows`] treats `None` as "not
    /// proven to be the target" rather than as a pass.
    pub process: Option<String>,
}

/// Tell Windows we speak in physical pixels.
///
/// Without this, a process on a scaled desktop is handed virtualised
/// coordinates and a virtualised screen size, and a capture of a window's
/// "rect" reads the wrong region — or reads a blurry upscale of one. Called
/// before anything else touches a coordinate.
pub fn become_dpi_aware() {
    // Safety: no arguments, no state of ours involved. It fails only if
    // awareness was already set, which is fine.
    unsafe { SetProcessDPIAware() };
}

/// The bounding box of every monitor, which is what `GetDC(NULL)` covers.
#[must_use]
pub fn virtual_screen() -> Rect {
    // Safety: GetSystemMetrics takes an index and returns an int.
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        Rect {
            left: x,
            top: y,
            right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

struct EnumState {
    found: Vec<Found>,
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> i32 {
    // Safety: `lparam` is the `&mut EnumState` we passed to EnumWindows, and
    // EnumWindows calls this synchronously on the calling thread.
    let state = unsafe { &mut *(lparam as *mut EnumState) };

    // Safety: `hwnd` is supplied by EnumWindows and is valid for this call.
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }

    let mut buf = [0u16; 512];
    // Safety: buf is 512 u16s and we say so.
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), 512) };
    if len <= 0 {
        return 1;
    }
    let title = String::from_utf16_lossy(&buf[..len as usize]);

    let (rect, from_dwm) = window_rect(hwnd);
    state.found.push(Found {
        hwnd: hwnd as isize,
        title,
        rect,
        from_dwm,
        process: owner_exe(hwnd as isize),
    });
    1
}

/// The file name of the executable behind `hwnd`, lower-cased — `straf3.exe`.
///
/// # Why a window's title is not enough to identify it
///
/// This is not a refinement; it closes a hole that was open on this project's
/// own host. `find_windows` matches a title substring, and the substring is
/// `straf3` — which is also in the title of every editor, terminal and file
/// manager that happens to have a straf3 file open. Measured here, not
/// imagined: with the game not running at all, the first capture this tool
/// took after the window-only rule landed came back as a clean, valid,
/// non-blank PNG of a Notepad window called `straf3 - Notepad`, at 100 % of
/// the occlusion hit test, exit 0.
///
/// Nothing else in the tool can catch that. [`crate::Image::verify`] sees a
/// perfectly good picture; [`occlusion`] sees a window that is entirely
/// unobstructed, because it *is* — it is simply the wrong window. And the
/// failure lands exactly where the tool's whole point is to not land: an image
/// of the operator's document, filed as evidence about straf3.
///
/// So identity is asked of the operating system instead of the title bar. A
/// window belongs to straf3 when the process that owns it is the straf3
/// executable, which no amount of naming a text file can forge.
///
/// `None` when the process will not answer — a protected or higher-integrity
/// process refuses `OpenProcess` to a normal-privilege caller. Callers must
/// read that as "could not prove it", never as "close enough".
/// Takes the handle as `isize` rather than `HWND`, like [`title_of`] and
/// [`occlusion`] do: a public safe function taking a raw pointer it
/// dereferences is what `clippy::not_unsafe_ptr_arg_deref` exists to stop.
#[must_use]
pub fn owner_exe(hwnd_bits: isize) -> Option<String> {
    let hwnd = hwnd_bits as HWND;
    let mut pid: u32 = 0;
    // Safety: `pid` is a live u32 for the duration of the call.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &raw mut pid) };
    if thread == 0 || pid == 0 {
        return None;
    }

    // LIMITED_INFORMATION is the least that can name an image, and unlike
    // PROCESS_QUERY_INFORMATION it is granted across integrity levels.
    // Safety: a plain kernel32 call; a refused open returns null.
    let process: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }

    let mut buf = [0u16; 512];
    let mut len: u32 = 512;
    // Safety: `buf` is 512 u16s and `len` says so; both outlive the call, and
    // `len` is updated in place to the characters actually written.
    let ok = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &raw mut len)
    };
    // Safety: `process` came from OpenProcess above and is not used after this.
    unsafe { CloseHandle(process) };
    if ok == 0 || len == 0 || len as usize > buf.len() {
        return None;
    }

    let full = String::from_utf16_lossy(&buf[..len as usize]);
    // Split on the separators by hand rather than through `Path`: the answer
    // must be the same whichever host built this binary.
    Some(
        full.rsplit(['\\', '/'])
            .next()
            .unwrap_or(full.as_str())
            .to_lowercase(),
    )
}

/// The on-screen rectangle of a window, preferring DWM's view of it.
fn window_rect(hwnd: HWND) -> (Rect, bool) {
    let mut dwm = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // Safety: the attribute is DWMWA_EXTENDED_FRAME_BOUNDS, whose out
    // parameter is a RECT, and we pass exactly one RECT's worth of space.
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            std::ptr::from_mut(&mut dwm).cast::<c_void>(),
            u32::try_from(size_of::<RECT>()).unwrap_or(16),
        )
    };
    if hr >= 0 && dwm.right > dwm.left && dwm.bottom > dwm.top {
        return (dwm.into(), true);
    }

    let mut plain = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // Safety: `plain` is a live RECT for the duration of the call.
    if unsafe { GetWindowRect(hwnd, &raw mut plain) } != 0 {
        (plain.into(), false)
    } else {
        (
            Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            false,
        )
    }
}

/// Every visible top-level window that has a title.
#[must_use]
pub fn list_windows() -> Vec<Found> {
    let mut state = EnumState { found: Vec::new() };
    // Safety: `collect` matches WNDENUMPROC, and `state` outlives the call.
    unsafe {
        EnumWindows(Some(collect), std::ptr::from_mut(&mut state) as LPARAM);
    }
    state.found
}

/// Visible top-level windows that are on the desktop, whose title contains
/// `needle` case-insensitively, and — when `exe` is `Some` — which are owned by
/// a process with that executable file name.
///
/// Substring rather than exact match so the window title can carry a version
/// or a map name later without breaking every script that captures it. That
/// looseness is exactly why `exe` exists: a substring of `straf3` matches the
/// game, and it equally matches an editor holding a straf3 file, which this
/// tool once captured and wrote out as evidence. [`owner_exe`] has the measured
/// case. The title says what a window is called; the process says what it is.
///
/// A window whose owner will not identify itself (`process: None`) is dropped
/// whenever `exe` is `Some`. Failing closed is the point: an unidentifiable
/// window is not evidence that the game is on screen, and the alternative is
/// the wrong-window capture this filter exists to stop. `exe: None` — the
/// `--any-process` escape hatch — restores the old title-only behaviour for a
/// caller who knowingly wants it.
///
/// The on-screen filter is not a nicety either. `IsWindowVisible` is true for a
/// minimised window — Windows parks it at roughly `(-32000, -32000)` rather
/// than hiding it — so a minimised editor with `STRAF3_VISION.md` in its title
/// out-matched the running game here, and the capture failed against a window
/// that was never the target. Measured, not anticipated.
#[must_use]
pub fn find_windows(needle: &str, exe: Option<&str>) -> Vec<Found> {
    let needle = needle.to_lowercase();
    let exe = exe.map(str::to_lowercase);
    let screen = virtual_screen();
    let mut hits: Vec<Found> = list_windows()
        .into_iter()
        .filter(|w| w.title.to_lowercase().contains(&needle))
        .filter(|w| match exe.as_deref() {
            None => true,
            Some(want) => w.process.as_deref() == Some(want),
        })
        .filter(|w| {
            let on_screen = w.rect.intersect(screen);
            on_screen.width() > 0 && on_screen.height() > 0
        })
        .collect();
    // Largest first: if a tooltip or a console window also matches, the one
    // with pixels in it is the one worth capturing.
    hits.sort_by_key(|w| std::cmp::Reverse(i64::from(w.rect.width()) * i64::from(w.rect.height())));
    hits
}

/// Put a window in front, so the desktop read sees it rather than whatever is
/// on top of it.
///
/// # Why this is not just `SetForegroundWindow`
///
/// Windows refuses focus changes from a process that does not own the
/// foreground, and this tool is launched from a shell. Measured on this host:
/// with the operator actively using a browser, `SetForegroundWindow` and
/// `BringWindowToTop` were both refused and the capture failed twice in a row
/// against a game window that was running perfectly well behind Chrome.
///
/// The topmost toggle is not refused. `SetWindowPos(HWND_TOPMOST, …,
/// SWP_NOACTIVATE)` changes z-order without asking for activation, which is
/// exactly the distinction the focus-stealing rules care about — and it is
/// also the politer thing to do to somebody who is typing, since it raises the
/// window without taking their keyboard.
///
/// It must be undone. Leaving the game pinned above everything else on the
/// operator's desktop, after a tool they ran for one screenshot, would be a
/// worse bug than the one this fixes — so [`Raised::drop`] restores the
/// previous z-order on every exit path, including the error ones.
#[must_use]
pub fn raise(hwnd_bits: isize) -> Raised {
    let hwnd = hwnd_bits as HWND;
    // Safety: `hwnd` came from EnumWindows in this process, and each call is a
    // plain user32 call that tolerates a stale handle by returning false.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        BringWindowToTop(hwnd);
        // Still attempted, because a window that also has focus is what a
        // person would have produced by clicking on it. Refusal is fine.
        SetForegroundWindow(hwnd);
    }
    Raised { hwnd }
}

/// A window this tool pushed to the top, which it will put back.
pub struct Raised {
    hwnd: HWND,
}

impl Drop for Raised {
    fn drop(&mut self) {
        // Safety: the same handle raise() was given; a stale one returns false.
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

/// How much of a window's rectangle actually belongs to that window on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occlusion {
    /// Points hit-tested.
    pub sampled: u32,
    /// How many resolved to this window (or one of its children).
    pub ours: u32,
    /// The window sitting on top at the first point that was not ours, if any.
    pub blocker: Option<isize>,
}

impl Occlusion {
    /// Visible fraction, in tenths of a percent.
    #[must_use]
    pub fn visible_permille(self) -> u32 {
        if self.sampled == 0 {
            return 0;
        }
        self.ours * 1000 / self.sampled
    }
}

/// Hit-test a grid of points inside `rect` and report how many of them the
/// desktop says belong to `hwnd_bits`.
///
/// # Why this exists
///
/// Reading the desktop DC over a window's rectangle returns *what is on
/// screen there*, which is not the same thing as *that window*. On a live
/// desktop the first real capture this tool took came back as a picture of a
/// browser that happened to be in front — a perfectly valid, perfectly
/// non-blank image of the wrong thing, which the uniformity check has no way
/// to catch. That is the failure mode this closes, and it matters twice over:
/// a wrong-window capture is not evidence, and it puts whatever the operator
/// had open into a file that was meant to hold a game.
///
/// The grid deliberately avoids the outermost edge, where DWM's extended
/// frame bounds and the hit-test region disagree by a pixel or two.
///
/// See [`SAMPLE_STEPS`] for why the grid is as dense as it is: a coarse one
/// does not merely report an imprecise number, it reports a *reassuring* one.
#[must_use]
pub fn occlusion(hwnd_bits: isize, rect: Rect, steps: u32) -> Occlusion {
    let hwnd = hwnd_bits as HWND;
    let steps = steps.max(2);
    let mut sampled = 0;
    let mut ours = 0;
    let mut blocker = None;

    for iy in 0..steps {
        for ix in 0..steps {
            // (i + 1) / (steps + 1) keeps every sample strictly inside.
            let x = rect.left + (rect.width() * (ix as i32 + 1)) / (steps as i32 + 1);
            let y = rect.top + (rect.height() * (iy as i32 + 1)) / (steps as i32 + 1);
            // Safety: WindowFromPoint takes a POINT by value and returns a
            // handle or null; nothing is borrowed.
            let at = unsafe {
                let hit = WindowFromPoint(POINT { x, y });
                if hit.is_null() {
                    std::ptr::null_mut()
                } else {
                    // A window's client area is often a child HWND; walk up so
                    // the game's own surface counts as the game.
                    GetAncestor(hit, GA_ROOT)
                }
            };
            sampled += 1;
            if std::ptr::eq(at, hwnd) {
                ours += 1;
            } else if blocker.is_none() && !at.is_null() {
                blocker = Some(at as isize);
            }
        }
    }

    Occlusion {
        sampled,
        ours,
        blocker,
    }
}

/// The title of a window, for naming whatever is covering the target.
#[must_use]
pub fn title_of(hwnd_bits: isize) -> String {
    let mut buf = [0u16; 512];
    // Safety: the buffer is 512 u16s and we say so; a stale handle returns 0.
    let len = unsafe { GetWindowTextW(hwnd_bits as HWND, buf.as_mut_ptr(), 512) };
    if len <= 0 {
        return "<untitled>".to_owned();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

/// Read a rectangle of the desktop.
///
/// The region is clipped to the virtual screen first: `BitBlt` happily
/// succeeds for an off-screen source and fills the result with black, which
/// would be indistinguishable from the failure this tool exists to catch.
///
/// # Errors
///
/// If the rectangle is empty after clipping, or any GDI call fails.
pub fn capture_rect(rect: Rect) -> Result<(Image, Rect), String> {
    let clipped = rect.intersect(virtual_screen());
    if clipped.width() <= 0 || clipped.height() <= 0 {
        return Err(format!(
            "{rect} does not overlap the virtual screen {}; nothing to capture \
             (is the window minimised?)",
            virtual_screen()
        ));
    }

    let width = clipped.width();
    let height = clipped.height();

    // Safety: every call below is a plain GDI call on handles we created, and
    // `Gdi` releases all of them on every exit path including the error ones.
    unsafe {
        let screen: HDC = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return Err("GetDC(NULL) failed: no desktop device context".into());
        }
        let mem = CreateCompatibleDC(screen);
        if mem.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen);
            return Err("CreateCompatibleDC failed".into());
        }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap.is_null() {
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            return Err(format!("CreateCompatibleBitmap({width}x{height}) failed"));
        }
        let previous = SelectObject(mem, bitmap);

        let blitted = BitBlt(
            mem,
            0,
            0,
            width,
            height,
            screen,
            clipped.left,
            clipped.top,
            SRCCOPY,
        );

        // GetDIBits requires the bitmap NOT to be selected into the DC it is
        // read through. Deselect before reading, or it returns zero scanlines.
        SelectObject(mem, previous);

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
            biWidth: width,
            // Negative height asks for a top-down DIB, so row 0 is the top
            // row and no flip is needed afterwards.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        };

        let stride = (width as usize) * 4;
        let mut buffer = vec![0u8; stride * height as usize];
        let lines = GetDIBits(
            mem,
            bitmap,
            0,
            u32::try_from(height).unwrap_or(0),
            buffer.as_mut_ptr().cast::<c_void>(),
            &raw mut info,
            DIB_RGB_COLORS,
        );

        DeleteObject(bitmap);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);

        if blitted == 0 {
            return Err(format!("BitBlt of {clipped} from the desktop failed"));
        }
        if lines != height {
            return Err(format!(
                "GetDIBits returned {lines} scanlines, expected {height}"
            ));
        }

        let image = Image::from_bgrx_top_down(
            u32::try_from(width).unwrap_or(0),
            u32::try_from(height).unwrap_or(0),
            &buffer,
        )?;
        Ok((image, clipped))
    }
}

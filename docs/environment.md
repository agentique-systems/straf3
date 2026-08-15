# Build environment

Status: **verified working** on 2026-08-15. Everything below was executed and its output checked;
nothing here is inferred from documentation.

Host: `DESKTOP-G99TTEO`, Ubuntu 24.04.1 LTS (noble) under WSL2, kernel `6.18.33.2-microsoft-standard-WSL2`,
x86-64. Project root `/home/cairon/agentique-runs/straf3`. `sudo` works non-interactively (user `cairon`
is in group `sudo`, no password prompt), so package installation was unattended.

---

## 1. Rust toolchain

Installed via the pre-existing rustup (`/home/cairon/.cargo/bin/rustup`), which had **zero** toolchains before.

```
rustup toolchain install stable --profile default --no-self-update
```

| Component | Version |
|---|---|
| rustc | `1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6 |
| cargo | `1.97.1 (c980f4866 2026-06-30)` |
| host triple | `x86_64-unknown-linux-gnu` |
| profile | `default` (rustc, cargo, rust-std, rust-docs, rustfmt, clippy) |
| rustup home | `/home/cairon/.rustup` |

Installed targets: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`.

There is **no `rust-toolchain.toml` pin yet**. Recommend the workspace seat add one pinning `1.97.1`, since
determinism is a project requirement (spec §2) and float codegen can shift between compiler versions.

## 2. System packages

Ubuntu 24.04 package names were checked against `apt-cache policy` before installing — all exist in the
noble repos. `build-essential` was already present; everything else was new.

```
sudo -n -E DEBIAN_FRONTEND=noninteractive apt-get update
sudo -n -E DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  build-essential pkg-config clang libclang-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols \
  libvulkan-dev mingw-w64
```

Explicitly requested packages and the versions that landed:

| Package | Version | Why |
|---|---|---|
| `build-essential` | 12.10ubuntu1 (already installed) | cc/ld for `cc-rs` build scripts |
| `pkg-config` | 1.8.1-2build1 | how `*-sys` crates find X11/Wayland/xkbcommon |
| `clang` | 1:18.0-59~exp2 (clang 18.1.3) | `bindgen` frontend |
| `libclang-dev` | 1:18.0-59~exp2 | `libclang.so` for `bindgen` (`/usr/lib/llvm-18/lib/libclang.so`) |
| `libx11-dev` | 2:1.8.7-1build1 | winit X11 backend |
| `libxcursor-dev` | 1:1.2.1-1build1 | cursors |
| `libxrandr-dev` | 2:1.5.2-2build1 | monitor/mode enumeration |
| `libxi-dev` | 2:1.8.1-1build1 | XInput2 raw mouse input |
| `libxkbcommon-dev` | 1.6.0-1build1 | keymap handling (both backends) |
| `libxkbcommon-x11-dev` | 1.6.0-1build1 | xkbcommon X11 glue |
| `libwayland-dev` | 1.22.0-2.1build1 | winit Wayland backend |
| `wayland-protocols` | 1.45-1~ubuntu0.24.04.2 | protocol XML for Wayland codegen |
| `libvulkan-dev` | 1.3.275.0-1build1 | Vulkan loader headers/`vulkan.pc` |
| `mingw-w64` | 11.0.1-3build1 (gcc 13.2.0) | Windows cross-linker, see §3 |

Pulled in automatically (partial, the ones that matter): `clang-18`, `libclang-18-dev`, `libclang1-18`,
`libllvm18`, `libxcb1-dev`, `libxcb-xkb-dev`, `libxext-dev`, `libxfixes-dev`, `libxrender-dev`,
`x11proto-dev`, `libwayland-bin`, `pkgconf`, `binutils-mingw-w64-x86-64`, `gcc-mingw-w64-x86-64-*`,
`g++-mingw-w64-*`, `mingw-w64-x86-64-dev`. `mesa-vulkan-drivers` 25.2.8 was already installed.

`pkg-config` resolves every module winit/wgpu needs:

```
x11 1.8.7   xcursor 1.2.1   xrandr 1.5.2   xi 1.8.1
xkbcommon 1.6.0   xkbcommon-x11 1.6.0
wayland-client 1.22.0   wayland-cursor 1.22.0   wayland-egl 18.1.0
vulkan 1.3.275
```

## 3. Windows target — cross-compiling from WSL2 **works**, and the exe runs natively

This turned out much better than the spec assumed, and it changes the day-to-day loop.

```
rustup target add x86_64-pc-windows-gnu
```

`x86_64-pc-windows-msvc` was **not** chosen: it needs the MSVC link.exe plus the Windows SDK
import libraries, which means either `xwin` fetching the SDK or building on the Windows side. `-gnu`
links with the mingw-w64 toolchain we already have and produced a working binary on the first attempt.

**Verified:** a full winit + wgpu + egui + rapier3d binary cross-compiles and **links** cleanly:

```
cargo build --target x86_64-pc-windows-gnu     # 39.9 s cold for 313 crates
file …/straf3-smoke.exe
  → PE32+ executable (console) x86-64, for MS Windows, 21 sections
```

No `.cargo/config.toml` linker override was needed — rustc found `x86_64-w64-mingw32-gcc` (GCC 13-win32)
on `PATH` by itself. There is no `~/.cargo/config.toml` on this machine, and no `RUST*`/`CARGO*` env
overrides are set; the build is reproducible from a clean shell.

The exe imports **only Windows system DLLs** — `kernel32`, `user32`, `gdi32`, `ole32`, `combase`,
`dxgi`, `opengl32`, `dwmapi`, `uxtheme`, `imm32`, `setupapi`, `shell32`, `ws2_32`, `bcryptprimitives`,
`msvcrt`, `ntdll`, `rpcrt4`, `oleaut32`, `userenv`. **No `libgcc_s_seh-1.dll` / `libwinpthread-1.dll`
redistributables are required** — it is a self-contained drop-and-run binary.

### The important part: it runs against the real GPU, launched from WSL

WSL interop is enabled (`/proc/sys/fs/binfmt_misc/WSLInterop` → `enabled`), so the `.exe` can be executed
straight from the WSL shell. It runs as a **real Windows process** on the Windows side. Actual output:

```
winit: event loop created OK
adapter: Vulkan  DiscreteGpu  "NVIDIA GeForce RTX 3060 Ti"  driver NVIDIA 560.94
adapter: Dx12    DiscreteGpu  "NVIDIA GeForce RTX 3060 Ti"  driver 32.0.15.6094
adapter: Dx12    Cpu          "Microsoft Basic Render Driver"
adapter: Gl      Other        "NVIDIA GeForce RTX 3060 Ti/PCIe/SSE2"  4.6.0 NVIDIA 560.94
request_adapter -> NVIDIA GeForce RTX 3060 Ti, DiscreteGpu, Vulkan, pci 0000:07:00.0
```

Real discrete adapter, real DX12 and Vulkan backends, real driver — none of the WSLg software path.
So `cargo build --target x86_64-pc-windows-gnu && ./target/x86_64-pc-windows-gnu/debug/game.exe` **is**
the play-and-tune loop, driven from this shell. No Windows-side Rust install is needed.

Caveats on that loop, none of them blocking:
- The exe lives on the Linux filesystem and Windows reaches it over `\\wsl.localhost` (9p). Process
  execution is native, but **startup and any file I/O the game does are slow and go through the network
  redirector**. For timing runs, copy or build into a Windows-native path (`/mnt/c/...`) — or set
  `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_DIR=/mnt/c/straf3-target`. Do not benchmark asset loading over 9p.
- `-gnu` uses the mingw CRT, not MSVC's. For this project that is fine (all graphics/input go through
  Win32/DXGI, which are ABI-stable). Move to `-msvc` only if a dependency ever needs MSVC-only libraries
  or if you want the MSVC debugger/PDBs; that switch means installing the toolchain on the Windows side
  or `cargo xwin`, and is not needed today.
- Backtraces/PDBs: `-gnu` emits DWARF, so Windows-native debuggers (WinDbg/VS) will not read symbols.
  `RUST_BACKTRACE=1` inside the process still works.

### If cross-linking ever breaks: the native Windows path

Fallback steps for the operator, on the Windows side, in a PowerShell prompt:

1. Install Rust: download `https://win.rustup.rs/x86_64` (`rustup-init.exe`) and run
   `.\rustup-init.exe -y --default-toolchain 1.97.1 --default-host x86_64-pc-windows-msvc`.
2. Install the MSVC C++ build tools when rustup offers them (Visual Studio Build Tools →
   "Desktop development with C++", which supplies `link.exe` and the Windows 10/11 SDK).
3. `cd \\wsl.localhost\Ubuntu\home\cairon\agentique-runs\straf3` — or `git clone` to a native path,
   which is strongly preferred for build speed.
4. `cargo run --release -p straf3-game`.

No cross-compilation is involved in that path, and it is the only way to get MSVC-format PDBs.

## 4. Resolved crate versions — ground truth

Obtained by `cargo add` in a throwaway crate (`/tmp/straf3-smoke`, since deleted) against the live
crates.io index on 2026-08-15, resolving "latest compatible with Rust 1.97.1". These are the versions
the workspace should pin; they are what actually resolves today, not what any documentation claims.

| Crate | Resolved | Notes |
|---|---|---|
| `wgpu` | **30.0.0** | default features `vulkan, dx12, metal, gles, webgpu, wgsl, std, parking_lot` |
| `winit` | **0.30.13** | `ApplicationHandler` API |
| `egui` | **0.36.1** | pairs with `epaint`/`ecolor` 0.36.1 |
| `parry3d` | **0.30.2** | f32 build |
| `rapier3d` | **0.35.1** | f32 build |
| `glam` | **0.33.3** | see the `fast-math` prohibition below |
| `egui-wgpu` | **0.36.1** | must stay in lockstep with `egui` |
| `egui-winit` | **0.36.1** | must stay in lockstep with `egui` |
| `gltf` | **1.4.1** | resolved but unused so far; upstream has been quiet since 2024-05 |

`egui-wgpu` / `egui-winit` / `gltf` were resolved with `cargo add --dry-run` — their versions are real,
but unlike the six above they were **not** compiled or linked here.

These match, version for version, the pins independently researched by the coordinator, so two separate
methods agree on all of them. `winit` is deliberately **0.30.13, the stable `ApplicationHandler` line —
not the 0.31 beta rewrite.**

### `glam`: `fast-math` must never be enabled

`glam` 0.33.3 exposes a `fast-math` feature. It **must not be turned on anywhere in this workspace, in any
crate, for any profile.** It permits reassociation and other float transforms that break glam's otherwise
bit-identical behaviour, which would silently destroy the determinism the whole project rests on (spec §4).
It fails quietly — replays would simply stop reproducing.

Verified rather than assumed, in the smoke crate:
- glam 0.33.3's `[features]` list contains `fast-math = []`, and `default = ["std", "all-types"]` — so it
  is strictly opt-in.
- `cargo tree -e features` shows the features actually enabled across the whole graph are
  `default, std, all-types, float-types, f64, integer-types, i8..i64, u8..u64, isize, usize, size-types,
  approx, nostd-libm`. **`fast-math` is not among them** — nothing transitively enables it today.

So the requirement is to keep it that way: never add it to a feature list, and never let a dependency's
feature unification pull it in. Worth a CI assertion on the feature tree.

Transitive glam versions pulled in by the render stack are fine and expected — `straf3-sim` keeps its own
pin behind the numeric seam, so a split graph is not a problem needing a fix.

Relevant transitives actually compiled: `naga` 30.0.0, `wgpu-core`/`wgpu-hal` 30.0.0, `ash` 0.38.0+1.3.281,
`nalgebra` 0.35.0, `glamx` 0.3.0, `raw-window-handle` 0.6.2, `bytemuck` 1.25.2, `pollster` 1.0.1.
Whole graph: 313 packages.

Two things worth knowing before the workspace pins these:

- **`nalgebra` 0.35 converts to `glam` 0.33** (via `glamx` 0.3). So parry3d/rapier3d and our own math
  agree on one glam version — no duplicate-glam type mismatch at the collision seam. `Cargo.lock`
  additionally *lists* glam 0.30.10 / 0.31.1 / 0.32.1, but `cargo tree` confirms none of them are in the
  build graph; they are inert entries from optional conversion features.
- **`parry3d`/`rapier3d` are nalgebra-based internally.** Spec §4 already restricts rapier to
  non-movement rigid bodies. The nalgebra↔glam conversion at the parry boundary is a real cost and a
  real determinism surface — it belongs in the parry determinism audit the spec calls for.

### wgpu 30 API notes (found by compiling, not by reading docs)

These differ from most wgpu material online and cost three compile iterations:

- `wgpu::InstanceDescriptor::default()` **no longer exists**. Use
  `InstanceDescriptor::new_without_display_handle()` (or `..._from_env()` to honour `WGPU_BACKEND`).
- `Instance::new` takes the descriptor **by value**, not by reference.
- `Instance::enumerate_adapters()` is **async** — it returns a future of `Vec<Adapter>`.
- `Instance::request_adapter()` returns `Result<Adapter, RequestAdapterError>`, not `Option`.
- `egui::Context` has no `available_rect()`; `pixels_per_point()` is the cheap liveness check.

## 5. Proof that it builds and links

Throwaway crate `/tmp/straf3-smoke` (deleted afterwards, as instructed — it never touched the workspace).
`src/main.rs`, reproduced here so the check can be repeated:

```rust
use winit::event_loop::EventLoop;

fn main() {
    match EventLoop::new() {
        Ok(_el) => println!("winit: event loop created OK"),
        Err(e) => println!("winit: event loop unavailable at runtime: {e}"),
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    for a in pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all())) {
        let i = a.get_info();
        println!("adapter: {:?} {:?} name={:?} driver={:?} {:?}",
                 i.backend, i.device_type, i.name, i.driver, i.driver_info);
    }
    match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        Ok(a) => println!("request_adapter -> {:?}", a.get_info()),
        Err(e) => println!("request_adapter failed: {e}"),
    }

    let v = glam::Vec3::new(320.0, 0.0, 0.0);
    let ball = parry3d::shape::Ball::new(1.0);
    let mut set = rapier3d::prelude::RigidBodySet::new();
    let ctx = egui::Context::default();
    println!("glam len={} parry ball r={} rapier bodies={} egui ctx ok={}",
        v.length(), ball.radius,
        { set.insert(rapier3d::prelude::RigidBodyBuilder::dynamic().build()); set.len() },
        ctx.pixels_per_point().is_finite());
}
```

Results:

| Target | Build | Link | Run |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | ok | ok | ok — llvmpipe adapters, exit 0 |
| `x86_64-pc-windows-gnu` | ok (39.9 s cold) | ok | ok via WSL interop — **RTX 3060 Ti**, exit 0 |

Linux run, with WSLg display present:

```
winit: event loop created OK
adapter: Vulkan Cpu "llvmpipe (LLVM 20.1.2, 256 bits)"  Mesa 25.2.8
adapter: Gl     Cpu "llvmpipe (LLVM 20.1.2, 256 bits)"  4.5 (Core Profile) Mesa 25.2.8
request_adapter -> llvmpipe, device_type: Cpu, backend: Vulkan
```

With `DISPLAY` and `WAYLAND_DISPLAY` both unset, wgpu still enumerates both llvmpipe adapters and
`request_adapter` still succeeds — **wgpu works fully headless here**, which is what CI and headless
determinism tests need. Only winit fails, with a clean error rather than a hang:
`neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set`. Any headless test harness must therefore
avoid constructing an `EventLoop`, which the spec's platform/sim split already implies.

## 6. Caveats that must not be forgotten

**Never trust a frame-pacing or latency number measured on the WSL2/Linux side.** Confirmed again here:
the only adapters visible to the Linux build are `llvmpipe` (Vulkan, `device_type: Cpu`) and llvmpipe
under GL. There is no `/dev/dri`, no NVIDIA ICD, and presentation goes through Weston's RDP backend to a
synthetic 59.96 Hz virtual output with no real vblank. A wgpu window opened here **will render and will
look plausible** — that is exactly the trap. Frame times, present latency, input-to-photon, and anything
derived from them are fiction on this path.

The one thing that changed: you do not need to leave this shell to get real numbers. Build for
`x86_64-pc-windows-gnu` and run the `.exe`; it is a native Windows process on the real GPU with a real
swapchain and real vblank. Treat that, and only that, as measurement.

Corollaries for the workspace and CI:
- WSL2/Linux is for compiling, unit tests, headless determinism checks, and replay regression runs.
- Any "does it feel right" or timing work runs the Windows binary.
- Determinism scope per spec §4 is same-binary/same-machine. Linux and Windows builds use different
  codegen backends and CRTs, so **do not** assume a replay recorded on the Windows build reproduces
  bit-identically under the Linux build. If cross-target replay comparison is ever wanted, it needs its
  own decision; today, record which target produced a replay.

## 7. Nothing failed

No blocker was hit. `sudo -n` worked, every package existed under the name expected on Ubuntu 24.04, the
toolchain installed clean, and both targets built and linked. The only friction was three wgpu 30 API
changes (§4), fixed and documented.

Not verified, and honestly flagged:
- No pinning file exists yet — versions above are what resolves *today*, and would drift on a fresh
  `cargo add` tomorrow. Pin them.
- `cargo build --release` was not exercised for either target, only `dev`.
- No swapchain was created and no frame was presented on either target; the proof is adapter
  enumeration plus a successful link, which is what was asked for.
- Parry/rapier determinism was not audited — that is a separate task the spec calls for.
- Windows-side native MSVC steps in §3 are written from the standard rustup/VS Build Tools procedure and
  were **not** executed, since cross-compilation made them unnecessary.

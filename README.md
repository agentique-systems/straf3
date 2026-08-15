# straf3

A first-person **movement game** in the Quake 3 Defrag tradition. No combat, no
opponents: a map, a clock, and a movement system deep enough to spend years on.

You spawn at a start line, the timer starts when you cross it and stops at the
finish. Your best run per map is saved and replayed as a ghost you race
against. Strafejumping, circle jumps, ramp boosts, overbounce, edge clipping,
rocket jumps and plasma climbs are the vocabulary. The whole product is the
quality of the movement.

Two properties are treated as correctness, not polish:

- **Determinism.** The same inputs produce the same run, always — which is what
  makes replays, ghosts and regression tests possible.
- **Latency and frame pacing.** Measured and defended, not assumed.

> **Play and tune on native Windows.** Under WSL2/WSLg, Vulkan resolves to the
> software rasteriser `lavapipe` and presentation goes through Weston's RDP
> backend with no real vblank. A window will open and it will look fine, but
> anything it tells you about frame pacing or input latency there is fiction.
> WSL2 is for building, headless tests and determinism checks.

## Workspace layout

```
straf3-game        the executable: window, loop, glue
straf3-render      wgpu + WGSL renderer
straf3-platform    winit windowing, raw input, timing
straf3-devtools    egui overlays, movement telemetry
───────────────────────────────────────────────────────  the seam
straf3-sim         the simulation. no rendering, no windowing, no I/O
straf3-collision   swept convex queries against static geometry
straf3-map         .map parsing + compiled map format
straf3-replay      input recording and playback
```

Plus `xtask/`, which holds workspace automation.

## The seam rule

**Nothing below the line may depend on anything above it.**

`straf3-sim` additionally may not reach anything that touches the filesystem, a
window, or a GPU. It is a pure function from `(state, commands) → state`; it
knows nothing about frames, files or adapters.

This is the single architectural property most worth protecting. It is what
lets the simulation run headless in CI, lets a replay be a regression test, and
would later let a server or an RL environment reuse the physics without
touching it. It is also exactly the kind of property that dies quietly to one
convenient `use straf3_platform::...`.

So it is enforced, not documented:

```sh
cargo xtask check-seam
```

That command runs `cargo tree` and inspects the **actually resolved**
dependency graph — transitively, across all targets (`--target all`), with all
features enabled (`--all-features`), including build- and dev-dependency edges.
It fails, printing the offending chain, if:

- any of `straf3-sim`, `straf3-collision`, `straf3-map`, `straf3-replay`
  reaches `straf3-platform`, `straf3-render`, `straf3-devtools` or
  `straf3-game`, at any depth; or
- any of them reaches a known windowing / GPU / UI / audio / filesystem / async
  crate (`winit`, `wgpu`, `egui`, `tokio`, `tempfile`, …); or
- `straf3-sim`'s own sources mention `std::fs`, `std::net`, `std::process`,
  `std::env`, `Instant`, `SystemTime`, `include_str!` or `include_bytes!` —
  `std` needs no dependency edge, so the graph check alone cannot see this; or
- any crate enables `glam`'s `fast-math` feature, checked under both default
  and `--all-features` resolution. `fast-math` permits float reassociation,
  which silently breaks bit-identical replay — the property ghosts, regression
  tests and any later RL environment all rest on. It is opt-in today; this
  keeps it that way.

The same check runs as a plain test (`cargo test --workspace`) and as its own
CI job, so it fails whether or not anyone remembers the command. The
implementation, and the reasoning behind each list, is in
[`xtask/src/seam.rs`](xtask/src/seam.rs).

The third-party denylist is a backstop and is necessarily incomplete. The real
guarantee is structural: rendering and windowing live in the crates above the
line, and those crates are unreachable from below.

## Building

```sh
cargo build --workspace     # everything
cargo test  --workspace     # includes the seam test
cargo xtask check-seam      # the architectural gate on its own
```

Linux builds need `pkg-config`, `clang`, and X11/Wayland/xkbcommon development
headers before `winit` and `wgpu` will link. None of them are needed to build
or test the crates below the seam.

The toolchain is pinned to an exact rustc version in `rust-toolchain.toml`,
not to `stable`, for the same reason: float codegen shifts between compiler
releases, so the compiler version is part of the determinism contract.

## Status

Early. Every crate is a compiling stub; the movement code is not written yet.
Dependency versions in the root `[workspace.dependencies]` table are pinned to
exact resolved versions.

Note that `rust-version = "1.87"` is load-bearing rather than decorative:
cargo's MSRV-aware resolver silently caps `wgpu` at 26.0.1 if it says 1.85.

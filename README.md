# straf3

A first-person **movement game** in the Quake 3 Defrag tradition. No combat, no
opponents: a map, a clock, and a movement system deep enough to spend years on.

You spawn at a start line, the timer starts when you cross it and stops at the
finish. Your best run per map is saved and replayed as a ghost you race
against — that loop has been closed once on real hardware, and its evidence is
not yet committed; see [Status](#status). The whole product is the quality of
the movement.

The intended vocabulary is strafejumping, circle jumps, ramp boosts,
overbounce, edge clipping, rocket jumps and plasma climbs. That is the target,
not an inventory: there are no weapons yet, so the last two are vision rather
than present tense. What actually runs today is in [Status](#status); what you
can do with it, and what is not proven, is in [PLAYING.md](PLAYING.md). The
governing document is [`docs/VISION.md`](docs/VISION.md).

Two properties are treated as correctness, not polish:

- **Determinism.** The same inputs produce the same run, always — which is what
  makes replays, ghosts and regression tests possible.
- **Latency and frame pacing.** Measured and defended, not assumed.

> **Play and tune on native Windows.** Under WSL2/WSLg, Vulkan resolves to
> Mesa's software rasteriser — the `lavapipe` driver, which reports itself
> under the adapter name `llvmpipe` you will see in the logs — and presentation
> goes through Weston's RDP backend with no real vblank. A window will open and
> it will look fine, but anything it tells you about frame pacing or input
> latency there is fiction. WSL2 is for building, headless tests and
> determinism checks.
>
> You do not have to leave the WSL shell to get real numbers. The
> `x86_64-pc-windows-gnu` cross-build runs as a native Windows process against
> the real GPU, launched from here through WSL interop — see
> [Building](#building), and [PLAYTEST.md](PLAYTEST.md) if you are here to play.

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

Two more workspace members sit outside `crates/`: `xtask/`, which holds
workspace automation, and `tools/det-runner/`, which owns the reference command
stream the cross-target determinism check replays. It is a member rather than a
standalone crate so that it resolves `glam` through this workspace's
`Cargo.lock` — a check built against a different resolution would be verifying a
different tree.

`probes/` holds standalone crates, each with its own lockfile and none of them
workspace members. A probe answers one empirical question, publishes its
numbers, and is kept as evidence rather than as a build input.

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
- `straf3-sim` calls a **transcendental function** — `sin`, `cos`, `sin_cos`,
  `tan`, `asin`, `acos`, `atan2`, `exp`, `ln`, `powf` — anywhere outside
  `num.rs`. These are not IEEE-specified, so two targets may legitimately
  disagree in the last bit, which is fatal to a replay. `num.rs` is the one file
  allowed to hold deterministic replacements, and its tests compare against libm
  on purpose. `sqrt` is deliberately absent from the list: it *is* specified and
  correctly rounded, so `normalize` may keep calling it; or
- any crate enables `glam`'s `fast-math` or `scalar-math` feature, checked under
  both default and `--all-features` resolution. `fast-math` permits float
  reassociation and `scalar-math` replaces glam's SIMD paths with scalar ones;
  either silently breaks bit-identical replay — the property ghosts, regression
  tests and any later RL environment all rest on. Both are opt-in today; this
  keeps them that way; or
- `glam`'s libm-family features (`libm`, `nostd-libm`) are enabled *and*
  `glam/std` is off, which is exactly when they change what float math runs.
  The check is conditional because the hazard is: with `glam/std` on, the libm
  dependency is inert and enabling the feature changes nothing. If the workspace
  ever goes `no_std`, this becomes a violation the same day.

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
cargo xtask determinism     # one command stream, four targets, one digest
```

`cargo xtask determinism` must run with **`CARGO_TARGET_DIR` unset**. It looks
for each target's artefact at the workspace-relative path, so an override makes
it exit non-zero saying the binary "was not produced" — a loud infrastructure
failure rather than a false pass, but one that looks like a determinism break to
anyone following ordinary shared-build-cache hygiene.

Linux builds need `pkg-config`, `clang`, `libvulkan-dev` and
X11/Wayland/xkbcommon development headers before `winit` and `wgpu` will link;
`mingw-w64` supplies the cross-linker for the Windows target below. None of them
are needed to build or test the crates below the seam. The exact package list
that was installed and verified on Ubuntu 24.04 is in
[`docs/environment.md`](docs/environment.md) §2.

To build the binary that reaches the real GPU, from this same shell:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu -p straf3-game --bin straf3
./target/x86_64-pc-windows-gnu/release/straf3.exe      # runs via WSL interop
```

No `.cargo/config.toml` linker override and no Windows-side Rust install are
needed. That path, and the caveats that come with it, are documented in
`docs/environment.md` §3 and in [PLAYING.md](PLAYING.md).

The toolchain is pinned to an exact rustc version in `rust-toolchain.toml`,
not to `stable`, for the same reason: float codegen shifts between compiler
releases, so the compiler version is part of the determinism contract. That file
also lists `x86_64-pc-windows-gnu`, so the play-and-tune target comes with the
toolchain.

## Status

**The movement code is written and the game is playable.** The native client
compiles a Valve 220 `.map` into the geometry you collide with *and* the
geometry you see, runs Straf3's own frozen movement canon at a fixed 125 Hz
command rate, records and replays runs whose checksums three independent readers
agree on, and can drive a windowed session from a recording (`--play`).

That canon — `--profile straf3`, and what a session runs when you name no
profile — is **numerically equal to the Q3/CPM model it was reconstructed
beside**. `docs/movement-canon.md` Part 2 judged three candidate mechanics and
rejected all three, so the freeze moved no constant and Straf3's ruleset came
out equal to CPM's. That equality is a finding this tree keeps visible rather
than a link between the two; `cpm` and `vq3` remain selectable and ranked. `cargo test
--workspace` is green — 40 suites, 543 tests, on 2026-08-17 — and it includes
the seam gate; `check-seam` and `determinism` additionally run as their own CI
jobs, the latter across four targets. It has been played on a real RTX 3060 Ti
— a report from an earlier session rather than something this file's author
watched; PLAYING.md marks it as such where it matters.

What is *not* proven is tracked, with its evidence and with what would retire
it, in [PLAYING.md](PLAYING.md)'s "Not proven yet" section. The largest item:
the personal-best and ghost loop has been closed once on the real GPU, but its
evidence — a screenshot, and a `.s3d` that will still load after straf3's own
movement constants land — is not committed. That list is maintained as a claim
about this tree rather than as a roadmap, so it is the honest answer to "what
state is this in".

Two conventions worth stating because they are easy to break by accident:

- Dependency versions in the root `[workspace.dependencies]` table name the
  exact version that resolved. They are ordinary caret requirements, not `=`
  pins; reproducibility comes from the committed `Cargo.lock`.
- **No literal checksum or state digest appears in this file, deliberately.**
  A state checksum changes whenever a field is added to the simulation state —
  legitimately, and by design — so a number pasted into prose is a claim with an
  expiry date nobody can see. Numbers belong next to the command that produces
  them, labelled with the build that produced them; PLAYING.md does that and
  explains which of its digests are stable and which are not. The same reasoning
  retires line-number citations in favour of symbol names.

Note that `rust-version = "1.87"` is load-bearing rather than decorative:
cargo's MSRV-aware resolver silently caps `wgpu` at 26.0.1 if it says 1.85.

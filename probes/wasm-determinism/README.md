# wasm determinism probe

> **Superseded by the fix it designed.** Everything below measures the tree as
> it was *before* contract item C1 landed, when `angle_vectors` still called
> `f32::sin_cos`. It now calls `num::sin_cos`, so this probe's `build.rs` has
> no call sites left to redirect: it detects that, emits a `cargo:warning`,
> and applies no patch — which makes its `patched_sim` module an exact copy of
> the real sim and every patched-vs-stock figure it reports a tautology. The
> crate still builds and its harness (including `web/`) still works; the
> results in this file are not reproducible against the current tree. For the
> post-C1 numbers, see `probes/c1-owned-trig/`.

Answers spec rev 5 §L: **does `straf3-sim` produce bit-identical
`SimState::checksum()` results in a browser (wasm32) and on a native x86-64
verification server?**

Answer: **no, not as things stand today** — and the divergence is one
operation, `sin`/`cos`, disagreeing by exactly 1 ULP.

This crate is standalone (empty `[workspace]` table in its `Cargo.toml`) so it
is excluded from the root workspace and edits nothing under `crates/`. It
depends on `straf3-sim` by relative path, unmodified.

## Result

| Build | grand checksum over all 6 cases × 1200 commands |
|---|---|
| native x86-64, **glibc** | `a7456f3fe4f4bfbd` |
| native x86-64, **musl** | `8264efaea027161e` |
| wasm32-unknown-unknown, **Node/V8** | `8264efaea027161e` |
| wasm32-unknown-unknown, **headless Chrome 146** | `8264efaea027161e` |

So the split is not native-versus-browser. It is **glibc versus everyone
else**: Chrome, Node and a musl native build agree with each other bit for bit
in every single measurement the probe takes (all 6 cases, all 1200 per-command
checksums, all 977 probe angles, f32 and f64, the NaN payloads). Rust's
`wasm32-unknown-unknown` target has no libm to link against, so it compiles in
the `libm` crate — a port of musl's — which is why those three match.

## What diverges, exactly

`f32::sin_cos` in `angle_vectors` (`crates/straf3-sim/src/step.rs:955-957`),
by **1 ULP**, on ~1–3 % of angles. Nothing else:

| operation | native glibc vs Chrome |
|---|---|
| `f32 sin`, `f32 cos`, `f32 sin_cos` | **differ**, max 1 ULP, 12/977 and 4/977 probe angles |
| `f64 sin`, `f64 cos` | differ, 1 ULP, 36/977 and 33/977 |
| `f32 sqrt`, `f64 sqrt` | identical everywhere (IEEE-754 requires it) |
| `deg * DEG_TO_RAD` (multiply) | identical |
| `VectorNormalize` shape (`x / sqrt(x²+y²+z²)`) | identical |
| `max`/`min`/`clamp`/`abs` on ±0.0 and NaN, and NaN bit payloads | identical (16/16) |

The sim's entire floating-point surface is `abs`, `sqrt`, `sin_cos`, `max` and
`clamp` — of those, only `sin_cos` is not IEEE-754-specified, and only
`sin_cos` diverges. There is no evidence of fast-math, FMA contraction or x87
excess precision.

## What it costs in the simulation

Divergence starts late and stays small on flat ground, but it does not heal:

| case | first differing command | commands differing | final drift |
|---|---|---|---|
| `cpm-strafe-turn` | #677 (t = 5416 ms) | 57/1200 | velocity.x by 3.05e-5 u/s |
| `vq3-strafe-turn` | #393 (t = 3144 ms) | 383/1200 | origin.x by 3.05e-5 u |
| `cpm-yaw-0` (yaw pinned to 0°) | never | 0 | — |
| `cpm-yaw-90` (yaw pinned to 90°) | never | 0 | — |
| `cpm-still` (no input) | never | 0 | — |
| `cpm-fine-turn` (sub-degree turning) | #635 | 29/1200 | final checksum happens to re-converge |

`cpm-fine-turn` is the case worth remembering: its **final** checksum matches
while 29 intermediate commands did not. Comparing only end-state checksums
would have called that run identical. Any verification protocol should compare
a checksum per command (or a rolling digest), not just the finish line.

The absolute drift here is tiny because a flat plane has no discrete branches
to flip. On real geometry a 1-ULP difference decides *landed vs still falling*,
*clipped against one plane vs two*, *step-up taken vs not* — and those are
macroscopic. Do not read 3e-5 units as the worst case.

## Does shipping our own trig close the gap?

**Yes — demonstrated end to end, not argued.** `src/dettrig.rs` implements
`sin`/`cos`/`sin_cos` with Cody–Waite range reduction plus Taylor polynomials,
using only `+ - * /` — all IEEE-754-specified, all single wasm instructions, no
contraction.

`build.rs` then makes a build-time copy of `straf3-sim`'s sources in `OUT_DIR`
with the three `sin_cos` calls in `angle_vectors` redirected to it — three
lines, nothing else, and nothing under `crates/` is modified — and the probe
runs the same six cases through that patched sim as well:

| build | stock grand | patched grand |
|---|---|---|
| native glibc | `a7456f3fe4f4bfbd` | **`9353a522e0a60466`** |
| native musl | `8264efaea027161e` | **`9353a522e0a60466`** |
| wasm, Node | `8264efaea027161e` | **`9353a522e0a60466`** |
| wasm, Chrome | `8264efaea027161e` | **`9353a522e0a60466`** |

With the trig owned, all four agree — including per-command checksums, not
just the finish line. Note the patched digest differs from both stock digests:
the fix changes results by a last bit here and there, so **any checksum
recorded before the swap is invalidated by it.** Do it before reference
replays or leaderboard runs exist, not after.

Over a 200 000-angle sweep from −720° to +720°:

- `libm` digest: `b71c58a6cda8908b` (glibc) vs `e935a7fa1bbd2d37` (Chrome) — **differ**
- own-trig digest: `ed430df756584863` on **both** — identical

It is also within **1 ULP of libm on both platforms** (disagreeing on 3.19 % of
sweep angles under glibc, 2.00 % under wasm), so substituting it changes the
physics by less than the platforms already differ by. A Q3-style lookup table
is equally deterministic — the determinism comes from not calling `libm`, not
from the table; the table is just the 1999 way of being fast at it.

The implementation here is a probe, not a proposal: if this is adopted it
wants accuracy tests against a reference and a decision on where it lives
(the `num` seam is the natural home).

## Running it

```sh
./run-all.sh          # builds, runs native/musl/node/chrome, writes results/
```

`results/digests.json` and the `compare-*.txt` files are kept in git; the full
~300 KB reports are regenerable and are gitignored.

The browser run is the one that counts: `web/index.html` instantiates the same
`det_probe.wasm` with `WebAssembly.instantiateStreaming` and no wasm-bindgen,
served over loopback by `web/serve.mjs`, driven by headless Chrome with
`--dump-dom`. The module imports **nothing** (`results/node-imports.txt`
confirms `imports: []`), which is why V8 in Node and V8 in Chrome cannot
differ: the trig is inside the wasm, not borrowed from the JS engine.

## Consequences for the leaderboard (spec §L, D1/D2)

1. Server-side replay verification is viable, but not by accident — the
   verification server must be pinned to the same libm as the browser. Today a
   **musl** server (Alpine, or `x86_64-unknown-linux-musl`) already matches
   Chrome bit for bit; a stock glibc server does not.
2. That match is a coincidence of the Rust `libm` crate tracking musl. It is
   not a contract, and it can break on a toolchain bump — which the repo
   already treats as a determinism-relevant event (`rust-toolchain.toml`).
3. The durable fix is to own the trig: three call sites in `angle_vectors`, and
   the `num` seam is already the place to put it.

# C1 verification — the landed `num::sin_cos`

Verifies contract item C1 (`docs/web/ARCHITECTURE.md` §1.2, §2/C1) *after* it
landed, which is a different question from the ones the two probes next door
answered before it did.

`probes/wasm-determinism/` asked "is the simulation bit-identical across
targets?" (no — `f32::sin_cos`) and "would owning our trig fix it?" (yes).
`probes/dettrig-accuracy/` asked "is that owned trig actually accurate?"
(within 1 ULP of both libms below 8192°, worse above). Both measured code that
lived under `probes/`. This one asks the only question left: **is the function
that shipped in `crates/straf3-sim/src/num.rs` the same function they
measured** — because if it is not, both of those reports describe something
that is not in the tree.

## Results

Ran on the pinned toolchain (1.97.1), 8 threads.

### 1. The landed code is the reference, bit for bit

Exhaustive **bit-pattern** enumeration of every representable `f32` degree
magnitude in `|deg| ∈ [2^-10, 2^24]`, both signs — **570,425,346 samples** —
comparing `straf3_sim::num::sin_cos` against
`probes/wasm-determinism/src/dettrig.rs::det_sin_cos`, the file included here
verbatim by `#[path]` rather than copied:

| | samples differing in any bit |
|---|---:|
| glibc build | **0** |
| musl build | **0** |

So `probes/dettrig-accuracy/`'s 570M-sample measurement against its
double-double reference transfers to the landed code unchanged, and this probe
does not redo it. Its headline figures, restated for convenience: max 1 ULP
(sin and cos) below 8192°, degrading above — 12 ULP by 8192° for cos and by
16384° for sin, 1131/1185 ULP at the `f32` integer-exactness ceiling.

### 2. Against the host libm

| | worst ULP, whole swept domain | worst ULP, \|deg\| ≤ 8192 |
|---|---:|---:|
| vs glibc `sinf`/`cosf` | sin 1131, cos 1185 | **sin 1, cos 1** |
| vs musl `sinf`/`cosf` | sin 1131, cos 1185 | **sin 1, cos 1** |

Spec acceptance criterion 1 ("within 1 ULP of libm across a dense angle
sweep") holds across the domain a view angle can reach and does not hold
above it. That is the pre-existing, measured, documented property of
Cody-Waite reduction carried out in plain `f64` — not a regression introduced
by landing it — and it is an *accuracy* limit, not a determinism one: the two
rows above are identical because the function does not call libm at all.
`crates/straf3-sim/src/num.rs` says the same thing in its own docs, and C3
(16-bit view angles) makes the degraded region unreachable by construction.

### 3. The same bits on every target

A digest over all 570,425,346 outputs, folded commutatively so it does not
depend on thread count:

| target | digest |
|---|---|
| `x86_64-unknown-linux-gnu` | `0x20064597458820de` |
| `x86_64-unknown-linux-musl` | `0x20064597458820de` |

### 4. …and the whole simulation, not just the function

The narrow digest above covers `sin_cos` alone. `probes/wasm-determinism`'s
harness runs 6 movement cases × 1,200 commands through the real
`straf3-sim` and checksums **every command**, which is the comparison §1.3
argues for. Re-run against this tree (`results/sim-*.txt`):

| pair | grand checksum | per-command |
|---|---|---|
| native glibc | `9353a522e0a60466` | — |
| native musl | `9353a522e0a60466` | identical, all 6 cases × 1,200 |
| wasm32 under Node/V8 12.4 | `9353a522e0a60466` | identical, all 6 cases × 1,200 |

Before C1, those three were `a7456f3fe4f4bfbd` / `8264efaea027161e` /
`8264efaea027161e` — glibc against everyone else. Note that the value they now
agree on is a *third* number: own-trig is not "glibc adopting musl's answers",
it is its own answer, which is the point of owning it.

**The comparison is not vacuous, and the same reports prove it**: in the very
same run, the probe's direct `f32::sin_cos` measurements still diverge exactly
as they always did — 12/977 probe angles for sin, 4/977 for cos, 1 ULP each,
glibc versus the other two. The libms still disagree. The simulation no longer
asks them.

Two targets in the spec's list are **not** covered here:
`x86_64-pc-windows-gnu` (builds, but there is no Wine or Windows host in this
sandbox to run it) and wasm in real Chrome (no browser here — Node/V8 is the
same engine family and the earlier probe measured Node and Chrome 146
bit-identical in every one of its measurements, but that is inference, not a
Chrome run). Closing both is C2's job.

## Running it

```sh
cargo run --release                                     # ~4 s on 8 threads
cargo run --release -- --quick                          # strided, instant
cargo run --release --target x86_64-unknown-linux-musl  # the other side
```

`results/glibc.txt` and `results/musl.txt` are those two runs.
`results/sim-glibc-vs-musl.txt` and `results/sim-glibc-vs-wasm-node.txt` are
`probes/wasm-determinism/web/compare.mjs` over reports regenerated from this
tree; reproduce them with that probe's `run-all.sh`, which also covers Chrome
if one is installed.

## A note on the neighbouring probe

`probes/wasm-determinism/build.rs` patched a build-time copy of `step.rs` to
redirect its three `.sin_cos()` call sites, and asserted it found exactly
three. C1 removed them, so that assertion began failing and the crate stopped
building. It now detects the landed state and emits a `cargo:warning` instead:
with no patch to apply, its `patched_sim` module is an exact copy of the real
sim, so its patched-vs-stock comparison is a tautology rather than a
measurement — visible in `results/sim-*.txt`, where every "patched" line reads
`0/1200 commands`. Its README's headline results describe the pre-C1 tree and
are not reproducible against this one.

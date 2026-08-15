# dettrig accuracy probe

Answers spec rev 6 §Q1's precondition: own-trig ("Cody–Waite reduction plus
Taylor polynomials," `probes/wasm-determinism/src/dettrig.rs`) has so far only
been characterised as **"within 1 ULP of the two libms."** That is circular —
one of those two libms is exactly what it would replace — so it says nothing
about whether own-trig is actually *accurate*, only that it agrees with
platforms that might themselves be wrong together. This probe breaks the
circularity: it builds an independent ground truth and measures own-trig,
glibc's `sinf`/`cosf`, and musl's `sinf`/`cosf` all against *that*, over an
exhaustive sweep, not the ±720° sample the original probe used.

**Headline result: own-trig is not uniformly at least as accurate as libm.**
It matches both libms (0–1 ULP) for `|degrees| < 8192` on *both* `sin` and
`cos`, comfortably covering any plausible view angle in normal play. **The
safe threshold is 8192, not 16384** — `own_sin` only starts degrading at the
16,384° octave, but `own_cos` degrades one octave earlier, at 8192° (12 ULP
in `[8192, 16384)`, exhaustively confirmed — see the per-function table
below), so a single combined threshold has to use the tighter of the two.
Beyond 8192° it degrades — 12 ULP by ~8,192° (cos) / ~16,384° (sin), 41 ULP by
~131,000°, up to **1131 ULP (sin) / 1185 ULP (cos)** by a few million degrees
— while **both glibc's and musl's `sinf`/`cosf` stay within 1 ULP of ground
truth across the entire swept domain**, right up to `f32`'s integer-exactness
ceiling. Whether this matters depends on whether `ViewAngles::yaw` can
actually accumulate that large in practice — see "Does this matter for the
game" below, which also resolves whether yaw is wrapped anywhere before it
reaches `angle_vectors` (it is not, currently).

## What exactly was measured

- **own-trig**: `det_sin`, `det_cos`, `det_sin_cos` in
  `probes/wasm-determinism/src/dettrig.rs`, called **directly from that
  crate** as a path dependency on its `det_probe` rlib target (see this
  crate's `Cargo.toml`) — not copied, not retyped, not reimplemented. There is
  no code in this probe that could have drifted from what the determinism
  probe actually measured.
- **glibc's libm** and **musl's libm**: plain `f32::sin_cos`/`sin`/`cos`,
  compiled and run natively for `x86_64-unknown-linux-gnu` (dynamically links
  glibc) and `x86_64-unknown-linux-musl` (statically links musl) respectively
  — the same two libms the determinism probe found diverging by 1 ULP from
  each other, and (per that probe's README) musl-native is bit-identical to
  what `wasm32-unknown-unknown` actually links (the Rust `libm` crate, a musl
  port) and to real Chrome 146. Running musl natively stands in for "the
  browser" without needing a browser — the determinism probe already proved
  that substitution is exact, not approximate.
- **The exact argument**: `rad = deg * DEG_TO_RAD` computed in `f32`, where
  `DEG_TO_RAD` is reproduced byte-for-byte from
  `crates/straf3-sim/src/step.rs:86` (`src/main.rs` documents this — the
  constant is private in `straf3-sim` so it can't be imported, only
  reproduced as the identical literal expression, which the compiler folds to
  the identical `f32` bits). This is what `angle_vectors`
  (`crates/straf3-sim/src/step.rs:954-957`) actually feeds `sin_cos` — not an
  idealised degree value, not a radian value chosen for convenience.

`crates/` was read (`step.rs`, `cmd.rs`) but never edited. Everything here
lives under `probes/dettrig-accuracy/`.

## The reference: a from-scratch double-double `sin`/`cos`

`src/ddfloat.rs` implements double-double (`f64`+`f64`, ~106-bit
significand, Dekker/Knuth/Bailey-style) arithmetic and a `sin`/`cos` built on
it, using only `+ - *` on `f64` — no `libm`, no external crate, no shared code
with `dettrig.rs` beyond the same textbook quadrant identity every Cody-Waite
implementation uses. Full reasoning for *why* double-double and not an
external arbitrary-precision crate is in the module's doc comment; the short
version: double-double gives ~32 decimal digits, `f32` only needs ~7.2
resolved, so this reference's own rounding error sits about **2^82** below an
`f32` ULP — a wider margin than an external MPFR-style crate would buy, at the
cost of a new dependency and (for `rug`/`gmp-mpfr-sys`) a C build step whose
availability in this sandbox was never verified.

Nothing here is a hardcoded double-double constant taken on faith: π is
parsed from its well-known decimal digits by a from-scratch digit-by-digit
parser (`dd_from_decimal`), and the `self_check`/`dd_arithmetic_sanity` unit
tests (`cargo test --release`) check the result transitively —
`reduce(π)` and `reduce(π/2)` land within `1e-29` of the exact quadrant
identities, `sin²+cos²==1` to `1e-30` at a non-special angle, `π`'s high limb
is bit-identical to `std::f64::consts::PI` — entirely independent of how π
was constructed, so a mistyped digit or a broken parser fails loudly before
any sweep runs. `Dd -> f32` conversion (`dd_to_f32`) is a careful
correctly-rounded conversion, not a naive `as f32` cast, specifically to
avoid double-rounding at exactly the ULP boundaries this probe exists to
check — it resolves the nearest-`f32` decision (including exact ties, round
to even) in full `Dd` precision via `Dd::sub` against the midpoint, not by
collapsing `d.hi + d.lo` into one `f64` first, which is regression-tested
(`dd_to_f32_resolves_far_subnormal_lo_across_exact_midpoint`) with a `Dd`
sitting exactly on an `f32` midpoint and nudged only by a `lo` far below
`f64`'s own resolution at that scale.

One known, harmless gap: `sin_cos_dd(-0.0)` returns `(+0.0, 1.0)`, not
IEEE-754's `(-0.0, 1.0)` — documented on `sin_cos_dd` itself. It doesn't
affect any figure in this report: `-0.0` is outside the exhaustive sweep's
domain, `spot_check_tiny` (the only caller that evaluates it) only checks
magnitude, and both libms get the sign right regardless.

Argument reduction reuses the ordinary `f64` division to pick the integer
quadrant `q` (not a double-double 2/π constant) — this is deliberate and
argued in the module docs, not a shortcut: `q` only has to be the *correct
integer* nearest `x/(π/2)`, which plain `f64` resolves correctly for every
magnitude this probe sweeps, and even at an exact tie, both neighbouring
integers give a valid decomposition `x = q·(π/2) + r` with `|r| ≤ π/4` — the
reference's accuracy comes from the double-double-precision `π/2` constant
used to compute `r`, not from `q`'s precision.

## Domain swept

Exhaustive **bit-pattern** enumeration (every representable `f32`, not
samples) of `|degrees| ∈ [2^-10, 2^24]`, both signs: **570,429,352 values**,
took ~2 minutes per libm target on 8 threads. Two boundary choices, both
argued in `src/main.rs`'s doc comments, not just asserted:

- **Lower bound 2^-10° (~0.001°)**: below this, `sin(x) == x` to full `f32`
  precision for *any* competent implementation (Taylor remainder is under
  `f32`'s last bit), so exhaustively sweeping the ~2-billion-per-sign
  subnormal/near-zero tail below it would spend the overwhelming majority of
  the sweep's time on provably-trivial cases. That region gets a
  **non-exhaustive spot-check** instead (`spot_check_tiny`: both signed
  zeros, smallest subnormal, `f32::MIN_POSITIVE`, 2000 log-spaced samples
  down to the smallest subnormal) — no anomaly found, own-trig and both
  libms agree with the reference to 0 ULP everywhere checked there. This is
  a real scope decision, stated so it isn't mistaken for full coverage.
- **Upper bound 2^24° (~16.78M°, ~46,603 turns)**: `f32`'s well-known
  integer-exactness ceiling. Above it, adjacent `f32` values are more than 1
  apart, so a single `f32` degree value stops identifying one specific angle
  — every implementation, including a perfect one, would be answering "sin of
  some real number near this float," and disagreements stop being about
  `sin_cos` accuracy at all. This is what makes the sweep an accuracy
  measurement of `sin_cos` rather than a demonstration of an unrelated,
  already-known `f32` limitation.

## Results

All figures from `results/glibc.json` and `results/musl.json` (raw sweep
output, kept in git — each ~34 KB, includes the full per-octave breakdown).
Both runs are 570,429,352 samples each; own-trig's numbers are, correctly,
bit-identical between the two runs (it doesn't call libm at all), which is
itself a useful cross-check that the measurement is reproducible and not an
artifact of one build.

| | max ULP (sin) | max ULP (cos) | samples with ULP ≥ 1 (sin / cos) | samples with ULP ≥ 2 (sin / cos) |
|---|---:|---:|---:|---:|
| **own-trig** | **1131** | **1185** | 6,817,312 / 6,794,428 (1.195% / 1.191%) | 1,704 / 1,704 (0.0003%) |
| **glibc `sinf`/`cosf`** | 1 | 1 | 5,385,828 / 4,384,480 | 0 / 0 |
| **musl `sinf`/`cosf`** | 1 | 1 | (see `results/musl.json`) | 0 / 0 |

own-trig is strictly worse than the platform's own libm (sin) on
**6,535,944 samples (1.146%, glibc run) / 6,795,122 (1.191%, musl run)** of
the 570M swept, and on a near-identical fraction for cos (6,512,188 / 1.142%
glibc; 6,772,998 / 1.187% musl; full counts in `results/*.json`) — never the
other way around at ULP ≥ 2 (own-trig can be at most 1 ULP *better* than
either libm, since both libms themselves never exceed 1 ULP anywhere in this
sweep; every ULP-≥-2 disagreement in the dataset is own-trig alone).

### Error vs magnitude — where it actually comes from

The single global "max ULP" figure above hides the real shape of the result,
which is the useful part. Per-octave worst-case ULP, **sin and cos given
separately since their cliffs are one octave apart** (full table in
`results/*.json`, `octaves_sin`/`octaves_cos`):

Each cell is the worst ULP seen **anywhere from 0° up to that row** (running
max across all finer octaves below it, not just the single octave starting
there — cos in particular is not monotonic octave-to-octave, e.g. its raw
per-octave max at exactly 16,384° is only 2, *lower* than the 12 it already
hit at 8,192°, so a non-cumulative table would misleadingly suggest recovery):

| `\|degrees\|` up to | own-trig max ULP (sin) | own-trig max ULP (cos) | glibc max ULP (both) |
|---:|---:|---:|---:|
| 4,096 | 1 | 1 | 1 |
| **8,192** | **1** | **12** | 1 |
| 16,384 | 12 | 12 | 1 |
| 65,536 | 12 | 41 | 1 |
| 131,072 | 41 | 41 | 1 |
| 2,097,152 | 41 | 1131 | 1 |
| 4,194,304 | 1131 | 1131 | 1 |
| 8,388,608 | 1131 | **1185** | 1 |
| 16,777,216 (ceiling) | 1131 | 1185 | 1 |

The first crossing is sharp and lands on an exact octave boundary, but not
the *same* boundary for both functions: `own_sin` max ULP is **1 for every
degree magnitude below 16,384°**, first exceeding 1 (reaching 12) in the
`[16,384, 32,768)` octave. `own_cos` breaks one octave earlier — max ULP is
**1 for every degree magnitude below 8,192°**, first exceeding 1 (reaching
12) in the `[8,192, 16,384)` octave (single exhaustively-confirmed
counterexample there: `deg = 14490`, `own_cos` 12 ULP against 0 ULP for both
libms). Past that first crossing, per-octave error is **not** monotonic for
either function (`own_cos`'s raw per-octave max actually drops back to 2 in
`[16,384, 32,768)` before climbing again later — see `results/*.json`
`octaves_cos` for the un-smoothed, non-cumulative numbers), which is exactly
why the table above reports a running max rather than the raw per-octave
figure: a non-cumulative reading of "12 ULP at 8,192°, only 2 ULP at 16,384°"
would wrongly suggest own-trig recovers accuracy as `|x|` grows, when what's
actually happening is *which* octaves get unlucky is scattered, not that the
degraded region shrinks. This is not sampling noise — every one of the
octaves quoted here is exhaustively swept (16,777,216 samples each).
**Anything quoting one safe threshold for "own-trig" without saying which
function is being asked to account for both, and should use 8192.**

This is the expected signature of `dettrig::reduce`'s specific technique:
Cody–Waite range reduction with a two-limb `PIO2_HI`/`PIO2_LO` split
*computed in plain `f64` arithmetic* (see `dettrig.rs`'s `reduce`, and its
own doc comment, which never claimed unbounded-magnitude accuracy — only
determinism). The two-limb split buys roughly 106 bits of *stored precision*
in the constant, but the reduction's own subtraction
`(x - qf * PIO2_HI) - qf * PIO2_LO` is carried out in ordinary `f64`, so the
achievable cancellation is bounded by `f64`'s 53 bits regardless of how
precise the constant is — and the residual error scales with `q` (hence with
`|x|`), which is exactly the roughly-linear-with-octave growth measured
above. This is a well-known, textbook limitation of single-`f64` Cody-Waite
reduction, not a coding mistake; the fix (reduce in double-double, the same
technique this probe's own reference uses) is a known pattern, not a research
problem — but implementing it is out of this probe's scope (it lives in
`probes/dettrig-accuracy/`, not `crates/`, and this assignment is measurement,
not remediation).

### Does it show up inside plausible gameplay range too?

Yes, rarely, well before the 8,192° cliff. The smallest-magnitude example in
the sweep where own-trig is strictly worse than glibc:

```
deg = 23.854576°, rad = 0.4163409, own sin ULP = 1, glibc sin ULP = 0
```

i.e. even at an ordinary view angle, own-trig is occasionally (not
systematically — `own_sin`'s octave-140 max is still only 1 ULP, same as
glibc's) one bit worse than a libm that happens to be exactly correctly
rounded there. This is a minor, expected cost of a 9-term polynomial
approximation versus a hand-tuned libm and is not evidence of a bug — 1 ULP
disagreement between *any* two competent `sin` implementations is normal.
It's reported because "be explicit about the numbers you did not want"
applies here too, not just to the large-magnitude findings.

### Does this matter for the game

Resolved (read-only, `crates/straf3-sim` untouched): **`ViewAngles::yaw` is
not wrapped anywhere.** `step.rs:274` passes `cmd.view.yaw` straight into
`angle_vectors`; there is no `wrap`/`angle_mod`/`rem_euclid`/`%360`/
normalisation anywhere in `cmd.rs` or `step.rs` (the only `clamp` in that
path is unrelated, on `wishspeed`), and `ViewAngles`'s own doc comment
(`cmd.rs:141-151`) describes unbounded absolute angles, justified by
absolute-over-delta being needed for replay fidelity. So whether own-trig's
degradation past 8,192° is reachable is entirely a property of the Wave 3
input layer, which does not exist yet — nothing in the current simulation
bounds it. Unbounded mouse-look accumulation over a long session
(competitive players circle-strafe continuously; 8,192° is only ~22.8 full
turns) would eventually cross into the degraded region, and it would do so
*silently* — nothing about the checksums or determinism guarantees would flag
it, since own-trig stays internally consistent (bit-identical across
platforms) even while it drifts from ground truth. This is the "reopens the
decision" scenario the assignment asked to surface if found, not resolve.

Worth noting for Wave 3's input layer: vanilla Q3 bounds this implicitly,
because `usercmd_t` angles are 16-bit packed (`ANGLE2SHORT`/`SHORT2ANGLE`),
so a Q3 angle is inherently wrapped into one turn by the wire format itself.
straf3 uses `f32` absolutes with no such encoding, so it has no equivalent
implicit bound. Wrapping yaw into `[0, 360)` at the `UserCmd` seam would make
this finding unreachable by construction, mirror Q3's own behaviour, and
cost nothing — a cheap mitigation if Wave 3's input layer wants to close this
off rather than merely note it.

## What was not measured

- **`f64` accuracy.** `angle_vectors` only ever calls `f32::sin_cos`;
  `dettrig::det_sin`/`det_cos` also only expose `f32` entry points, so `f64`
  was out of scope by construction, not by oversight.
- **Whether reduction failure at large `|x|` ever flips a discrete branch in
  the actual simulation** (landed-vs-falling, one-plane-vs-two clip, etc.) —
  the wasm-determinism probe raised this concern for the 1-ULP glibc/musl gap
  and never measured it either; this probe doesn't either. It would require
  running the sim, which lives in `crates/`, out of this probe's scope.
- **Whether `yaw`/`pitch`/`roll` will actually reach the magnitudes where
  own-trig degrades in a real play session.** The previous section resolves
  the part this probe *can* answer read-only (the current simulation applies
  no bound), but reachability in practice is a property of a Wave 3 input
  layer that doesn't exist yet, and of session length/player behaviour —
  neither is measurable from `probes/`.

## Running it

```sh
cargo test --release                                    # 3 unit tests on ddfloat.rs
cargo build --release && ./target/release/sweep          # glibc, ~2 min on 8 threads
cargo +1.97.1 build --release --target x86_64-unknown-linux-musl \
  && ./target/x86_64-unknown-linux-musl/release/sweep    # musl, ~2 min
./target/release/sweep --bench 20000000                  # throughput check, no full sweep
```

`results/glibc.json` and `results/musl.json` are the two runs this report is
based on, kept in git (~34 KB each) so the figures above are reproducible
without a 4-minute rebuild.

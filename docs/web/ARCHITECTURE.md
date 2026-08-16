# straf3 on the web — architecture, formats, and the contract the game must expose

Status: **design, for approval.** No application code is proposed here and none was written.
Governing specification: rev 6. Written 2026-08-15, revised against rev 6.

Spec rev 5 §M settled five decisions and they are treated as fixed throughout:

| | Decision | Where it shows up below |
|---|---|---|
| D1 | Server-side replay verification | §1, §7, §8 |
| D2 | Rust/axum backend linking `straf3-sim` directly | §7, §10 |
| D3 | OAuth only — Discord, then GitHub. No passwords | §6 |
| D4 | A playable URL with no accounts, before any backend | §9 |
| D5 | Monorepo, new top-level `web/` | §9 |

Rev 5 §L named one question as governing everything: *is `straf3-sim` bit-identical between browser
wasm and native?* It said the answer was cheap and that the backend should not be designed until it
was known.

**It is now known**, and answered twice independently: by probe session
`as_8c4119ef80d54651b279` (six builds × 1,200 commands, a 977-angle sweep, f32 and f64, NaN
payloads, headless Chrome included) and by me, separately, while designing against it. The two
investigations reached the same call site by different routes. §1 is the combined result, and it
marks clearly which claims are mine and which are the probe's. It sets the shape of the contract in
§2, which is the section to act on first.

**Rev 6 closed the other half of that probe too**, and this revision of the document is written
against the answer rather than against the question. Browser play is viable at **132 KiB** of
gzipped wasm, and rev 6 §Q2 decided the bundle shape: **WebGPU only, no combined fallback bundle.**
That decision is fixed and C9 states it rather than reopening it. Rev 6 §Q1 also confirmed C1 — we
own the trigonometry, we do not pin the server to musl — and §R made the rolling digest a recorded
protocol constraint, which §3.2 already implements.

---

## 0. Summary — the eight things to act on

1. **`straf3-sim` is *not* currently bit-identical across builds — but the fault line is not the one
   rev 5 §L expected.** It is not native-versus-browser. Native musl, wasm-in-Node and
   wasm-in-headless-Chrome are bit-identical on every measurement the probe took; **stock glibc is
   the sole outlier.** wasm has no system libm, so Rust statically links the `libm` crate into the
   module — the module imports no trigonometry at all, which is why no browser engine *can* differ
   from Node. On my own measurement, glibc-native and wasm-in-V8 agree for 1,790 commands of the
   crate's own determinism stream and disagree at command 1,791 — **14.3 seconds into a run.**

2. **It isolates to exactly one call-site family.** The three `f32::sin_cos` calls in `angle_vectors`
   (`crates/straf3-sim/src/step.rs:955-957`). The probe measured 1 ULP on 12/977 sin and 4/977 cos
   angles; I measured 1.29% of angles over a denser sweep. Everything else in the simulation's float
   surface — `sqrt`, `abs`, `min`/`max`/`clamp`, `VectorNormalize` — is identical across all builds,
   with no FMA contraction and no x87 excess precision.

3. **The fix is decided and already verified by the probe: `straf3-sim` owns its trigonometry.**
   Q3-style Cody-Waite argument reduction plus Taylor polynomials, using only `+ − × ÷`, all
   IEEE-specified and therefore identical on every conforming target. The probe built it and
   confirmed all four builds produce identical per-command *and* final checksums, within 1 ULP of
   libm. That is contract item **C1**; it lands after Wave 2 closes, and every other part of this
   document assumes it.

4. **End-state checksums are provably insufficient, and this constrains the format.** The probe found
   a case where the *final* checksum matched across builds while **29 of its 1,200 intermediate
   per-command checksums did not**. A verifier comparing only end state would have certified a run
   that had genuinely diverged and happened to reconverge. The `.s3d` format therefore carries a
   **rolling digest folded over every command** (§3.2), not a periodic sample and not an end-state
   value.

5. **Nothing starts or stops the run clock.** `RunState::start` and `RunState::finish` exist and are
   called from nowhere in the workspace. The product is a *time*, and no code currently produces
   one. That is contract item **C4**, and it is a bigger gap than the format work. C4 also settles
   *how* triggers are tested: they ride on `Trace` rather than on a second `World` method, because a
   single command issues up to a dozen sweeps along a bent path and any coarser granularity misses
   volumes the player really went through. The opposite error is just as live: several of those
   sweeps cover ground the hull never commits to, so the tracer must gather triggers over the
   traversed prefix rather than the queried segment, and `step_slide_move`'s discarded first attempt
   must additionally roll the accumulator back — otherwise the platform credits triggers the player
   never touched.

6. **Storage is not a constraint and never becomes one.** Measured on a realistic input stream, a
   45-second run stores in **~17 KiB**. Ten thousand players' worth of personal bests is 4.1 GiB —
   under seven cents a month. The costs that matter are verification CPU and operational care around
   physics changes, not bytes.

7. **Browser play is small, and its bundle shape is decided.** A working WebGPU skeleton — canvas,
   surface, adapter, swapchain, render loop — is **132 KiB** of gzipped wasm and ran in Chrome 146.
   Rev 6 §Q2 fixed the shape: WebGPU only, no compiled-in WebGL2 fallback (it costs 595 KiB and
   `wgpu` was measured to crash rather than degrade when it fires), no `egui`. Stage D — the whole
   game — has since been measured at **131 KiB** of gzipped wasm, *smaller* than the skeleton
   (`probes/wasm-render/sizes.txt`). The `parry3d` weight §P2 feared never existed: nothing called
   it, the linker dropped it, and it is no longer a dependency at all.

8. **Verification cannot tell a copied run from an original, and the document says so.** A `.s3d`
   re-simulates identically whoever uploads it, and no client-side computation can bind identity
   when the client is the adversary. §8.3 answers with a globally unique *canonical* run digest —
   over the decoded command stream, not the file bytes, which are trivially perturbable — so the
   first submitter owns the run, plus an attempt ticket and near-duplicate detection. It reduces the
   attack; it does not close it, and it is disclosed rather than claimed away.

One thing this document deliberately does **not** do, following from item 1: it does not encode a
libc constraint into the deployment story. "Pin the server to Alpine/musl" would work today and would
be a trap — it makes the physics a property of the base image. C1 makes the sim bit-exact
everywhere, and the deployment target stays a free choice.

Recommendation on ordering, given that `straf3-game`, `straf3-render` and `straf3-platform` are all
stubs: **build the browser first, and treat it as the primary client, not a port.** §9 argues it.

---

## 1. The determinism question, answered

Two independent investigations. The authoritative one is probe session
`as_8c4119ef80d54651b279` — six builds × 1,200 commands, a 977-angle sweep across f32 and f64, NaN
payload behaviour, and headless Chrome 146 as well as Node; evidence at `probes/wasm-determinism/`
with `run-all.sh` reproducing it. Mine was narrower and ran in parallel, for the purpose of not
designing a format against an unknown. They agree on the call site and on the fix.

I mark below which is which, because they are not equally broad: **the probe established the shape of
the fault line; I established that it bites a real run at 14 seconds.** Where they overlap they do
not contradict.

### 1.0 The fault line is glibc, not the browser

The result that matters most is the one that reframes rev 5 §L's premise. From the probe:

> Native musl, wasm-in-Node and wasm-in-headless-Chrome-146 are bit-identical on **every**
> measurement. Only stock glibc native differs.

The mechanism is worth stating because it is what makes the browser trustworthy rather than merely
lucky: **`wasm32-unknown-unknown` has no system libm**, so Rust statically links the `libm` crate
(a musl port) into the module itself. The module imports no trigonometric function from its host.
A browser engine therefore has nothing to differ *about* — V8 cannot disagree with Node because
neither is supplying the maths. I confirmed the mechanism independently by reading the symbol out of
a compiled module (§1.1c).

Two consequences run through the rest of this document:

- The browser is not the risky platform. It is the *self-contained* one. §9.2's browser-first
  recommendation gets a second, independent reason from this.
- **Do not pin the server's libc.** Deploying on Alpine/musl would make the checksums match today
  and would quietly make the physics a property of the base image, discovered later by whoever
  changes it. C1 removes the dependence entirely; the deployment target then stays a free choice.

### 1.1 What was measured

Four experiments of mine, all reproducible from Appendix A, alongside the probe's.

**(a) `straf3-sim` compiles to `wasm32-unknown-unknown` with no changes.** Verified by building it.
The crate's one dependency is `glam`, it uses no `std::fs`/`net`/`time`, and the seam check's
existing rules already exclude everything that would have blocked it. This was the cheap half of the
question and the answer is clean.

**(b) Native and wasm diverge, and I found exactly where.** A probe crate runs the command stream
from `crates/straf3-sim/tests/determinism.rs` (`yaw = i*0.37 + 0.1`, pitch −3.3, alternating strafe,
jump every 97th command, `FlatGround`, CPM) and returns `SimState::checksum()`. Built for native
x86-64 and for wasm, the latter executed in Node 22's V8 — the same engine as Chrome:

| commands | native x86-64 (glibc) | wasm in V8 | |
|---|---|---|---|
| 500 | `0x60f9dc6ff5e07955` | `0x60f9dc6ff5e07955` | identical |
| 1,790 | *(bisected)* | *(bisected)* | identical |
| **1,791** | `0x2af318592c222e64` | `0x99ab21727c93d5c1` | **diverged** |
| 2,000 | `0x6b4a9f58ea5a3676` | `0x72edbf07921158de` | diverged |

Command 1,791 at 125 Hz is **14,328 ms** into the run.

**(c) The cause.** Comparing std's `f32::sin_cos` (glibc `sinf`/`cosf` on this host) against the
`libm` crate over 359,387 angles at 0.001° steps across a full turn:

```
sin bit-differences   4,629 / 359,387  (1.288%)
cos bit-differences   4,613 / 359,387  (1.284%)
worst gap             1 ulp
```

And the wasm artifact names its own implementation. Disassembling the strings of a trivial
`x.sin()` module built for `wasm32-unknown-unknown`:

```
_RNvNtNtNtCs6RiiD5tsSf6_17compiler_builtins4math9libm_math4sinf4sinf
```

So: **glibc-native calls glibc's `sinf`, everything else calls the statically-linked `libm`, and they
differ by 1 ulp on about one angle in 78.** `angle_vectors` is the only place in `straf3-sim` that
calls a transcendental function; every other operation is `+ − × ÷` and `sqrt`, all of which are
correctly rounded and uniquely defined by IEEE-754 on both SSE2 and wasm. The probe checked that
directly across its six builds and found `sqrt`, `abs`, `min`/`max`/`clamp` and `VectorNormalize`
identical everywhere, with no FMA contraction and no x87 excess precision — which is the part I
could only argue from the standards, and it is now measured.

My 1.29%-of-angles figure and the probe's 12/977 sines are the same phenomenon at different sweep
densities; neither is more correct, and both are 1 ULP at worst.

The 14-second delay before the divergence shows up in the checksum is worth understanding: most
1-ulp differences in a direction cosine are absorbed when multiplied by a small acceleration
increment and rounded back into an f32 velocity. They are *usually* harmless. But they are frequent,
the absorption is a coincidence, and once one survives it is permanent and grows.

**(d) The Windows target, and a finding outside the web scope.** Fingerprinting `sin_cos` over the
same dense grid on the two native targets:

```
x86_64-unknown-linux-gnu   0xad47a99ae6ff170f
x86_64-pc-windows-gnu      0xe3a6a63033672561
```

The Windows build the operator is meant to tune movement on does not agree with the Linux build CI
runs. Same source, same pinned rustc — a different C library, which is exactly §1.0's fault line
seen from a third angle. At the 2,000-command checkpoint the Windows build agreed with wasm and
Linux-glibc was the odd one out, consistent with glibc being the outlier; I did not run a dense
wasm-versus-Windows sweep, because C1 makes the question moot.

This matters here because a time set on the Windows client would not verify on a glibc server. It is
pre-existing, has nothing to do with the web work, and the determinism tests cannot see it because
rev 1 scoped determinism to *same binary, same machine*.

### 1.2 The fix: `straf3-sim` owns its trigonometry

**Decided, and verified by the probe, not by me.** The three `sin_cos` call sites get a Q3-style
implementation inside `straf3-sim`: Cody-Waite argument reduction plus Taylor polynomials, using
only `+ − × ÷`. Every operation in it is IEEE-specified and correctly rounded, so it is identical on
every conforming target by construction rather than by coincidence. The probe built it and measured
all four of its builds producing identical **per-command and final** checksums, with accuracy within
1 ULP of libm.

It is scheduled rather than speculative: it touches `straf3-sim`, which Wave 2 has open, so it lands
once Wave 2 closes. Design everything downstream on the premise that the simulation is bit-exact
everywhere.

**My corroborating experiment**, which tested the same diagnosis by a different route: I copied
`crates/straf3-sim` out of the tree (the repository was not modified), changed only the three
`sin_cos` calls to `libm::sinf` / `libm::cosf`, and re-ran my probe on three targets:

| commands | linux-gnu | windows-gnu | wasm in V8 |
|---|---|---|---|
| 2,000 | `0x72edbf07921158de` | `0x72edbf07921158de` | `0x72edbf07921158de` |
| 20,000 | `0x77be5746402c5b76` | — | `0x77be5746402c5b76` |
| 100,000 | `0x6e42cb94334b412a` | — | `0x6e42cb94334b412a` |
| **400,000** | `0x037ac3485af83b4b` | `0x037ac3485af83b4b` | `0x037ac3485af83b4b` |

400,000 commands is 53 minutes of continuous play at 125 Hz. Three targets, one number — which
confirms the diagnosis is complete: replacing *only* those three calls is sufficient, and nothing
else in the crate contributes.

That route is not the one being taken, and the reason to prefer the owned implementation over a
`libm` dependency is sound: `straf3-sim`'s manifest treats a second dependency as an architectural
change, Q3 shipped its own tables for substantially this reason, and an implementation the project
owns cannot be changed by someone else's patch release. Noted so the alternative is on record, not
to reopen it.

Worth carrying forward regardless of route: the fixed checksum at 2,000 commands (`0x72ed…`) is the
value **wasm was already producing**. Whichever implementation is adopted, the browser's current
answers are close to canonical, and the builds that have to move are the glibc-native ones.

### 1.3 End-state checksums are not sufficient — a protocol constraint

The probe's most consequential finding for the *format* rather than the physics:

> In one test case the FINAL checksum matched across builds while 29 of its 1,200 intermediate
> per-command checksums did not.

A divergence can appear and then reconverge. This is not surprising in retrospect — §1.1 explains why
most 1-ulp perturbations get absorbed by rounding — but it is fatal to the obvious design. **A
verifier comparing only end state would have certified a run that genuinely diverged**, and would
have reported "client and server agree" when they did not.

The consequence is binding on §3.2 and C5: the recording carries a **rolling digest folded over every
command's state**, not an end-state checksum and not a periodic sample. A periodic checkpoint trail
is kept *as well*, but only to localise a divergence for debugging — it cannot be the detector,
because a 1 Hz sample would have missed 29 transient divergences just as the end state did.

### 1.4 What this means for the fixed decisions

**D1 (server-side verification) survives, and is now on measured ground rather than hoped-for
ground.** It requires C1. Without C1 it is not merely risky, it is broken today, at 14 seconds.

**D2 (Rust backend linking `straf3-sim` directly) survives and is the right call.** With C1 the
native verifier and the browser client compute the same bits regardless of the host's libc, so there
is genuinely one implementation and no hidden deployment constraint. §10 keeps a designed-in
fallback that does not require reopening D2.

### 1.5 The render half of the probe, and what is still open

The rev 5 §N probe had two halves. **Both are now closed**, and the second one closed in favour of
the browser more decisively than the first.

Measured by the probe with `stat` and `gzip -9`, not estimated:

| Stage | gzipped wasm | + JS glue |
|---|---|---|
| **A — `wgpu` + `winit`, WebGPU only** | **132 KiB** | 11.7 KiB |
| B — + WebGL2 backend | 727 KiB | 17.7 KiB |
| C — + `egui` | 2.50 MiB | 22.0 KiB |

Stage A is not a hello-world: it is a canvas-bound window, surface, adapter, device, swapchain and
a render loop, and **it ran in real Chrome 146**. The reason it is that small is structural — on the
WebGPU-only path `wgpu` compiles in no `wgpu-core`, no `wgpu-hal` and no `naga`, because WGSL goes
straight to the browser. `wgpu`'s reputation for weight is almost entirely the WebGL2 translation
layer, and we are not shipping it (rev 6 §Q2; C9).

**Two consequences the probe measured that are easy to assume away:**

- **`wgpu` does not fall back for you.** With both backends compiled in and `requestAdapter()`
  returning null, it crashed inside the WebGPU backend rather than degrading to GL. Backend
  selection is a decision the *host page* makes in JavaScript before entering wasm. A "just compile
  both and let it sort itself out" bundle does not work and would cost 595 KiB per visitor if it did.
- **`egui-winit` 0.36.1 cannot compile for `wasm32`** — its `arboard` dependency has no wasm
  backend — so `crates/straf3-devtools` has no web build today. Dropping `egui` from the web build
  is both the largest available saving (−1.83 MiB) and the right call on its own terms: the devtools
  overlay is speedometer, graphs and trace telemetry, which is cheaper as DOM over the canvas.

**Stage D is no longer open.** It was the last number missing on the render side — the game crates
plus `gltf`, with `parry3d` expected to arrive transitively through `straf3-map`/`straf3-collision` —
and the first probe seat's worktree was destroyed before it could measure it. It has since been
measured: **131 369 B of gzipped wasm plus 14 262 B of gzipped JS**, against stage A's 134 829 +
11 728. The complete renderer, simulation and collision are *smaller* than the empty skeleton, which
bought a `log`/`console_log` facade that `straf3-render` does not
(`probes/wasm-render/sizes.txt`).

`parry3d` — expected to be the single largest unknown in the bundle — turned out to weigh nothing:
`straf3-collision` answers the trace by hand and never called it, so the linker dropped it whole. It
has since been removed from the workspace outright, so the figure above cannot drift back.

---

## 2. The contract

This is the section that constrains the game, so it is the part to act on soonest. Each item states
what must exist, why, what it costs, and what breaks without it.

Two rules run through all of it, both inherited:

- **No float seconds, anywhere, ever.** Not in the wire format, not in the database, not in a JSON
  response, not in a TypeScript type. Durations and times are `u32` milliseconds. The single audited
  `seconds_from_millis` conversion at `num.rs:89`, reached from `step.rs:156`, stays the only
  crossing point, and it stays inside `straf3-sim`.
- **Physics exists once.** Every format, schema and API below is designed so that no second
  implementation of Q3 movement can come into existence — including a JavaScript one written to
  "just show the time on the client".

### C1 — `straf3-sim` must own its trigonometry *(decided; blocks everything)*

**Status: decided and pre-verified.** The probe built and measured this; it lands after Wave 2
closes, because it touches a crate Wave 2 has open. It is listed here as a contract item because
everything else in this document assumes it, not because it needs deciding.

**What:** `num.rs` gains an owned `sin_cos`, and `angle_vectors` calls it instead of
`f32::sin_cos`:

```rust
/// Sine and cosine of an angle in radians.
///
/// Not `f32::sin_cos`. std's implementation is whatever libm the target links,
/// and those disagree: glibc differs from the statically-linked `libm` that
/// wasm and musl builds use, by 1 ulp on a small percentage of angles. That is
/// enough to make a browser recording unverifiable on a glibc server after
/// ~14 seconds of play.
///
/// Cody-Waite argument reduction plus Taylor polynomials, using only `+ - * /`.
/// Every operation is IEEE-specified and correctly rounded, so this is
/// identical on every conforming target by construction — which is a stronger
/// promise than "we all call the same library", and it costs no dependency.
/// Q3 shipped its own tables for substantially this reason.
#[inline]
#[must_use]
pub fn sin_cos(radians: Scalar) -> (Scalar, Scalar) { /* … */ }
```

**No new dependency.** The alternative — depending on the `libm` crate, which is what wasm already
statically links — produces the same bit-identity and I verified it does (§1.2). It was not chosen,
for reasons that hold up: `straf3-sim`'s manifest treats a second dependency as an architectural
change, and an implementation the project owns cannot be altered by someone else's patch release.
Recorded here so the road not taken is on record; not reopened.

**Accuracy is not the goal, and this is worth saying explicitly.** The owned implementation is within
1 ULP of libm, which is more than enough — but even if it were worse, it would be *correct for this
project*, because the physics is defined as whatever this function returns. What cannot be tolerated
is two answers. Reviewers should judge it on determinism and on faithfulness to Q3, not on
approximation error.

**Enforcement:** extend the existing source scan in `xtask/src/seam.rs` to reject `.sin(`, `.cos(`,
`.sin_cos(`, `.tan(`, `.asin(`, `.acos(`, `.atan2(`, `.exp(`, `.ln(` and `.powf(` anywhere in
`crates/straf3-sim/src/` except `num.rs`. This project enforces its architecture rather than
documenting it, and this rule is exactly as checkable as the ones already there. Without it, the
next `.sin()` written anywhere in the physics silently reintroduces the whole problem.

**Cost:** three lines of `step.rs`, one function in `num.rs`, one seam rule. The movement tests use
tolerances well above 1 ULP and should be unaffected; any recorded fixture checksums change and must
be re-recorded — which is the argument for landing it before any reference demo exists.

**Without it:** D1 does not work. Runs are rejected non-deterministically once they pass roughly
14 seconds, and a leaderboard whose verification randomly fails is worse than no leaderboard. The
apparent shortcut — deploy the server on musl so it matches — is the trap described in §1.0.

### C2 — a cross-target determinism check must run in CI

**What:** an `xtask` command and a CI job that runs one reference command stream through
`x86_64-unknown-linux-gnu` (glibc), `x86_64-unknown-linux-musl`, `x86_64-pc-windows-gnu` and
`wasm32-unknown-unknown` (executed under Node or `wasmtime`), and fails if they do not all agree.

**It must compare the rolling digest over every command, not the final checksum** (§1.3). Comparing
end states is how the probe's reconvergence case would have passed. Including glibc *and* musl is
what makes the check meaningful at all: they are the two sides of the fault line, and a check that
omitted glibc would go green on a tree that has the bug.

**Why:** C1 fixes today's divergence. This is what stops tomorrow's. Every one of the divergences in
§1 was invisible to a test suite that only compares a binary against itself, and the next one will
be too. This is also the natural home for the finding in §1.1(d): the check is not a web feature,
it is the missing half of the project's existing determinism promise.

**Note the scope change this implies.** Rev 1 scoped determinism to *same binary, same machine*.
Verified leaderboards need *same source, any target*. That is a specification amendment, and it is
the operator's to make — see §11, decision A.

**Cost:** the reference stream already exists in `tests/determinism.rs`. The wasm runner is ~15 lines
of JavaScript (Appendix A has a working one). The Windows binary runs under WSL interop on the
current box.

### C3 — view angles must be 16-bit, as Quake's were

**What:** the recorded and simulated view angle is a `u16` per axis, Q3's `ANGLE2SHORT`
(`angle * 65536 / 360`), giving 0.0055° of resolution.

Two ways to land it, and they differ in how much they actually guarantee:

- **Recommended — `UserCmd` carries shorts.** `ViewAngles` becomes `{ pitch: u16, yaw: u16, roll:
  u16 }` and `angle_vectors` performs `SHORT2ANGLE` internally. This is what Q3 did: `usercmd_t`
  carried shorts over the wire and `PM_UpdateViewAngles` converted. It makes a recording exact by
  construction — there is no representation in which a demo can round-trip imperfectly — and it
  makes the input space finite, which matters for both the format and for reasoning about
  reproducibility. Cost: touches `cmd.rs`, `step.rs`, the headless input format and every movement
  test that writes a yaw.
- **Cheaper — keep `Scalar`, add constructors.** `ViewAngles::from_shorts(..)` / `to_shorts()`, and
  the platform layer is required by convention to build angles only through them. Costs almost
  nothing; guarantees almost nothing, because the convention is unenforceable.

**Why it matters beyond tidiness:** mouse input is a stream of deltas accumulated into an angle. If
that accumulator is an `f32` in degrees, its value depends on the order and magnitude of every delta
since the run began, and two clients with the same *intent* have no reason to produce the same
number. Quantising at the point of accumulation removes the whole question. It also shrinks the demo
format (§4) and defines the delta alphabet the encoder compresses.

**Without it:** demos are still verifiable — the input is whatever it is — but the format carries
three f32s per command instead of two u16s, and the client and any future replay tooling must agree
on float formatting rules they have no reason to agree on.

### C4 — the run clock must live below the seam, and be driven by swept trigger tests

**What:** `straf3-sim` must compute the run time. Today `SimState.run` is always `NotStarted`:
`RunState::start` and `RunState::finish` are `pub` and **called from nowhere in the workspace**
(verified by grep). The one number the entire product is about is not produced by any code.

The mechanism must be a seam, for the same reason `World` is one — and it turns out to need no new
method on `World` at all, only one more field on what `trace` already returns:

```rust
/// Trigger volumes the swept hull touched during a move.
///
/// A bitmask rather than a collection: `straf3-sim` allocates nothing, and a
/// map has a small fixed number of timing volumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TriggerSet(pub u32);

impl TriggerSet {
    pub const START: Self = Self(1 << 0);
    pub const FINISH: Self = Self(1 << 1);
    // bits 2..=31: checkpoints, for splits
}

pub struct Trace {
    // ... existing fields unchanged ...

    /// Timing volumes the hull overlapped along the part of this sweep it
    /// actually travelled — that is, over `start ..= start + (end - start) *
    /// fraction`, **not** over the whole requested segment.
    ///
    /// Triggers are non-solid, so they never affect `fraction` or `normal`:
    /// this field is what the sweep *passed through*, not what stopped it.
    /// But it must stop where the sweep stopped. Callers issue sweeps whose
    /// motion they then discard, and reporting volumes beyond `fraction`
    /// credits a player with a finish line they never reached.
    pub triggers: TriggerSet,
}
```

**Triggers ride on `Trace`; there is no second query.** This is the resolution of a question the
first draft of this document left dangerously open, so it is worth stating why, because the obvious
alternative — a parallel `fn triggers(&self, sweep: &Sweep) -> TriggerSet` next to `trace` — is
worse in three separate ways.

*Cost.* A separate method doubles the number of collision queries a command performs, and §4.3's
verification budget has no line item for that. Riding on `Trace` costs one extra `u32` per query and
no additional broadphase descent: in a BSP tracer, trigger brushes are `CONTENTS_TRIGGER` volumes
sitting in the same tree the sweep already walks. The tracer currently reaches them and discards
them. It is strictly cheaper to OR a bit than to skip the node.

*Granularity, which is the actual trap.* `Pmove::run` does not issue one sweep per command. It
issues up to a dozen along a genuinely bent path: `ground_trace` twice, `slide_move`'s bump loop up
to `SLIDE_BUMPS` (4) times, `step_slide_move` re-running `slide_move` after a step-up plus two probe
sweeps of its own, and `correct_all_solid` up to 27 more in the stuck case. That fan-out is not
incidental — it is the mechanism by which `PM_SlideMove` makes a player follow a corner or a ramp
instead of travelling in a straight line. **A single coarse sweep from command-start origin to
command-end origin is a chord across that bend, and it can miss a trigger volume the player's real
hull went through.** That is the same failure this contract item exists to prevent, reintroduced one
level down. Making triggers a field of `Trace` removes the question entirely: the granularity is the
granularity of `trace`, by construction, and there is no second call site whose cadence could drift.

*Hull correctness.* `Sweep` already carries `half_extents` and `center_offset`, and `check_duck`
changes them mid-command. A trigger is touched when the *hull* overlaps it, as in Q3 — so trigger
tests must inherit the exact hull of the sweep that found them. A field on `Trace` inherits it for
free; a second method would have to be handed the same hull and would be one refactor away from not
being.

**Which sweeps count — the sibling bug, in the opposite direction.** Not every `Sweep` the physics
issues is motion. Some are queries about geometry the hull never enters, and some are along a path
that is subsequently *thrown away*. OR-ing all of them into one accumulator produces false
positives — crediting a player with a trigger they never touched — which on a finish line is exactly
as wrong as missing one.

Reading `step.rs`, every trace funnels through `Pmove::sweep` except `check_duck`'s stand-up probe,
which calls `world.trace` directly with a different hull. The rule:

| Call site | Counts? | Why |
|---|---|---|
| `slide_move`'s bump-loop `sweep_to` | **yes** | this is the move; `p.origin` advances along it |
| `step_slide_move`'s up-lift `sweep_to(start_o, up)` | **yes** | the hull really is carried upward |
| `step_slide_move`'s down-drop `sweep_to(p.origin, down)` | **yes** | the hull really is carried down |
| `ground_trace`'s downward probe | no | a question about the floor; the hull does not go there |
| `step_slide_move`'s step-down probe (`sweep(start_o, down)`) | no | a question about whether stepping is allowed |
| `check_duck`'s stand-up probe | no | zero-length, and a different hull |
| `correct_all_solid`'s jitter probes | no | zero-length point tests |

That table classifies *call sites*, and classifying call sites is not sufficient, because three of
the "yes" sites do not always commit the motion they swept. Reading `step.rs` again at the level of
the position writes rather than the calls:

- `slide_move`, line 794: `if trace.all_solid { p.velocity.z = 0; return true }` — `p.origin` is
  never advanced.
- `slide_move`, line 801: `if trace.fraction > s(0.0) { p.origin = endpos }` — on a zero fraction
  the hull does not move.
- `step_slide_move`, lines 926–929: `if up_trace.all_solid { return }` — the lift is abandoned and
  `p.origin` is never set to `up_pos`.
- `step_slide_move`, lines 938–942: `if !down_trace.all_solid { p.origin = down_pos }` — the drop
  is conditional in the same way.

Each of those is a sweep that was *issued* over a segment the hull then did not travel. So the
governing invariant is not "which calls count" but:

> **A trigger is touched iff the player's hull overlapped it somewhere on the path the player
> actually occupied.**

Two mechanisms enforce that, and they are different in kind — one belongs to the tracer, one to
`step_slide_move`.

**Rule 1 — the tracer reports the traversed prefix, not the queried segment.** `Trace::triggers`
must contain the volumes the hull overlaps while swept over `[start, start + (end - start) *
fraction]`, not over `[start, end]`. This is a contract on `World::trace` (C8), stated here because
the natural BSP implementation violates it: the descent visits `CONTENTS_TRIGGER` leaves along the
whole query segment, and OR-ing each one as it is reached gathers volumes past the impact point.
Gathering must be filtered by the same interval fraction the tracer is already computing for the
solid hit.

This single rule disposes of all four branches above without any of them needing special handling.
When `fraction` is zero the traversed prefix is the hull at rest at `start`, so the only volumes
reported are the ones the player is *standing in* — genuinely touched, and already reported by the
previous committed sweep. `TriggerSet` is OR-ed, so re-reporting them is idempotent. A trigger under
a low overhang cannot be credited by an aborted step-up, because the aborted lift traverses nothing.

That reasoning leans on `all_solid` implying a zero fraction, which today is a property of
`FlatGround`'s implementation (`world.rs:211-218` returns `fraction: s(0.0)` on `start_solid`, and
`all_solid` is only ever set in that branch) rather than a stated obligation. Make it one:
**`all_solid` must imply `fraction == 0.0`.** It is already true of the only implementor and it is
true of Q3's tracer, but a hull that starts inside solid has not legitimately travelled anywhere, and
without the obligation written down a future tracer could report `all_solid` alongside a nonzero
fraction and quietly reopen the leak this rule closes.

Note the deliberate looseness: the tracer knows `trace.fraction`, not `sweep_to`'s
`SURFACE_CLIP_EPSILON`-backed-off fraction (`step.rs:340-345`), so gathering is clamped to the
former. That over-reports by at most the epsilon backoff at the moment of impact — a hull flush
against a wall reports a trigger it is touching but not quite standing in. That is the correct
direction to err on a finish line, and it is bounded by a constant rather than by the length of the
move.

**Rule 2 — the one genuine rollback.** Rule 1 does not cover `step_slide_move`'s first
`slide_move`, because that traversal really did happen: the hull moved along it, and only afterwards
does the function **overwrite `p.origin` with `up_pos` and `p.velocity` with `start_v`**, discarding
the attempt and re-running from the stepped-up position. Triggers accumulated during a traversal
that is subsequently un-done must be un-done with it. So the accumulator is savepointed exactly
where origin and velocity are — and, critically, restored on the *commit* path only:

```rust
// In Pmove, alongside `ground_plane` / `walking`:
touched: TriggerSet,

// step_slide_move, where start_o / start_v are captured:
let saved_triggers = self.touched;      // a u32 copy

// ... restored only alongside the writes that discard the first attempt:
p.origin = up_pos;
p.velocity = start_v;
self.touched = saved_triggers;
```

The placement is load-bearing in both directions. Restoring it here is required, because the first
attempt's path is abandoned. Restoring it on the `up_trace.all_solid` early return at line 927 would
be a *bug*, because that return keeps the first attempt's result — the player really did move there
— and rolling back would drop triggers they genuinely crossed. The early return and the successful
lift look symmetric and are not, which is why Rule 1 rather than a second savepoint is what makes
the aborted lift safe.

Because `TriggerSet` is a `u32`, savepoint-and-rollback is a register copy. The discipline costs
nothing; the absence of it silently mis-credits every trigger near a stair.

**The test that pins both rules**, and which `straf3-sim` should carry rather than the server: a
`World` implementation with one trigger volume and a one-unit step, driven so that (a) a step-up
succeeds over a trigger the first attempt clipped and the second did not — the accumulator must not
contain it; (b) a step-up aborts `all_solid` under an overhang while a trigger sits beyond the lift
— the accumulator must not contain it; (c) the player walks flat through the trigger at 1,000 ups —
it must. Case (b) fails under the call-site table alone and passes under Rule 1, which is the whole
reason Rule 1 is written down.

**Three further requirements that are easy to get wrong:**

1. **Swept, not sampled.** At 1,000 ups and 8 ms a player moves 8 units per command. A finish
   trigger tested only at the command's endpoint is missed by anyone fast — which is to say, by
   exactly the runs a leaderboard cares about. Testing the endpoint is the single most likely bug in
   this whole design; testing one chord per command, as above, is the second.
2. **Evaluated at command boundaries, in integer milliseconds.** `RunState` already stores
   `started_at_ms` / `finished_at_ms` as `u32`. The clock reads `SimState::time_ms`, which is the
   exact integer sum of command durations. No interpolation, no sub-tick estimate: a sub-tick time
   would be a float and would have to be reproduced bit-exactly by the verifier for no benefit.
   Note the shape this gives when `pmove_msec` sub-stepping lands (`step.rs:147`): the accumulator
   is consumed at the end of each `Pmove::run`, so sub-stepping makes the clock *finer* without
   making it float, and without changing this rule.
3. **`Trace` gains a field, which is a mechanical break, not a design one.** `FlatGround` and
   `EmptyWorld` build `Trace` literals and will not compile until they set `triggers:
   TriggerSet::default()`; `Trace::clear()` already exists and covers most of it. This is deliberate
   rather than hidden behind `#[non_exhaustive]`: a `World` implementor that has triggers and
   forgets to report them produces a leaderboard that silently never finishes, so the compiler
   should make every implementor look at the field once.

**Consequence to accept deliberately:** times quantise to the command duration — multiples of 8 ms
at 125 Hz, 4 ms at 250 Hz. Ties will be common, and a higher tick rate produces finer times. This is
one of the reasons §11 decision C proposes fixing the ranked tick rate.

**Without it:** the platform has nothing to rank. Every other part of this document is machinery for
moving a number that does not exist.

### C5 — `straf3-replay` owns the format; `straf3-sim` gains no serialisation

**What:** `straf3-replay` (currently a 26-line stub with one field) becomes the crate that defines
the `.s3d` container in §3 and exposes:

```rust
pub fn encode(recording: &Recording) -> Vec<u8>;
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Recording, DecodeError>;

/// Re-simulate and report what actually happened. The browser and the server
/// call this same function — that is the point of it existing.
pub fn verify<W: World>(rec: &Recording, world: &W) -> Verdict;

/// Identity of the *run*, independent of how it was encoded.
///
/// blake3 over the `PhysicsIdent`, the spawn, and the decoded command stream
/// in a canonical fixed-width form — NOT over the `.s3d` bytes. See §8.3: the
/// bit-packed encoding has slack (absolute-versus-delta angle seeding, the
/// optional checkpoint trail, reserved bytes), so the same run has many valid
/// byte encodings and a hash of the file is trivially perturbed without
/// touching the simulation. This value is what the platform makes unique.
pub fn canonical_digest(rec: &Recording) -> [u8; 32];

pub struct Verdict {
    pub outcome: Outcome,              // Finished { time_ms: u32 } | DidNotFinish | Rejected(..)
    pub final_checksum: u64,
    /// FNV-1a fold over `SimState::checksum()` after EVERY command. This, not
    /// `final_checksum`, is what a recording is compared against — see §1.3:
    /// a run can diverge and reconverge, and an end-state comparison misses it.
    pub rolling_digest: u64,
    /// First command index where our per-command checksum disagreed with the
    /// recording's checkpoint trail, if any. Diagnostic, not a security
    /// check — see §8.2.
    pub first_divergence: Option<u32>,
    pub commands_run: u32,
}
```

**`verify` must fold the rolling digest itself**, inside the same loop that steps the simulation.
Offering an API where a caller can obtain a verdict without it — or can compute it separately and
"forget" — puts the §1.3 mistake one careless call site away.

**Why the direction matters:** `straf3-sim` must not gain `serde`. It has one dependency and a
manifest that treats a second as an architectural change; C1 spends that budget for a reason that
cannot be got any other way. Serialisation can live one crate up at zero cost to the simulation.

**Hostile input is a hard requirement here.** `decode` runs on the server against attacker-supplied
bytes. It must not panic, must bound every allocation by a `Limits` argument (max commands, max
checkpoint count, max declared sizes), must reject trailing garbage, and should have a `cargo-fuzz`
target. A panic in `decode` is a denial-of-service on the verifier.

### C6 — physics identity must be a value a recording can carry

**What:** a recording binds to the exact physics it ran under:

```rust
pub struct PhysicsIdent {
    pub sim_version: [u8; 3],     // straf3-sim semver
    pub sim_build: u64,           // hash of the built artifact, from the build system
    pub profile_digest: u64,      // PhysicsProfile::digest()
    pub map_source_sha256: [u8; 32],
    pub map_collision_digest: u64,
    pub tick_rate_hz: u16,
}
```

which requires `PhysicsProfile::digest(&self) -> u64` — an FNV-1a fold over the exact bits of every
field, in the style of `SimState::checksum`. This is ~20 lines and it is the thing that stops a
tuned CPM constant silently invalidating a leaderboard.

**Why a digest and not a name:** "cpm" is not a physics profile, it is a label on one. The CPM
constants are explicitly community reconstructions marked `TODO(wave2)` and the operator is expected
to tune them. When `air_control` moves from 150 to 145, every stored time becomes a time under
different physics. The digest is what makes that detectable rather than silent. §5.4 covers what to
*do* about it.

### C7 — map ingestion: `.map` in, deterministic hulls out

The operator has chosen importing the existing Quake/CPMA/Defrag corpus over authoring. So the map
format is not ours to design — it is Valve 220 `.map` text, parsed by the `quake-map` crate already
pinned in the workspace. What the platform needs from `straf3-map` is:

1. **`compile(&str) -> Result<CompiledMap, CompileError>` stays text-in.** It already is, and its
   module docs already give the reason. This is what lets the browser fetch a `.map` over HTTP and
   compile it in wasm through the identical code path the server uses. Do not add a path-taking
   variant to this crate.
2. **`CompiledMap` must produce a `World` implementor.** Nothing currently connects `straf3-map` to
   `straf3-sim`'s collision seam. `fn collider(&self) -> impl World + '_` (or `impl World for
   CompiledMap`) is the missing link, and it must also implement `triggers` from C4.
3. **`compile` must be deterministic and identical across targets.** It is the second body of float
   code in the verification path — plane intersection, hull construction — and it is subject to
   exactly the divergence C1 fixes in the simulation. It must be covered by C2's cross-target check.
4. **`collision_digest() -> u64`** over the exact bytes of the compiled hulls and trigger volumes.
   Bound the recording to this, not only to the source hash: a change to the *compiler* changes the
   geometry the physics ran against even when the `.map` file is byte-identical.
5. **Defrag entity conventions must be read.** Start and finish are `target_startTimer` /
   `target_stopTimer` entities wired to `trigger_multiple` brushes; spawns are
   `info_player_deathmatch` / `info_player_start`. That mapping is what turns an imported map into
   something with a time. It belongs in `straf3-map`, not in the web layer.

**Not the platform's decision, but it must be flagged:** `.map` files reference textures and shaders
that ship in `.pk3` archives. Community Defrag maps are freely circulated, but the redistribution
rights on their texture sets vary and some inherit id assets. Hosting them for browser download is a
licensing question I cannot resolve and should not assume — §11, decision F. It does not block
anything in this document: the first browser client can render untextured geometry, and arguably
should, because texture download would dominate the bundle.

### C8 — the collision implementor must be deterministic across targets

The `World` trait's doc comment already requires implementors to be pure and deterministic, and
already forbids work-stealing parallelism. C1 adds a requirement to that contract: **the implementor
must also produce identical results on wasm and native.** Concretely, whatever answers `trace` must
avoid `f32::sin/cos/tan/exp/powf` for the same reason `straf3-sim` now does, and must not use SIMD
paths that exist on one target and not another.

This was a live constraint on the parry evaluation that spec section 4 made conditional: **"is parry
deterministic?" became "is parry deterministic *across targets*?"**, which is a materially harder
question. A hand-written brush tracer over convex hulls — which is what the compiled-map path wants
anyway, and what Q3 itself did — sidesteps it, and that is the route C8 took. The evaluation was
never run, because nothing came to depend on its answer; `parry3d` has been dropped from the
workspace, and re-adding it puts the across-targets question back on the table.

C4 adds a second requirement to the same contract, and it is the one an implementor is most likely
to get wrong by writing the obvious code: **`Trace::triggers` must report only the volumes the hull
overlaps within the traversed prefix `[start, start + (end - start) * fraction]`.** The natural BSP
descent walks the whole query segment and would OR in every `CONTENTS_TRIGGER` leaf it passes,
including ones beyond the impact point — and the physics issues several sweeps whose motion it then
does not commit (`step.rs:794`, `801`, `927`, `940`), so those over-reported volumes become finish
lines credited to players who never reached them. The clamp is not an optimisation; it is what makes
the accumulator mean what C4 says it means. This is a `World` obligation rather than a `Pmove` one
because only the tracer knows where along the segment each overlap occurred.

A corollary the clamp depends on, and which the `World` doc comment should state outright:
**`all_solid` must imply `fraction == 0.0`.** `FlatGround` already satisfies it (`world.rs:211-218`)
and Q3's tracer does too, but it is currently incidental. Written down, it makes "the hull started
inside solid" and "the hull travelled nowhere" the same fact, which is what lets the aborted step-up
at `step.rs:927` be safe without a second rollback point.

An additional argument for the hand-written tracer: this requirement is natural to satisfy in code
that already computes the entry fraction per leaf, and awkward to bolt onto a third-party library
whose query API returns a first hit rather than an ordered traversal.

### C9 — `straf3-platform` and `straf3-render` must be specified web-first

There is no incumbent implementation: `straf3-game`, `straf3-platform` and `straf3-render` are all
stubs whose module docs say the real content lands later. This is a greenfield seam, so the contract
can be stated rather than reverse-engineered.

**`straf3-platform` must expose a frame-to-commands function, and it is the only place wall time is
allowed to touch the game:**

```rust
/// Converts elapsed wall time into whole-millisecond commands at a fixed rate.
///
/// Above the seam because it reads a clock. The simulation never learns that
/// frames exist: it receives commands with integer durations, and how many
/// arrive per frame is not its business.
pub struct CommandPump { rate: TickRate, accumulated_ms: u32, /* ... */ }

impl CommandPump {
    /// Emit the commands owed for `elapsed_ms` of wall time. Never emits a
    /// zero-length command (the simulation early-returns on those and it would
    /// silently stop counting ticks), and never emits one longer than the
    /// pmove sub-step limit.
    pub fn pump(&mut self, elapsed_ms: u32, input: &InputSnapshot) -> impl Iterator<Item = UserCmd>;
}
```

Requirements:

- **The wall clock is floored to integer milliseconds before it reaches this type.** In the browser
  that means `performance.now()` is floored, not rounded and not passed as a float. Browser timer
  coarsening and `requestAnimationFrame` jitter then have no effect on physics whatsoever, because
  the accumulator absorbs them and the commands are fixed-duration. This is worth stating explicitly
  because it is the property that makes browser play legitimate rather than a compromise.
- **Mouse deltas accumulate into the 16-bit angle from C3**, never into f32 degrees. In the browser
  the source is Pointer Lock `movementX`/`movementY`, which are integers.
- **The renderer never advances the simulation.** `InterpolationAlpha` already exists and says so;
  the renderer holds the previous and current `SimState` and interpolates between them for display
  only. A displayed position is never fed back.
- **The client's displayed time comes from `SimState.run`,** i.e. from the simulation, never from a
  timer in JavaScript or from `Date.now()`. There must be no code path in the frontend capable of
  computing a run time.

**`straf3-render` targets web and native from one implementation.** `wgpu` and `winit` both do; that
is the convergence that makes browser-first cost nothing. The stack is measured and viable — 132 KiB
of gzipped wasm for a working WebGPU skeleton in Chrome 146 (§1.5) — and rev 6 §Q2 fixed its shape:

- **WebGPU only. One backend per bundle, and no fallback compiled in.** This is not a default to be
  revisited during implementation; it is a decision made on measurement. Compiling WebGL2 in
  alongside costs 595 KiB (727 vs 132) paid by every visitor, and — measured, not read from docs —
  `wgpu` **crashes inside the WebGPU backend rather than degrading to GL** when both are present and
  `requestAdapter()` returns null. The fallback you paid for does not fire.
- **The host page picks the backend in JavaScript, before entering wasm.** Capability detection is
  `navigator.gpu` plus a successful `requestAdapter()` in JS; on failure the page shows a browser
  message and never instantiates the module. There must be no code path where wasm starts up and
  then discovers it has no adapter.
- **If WebGL2 is ever wanted it is a separate, on-demand bundle**, selected by that same JS check —
  not a fallback baked into the primary artifact. The backend is already a runtime parameter to
  `wgpu`, so this is a build-matrix change, not an architecture change, and it stays cheap to decide
  later (§11 decision E).
- **No `egui` in the web build.** `egui-winit` 0.36.1 cannot compile for `wasm32` (`arboard` has no
  wasm backend), so `crates/straf3-devtools` has no web target today. Rather than work around it,
  the web build drops `egui` entirely: it is the largest single saving available (−1.83 MiB) and the
  overlay it provides — speedometer, graphs, trace telemetry — is cheaper and more legible as DOM
  elements positioned over the canvas. `straf3-render` must therefore not depend on `egui`, and any
  telemetry it exposes must be readable as plain values a host page can render.

**Stage D — once the one number still missing** — is measured: the game crates plus `gltf` are
131 369 B of gzipped wasm, marginally *under* stage A's skeleton
(`probes/wasm-render/sizes.txt`). The contingency this paragraph used to hold — "if it comes back
large, ask whether the web build needs `parry3d` at all, or ships precompiled hulls" — was never
needed and is now moot: `parry3d` was called by nothing, contributed ~0 bytes, and has been removed
from the workspace. The backend decision was never in question.

**This item is Wave 3's contract.** Rev 6 §S sets Wave 3 as the first playable straf3, and its
acceptance criteria 3 and 5 are C9 restated from the other side: input captured as the same
integer-millisecond `UserCmd` the simulation already takes, and frame pacing decoupled from
simulation stepping so the fixed command cadence survives a variable frame rate. `CommandPump` is
what satisfies both. Criterion 4 — a recorded input sequence replaying to the same checksum through
the windowed build as through `straf3-headless` — is the test that C9's seam was not bypassed.

### C10 — no glam SIMD types below the seam

`straf3-sim` uses `glam::Vec3`, which is scalar on every target: `dot` is
`(x*rhs.x) + (y*rhs.y) + (z*rhs.z)` in a fixed order and `length` is `sqrt(dot)`. That is why the
divergence in §1 was *only* trigonometric.

`Vec3A`, `Mat4` and `Quat` are a different matter: they take SSE2 paths on x86-64 and scalar paths
on wasm unless `simd128` is enabled, and horizontal reductions do not associate the same way. The
contract is that below-the-seam code uses `Vec3` and not the 16-byte-aligned types, and the seam
check should say so alongside its existing `fast-math` and `scalar-math` rules.

Related: the seam check's justification for forbidding `scalar-math` — "replaces glam's SIMD paths
with scalar ones, changing float results" — was written for same-machine determinism. Under
cross-target determinism the reasoning inverts: a single scalar path *everywhere* is the safe
configuration. The rule happens to be harmless today because `Vec3` is scalar regardless, but the
comment should be updated so the next person does not reason from it in the wrong direction.

### C11 — build-system items

- Add the `web/` crates to `ABOVE_THE_LINE` in `xtask/src/seam.rs`, so the one-directional rule
  covers them and nothing below the seam can ever reach the web stack.
- Set `[workspace] default-members` to the game crates, so a plain `cargo build` at the root does
  not compile axum and tokio.
- Add `wasm32-unknown-unknown` to `rust-toolchain.toml`'s `targets`.

### Contract summary

| | Item | Owner crate | Size | Blocks |
|---|---|---|---|---|
| C1 | Owned `sin_cos` (**decided**) | `straf3-sim` | hours | everything |
| C2 | Cross-target determinism in CI | `xtask` + CI | hours | trusting C1 over time |
| C3 | 16-bit view angles | `straf3-sim` | ~a day | format exactness |
| C4 | Run clock + swept triggers | `straf3-sim`, `straf3-map` | days | leaderboards entirely |
| C5 | `.s3d` + rolling digest + hostile-safe decode | `straf3-replay` | days | submission, playback |
| C6 | `PhysicsProfile::digest` + `PhysicsIdent` | `straf3-sim`, `straf3-replay` | hours | correctness over time |
| C7 | `.map` → hulls, triggers, digest | `straf3-map` | weeks | any real map |
| C8 | Cross-target-deterministic collision | `straf3-collision` | weeks | verification on real maps |
| C9 | `CommandPump`, web-first render seam | `straf3-platform`, `-render` | weeks | playing at all |
| C10 | No SIMD vector types below the seam | `xtask` | hours | future divergence |
| C11 | Workspace/seam/toolchain plumbing | `xtask`, root | hours | building `web/` |

C1 is decided and lands the moment Wave 2 closes; C2 can be built now against the current tree and
will simply fail until C1 lands, which is the correct behaviour for it. Between them they touch
three lines of `step.rs` and unblock everything else in this document.

---

## 3. Run submission format

### 3.1 Shape

One artifact, two uses. The **`.s3d` recording** is the demo, and submitting a run is uploading one.
There is no separate "submission format" carrying a claimed time, because **the server does not
accept a claimed time — it computes one** (§7.2). Everything a submission needs is either in the
recording or in the session cookie.

```
POST /v1/runs
Content-Type: application/vnd.straf3.demo
Content-Encoding: zstd
Cookie: s3_session=...
X-Straf3-Ticket: <attempt ticket from POST /v1/attempts>

<.s3d bytes>
```

Response `202 Accepted`:

```json
{ "run_id": "01J...", "status": "pending", "poll": "/v1/runs/01J..." }
```

Anonymous submissions are rejected with `401`. Anonymous play still records locally — see §6.4.

**The attempt ticket** is a server-issued opaque value bound to the session, the map and the profile,
obtained from `POST /v1/attempts` when the player starts a run and valid for a bounded window
(§7.3). It is **not** in the `.s3d` header and must not be: the recording is a portable artifact
meant to be downloaded, replayed and shared, and putting session state inside it would make every
published demo carry a fragment of someone's login. It travels in the envelope, where it belongs.

Be precise about what the ticket does and does not buy, because it is easy to over-read. It does
**not** authenticate the inputs — nothing can, see §8.3. What it does is force a submission to be
attached to a live, rate-limited attempt by a signed-in session, which is what turns "scrape the
public demo archive and mass-resubmit" into "sit through one interaction per stolen run".

### 3.2 `.s3d` layout

Little-endian throughout. No field is a float duration; no field is a float second.

**Header (fixed, 96 bytes):**

| off | size | field | notes |
|---|---|---|---|
| 0 | 4 | magic `S3D\0` | |
| 4 | 2 | `format_version` | starts at 1 |
| 6 | 2 | `flags` | bit 0: uniform command duration; bit 1: has checkpoint trail |
| 8 | 3 | `sim_version` | major, minor, patch |
| 11 | 1 | `profile_layout_version` | bumped when `PhysicsProfile` gains a field |
| 12 | 8 | `sim_build` | hash of the built artifact |
| 20 | 8 | `profile_digest` | `PhysicsProfile::digest()` (C6) |
| 28 | 32 | `map_source_sha256` | identifies the `.map` text |
| 60 | 8 | `map_collision_digest` | identifies the compiled hulls (C7) |
| 68 | 2 | `tick_rate_hz` | 1..=1000 |
| 70 | 2 | `command_duration_ms` | redundant with the rate; verified against it on decode |
| 72 | 4 | `command_count` | bounded by `Limits` |
| 76 | 4 | `checkpoint_interval` | commands between checksum entries; 0 = none |
| 80 | 4 | `spawn_index` | which map spawn; avoids carrying floats |
| 84 | 2 | `spawn_yaw` | 16-bit angle (C3) |
| 86 | 2 | reserved | zero |
| 88 | 8 | `client_rolling_digest` | folded over **every** command — see §1.3 and below |

Absolute spawn coordinates are deliberately *not* in the header. They come from the map, identified
by digest. A recording cannot ask to start somewhere the map does not put you.

**Command block:** the bit-packed encoding measured in §4.1. Per command, in order:

- 1 bit — movement axes changed since the previous command
- 1 bit — buttons changed
- if axes changed: 2 bits per axis (`0`, `+127`, `−127`, escape) plus 8 bits on escape
- if buttons changed: 4 bits
- pitch: 1 bit absolute-seed flag, then either 16 bits absolute or a zigzag delta in a 6/12/17-bit
  class selected by a 1–2 bit prefix
- yaw: the same

Roll is never encoded; player input never sets it (`cmd.rs` says so) and the decoder writes zero.
Command duration is not encoded per command when the uniform flag is set. When it is not set — which
should not happen for a browser client, and is reserved for future sub-stepped or tool-assisted
recordings — a 16-bit duration precedes each command.

**The rolling digest, and why it is a header field rather than a checkpoint interval.** §1.3 is the
reason: the probe observed a run whose final checksum matched across builds while 29 of its 1,200
intermediate states did not. Anything sampled — end state, or one checkpoint per second — can miss a
transient divergence that reconverges. So the client folds `SimState::checksum()` into an FNV-1a
accumulator **after every single command**, and that one `u64` is the value the verifier compares.
It costs 8 bytes for a whole run and it cannot miss anything: any state that ever differed changes
it permanently.

**Checkpoint trail:** `command_count / checkpoint_interval` entries of
`{ command_index: u32, checksum: u64 }`. At one entry per simulated second this is 12 bytes/second.
Its job is **localisation, not detection** — the rolling digest says *whether* a divergence happened,
the trail narrows down *where*, and a binary search over it plus one re-simulation finds the exact
command. Getting this the wrong way round, and treating the trail as the detector, is precisely the
error §1.3 documents.

**Trailer:** 4-byte `blake3` prefix over the header and body, for cheap corruption detection. Not a
security boundary: an attacker recomputes it trivially, which is fine, because §8 does not rely on
the file being unforgeable.

### 3.3 Why input-only, restated with numbers

Storing states instead of inputs would be 7 floats × 4 bytes × 125/s = 3.4 KiB/s, about **10× larger**
(§4.2), and — decisively — unverifiable: a state trail is a claim, and re-simulating it is exactly
as expensive as re-simulating inputs while proving strictly less. Inputs are the smaller artifact
*and* the stronger one.

---

## 4. Demo storage, with real arithmetic

### 4.1 Measured encoding cost

I implemented the §3.2 encoder and ran it over synthetic strafejump streams at 125 Hz — alternating
strafe every 40 commands, a yaw sweeping ±0.9°/command, jump every 60th command, and Gaussian mouse
jitter at three levels. Jitter is the variable that matters, because it is what the yaw delta coder
has to spend bits on.

| mouse jitter | raw | bit-packed | + deflate | bytes/second |
|---|---|---|---|---|
| 0.02° | 11.0 B/cmd | 3.16 B/cmd | **1.87 B/cmd** | 234 B/s |
| 0.08° | 11.0 B/cmd | 3.16 B/cmd | **2.61 B/cmd** | 327 B/s |
| 0.25° | 11.0 B/cmd | 3.23 B/cmd | **2.94 B/cmd** | 367 B/s |

I use the **pessimistic 2.94 B/cmd** everywhere below. Real mouse input is noisier than a synthetic
sweep, so treat 3 B/cmd as the planning number and the measured 1.87 as the floor.

A **45-second run** — a reasonable median for a Defrag map worth ranking — is 5,625 commands:

```
command block   5,625 × 2.94 B   = 16.1 KiB
header                             96 B
checkpoint trail  45 × 12 B      = 540 B
                                 ─────────
                                  ~17 KiB
```

30 s ≈ 11 KiB · 60 s ≈ 22 KiB · 120 s ≈ 44 KiB.

### 4.2 What that costs at rest

Retention policy assumed: **keep the current personal best per (player, map, profile), plus every
run that ever held a top-10 position.** Non-PB attempts are verified, their times recorded for
statistics, and their bytes discarded.

| population | PB demos (25 map-profiles/player) | at rest | monthly (S3/R2 ≈ $0.015/GB) |
|---|---|---|---|
| 1,000 | 25,000 | 0.41 GiB | **$0.01** |
| 10,000 | 250,000 | 4.1 GiB | **$0.06** |
| 100,000 | 2,500,000 | 41 GiB | **$0.62** |

Top-10 history adds trivially: 200 maps × 2 profiles × 50 historical records × 17 KiB = 340 MiB.

And the profligate alternative, for comparison — **keep every attempt forever**, 10,000 players
averaging 20 attempts a day: 3.4 GiB/day, **1.2 TiB/year, ≈ $19/month** by the end of year one.

**The conclusion is that storage never constrains this design.** Even the wasteful policy is a
rounding error. Choose retention on product grounds — do players want to re-watch a failed attempt?
— and not on cost. Egress is similarly negligible: a ghost download is 17 KiB, so 100,000 ghost
views a month is 1.6 GiB, free on R2 and about $0.15 on S3.

### 4.3 What *does* cost: verification CPU

Measured on this machine (8 cores, `--release`, `lto = "thin"`), running commands through
`straf3-sim` against `FlatGround`:

```
5,000,000 commands in 1.5–2.4 s single-core  →  ~2.5–3.3 M commands/s
a 45-second run (5,625 commands)             →  ~2 ms
```

**That figure is a floor, not a forecast.** `FlatGround` is one comparison per trace; a real brush
tracer over convex hulls with up to 5 slide-solver bumps and a step-up retry is plausibly 20–100×
more expensive. Planning conservatively at 100×, a 45-second run costs ~200 ms of one core:

- 10,000 players × 20 attempts/day = 200k verifications/day = 2.3/s average
- at 10× peak, 23/s × 200 ms = **~5 cores at peak**

One small machine, with headroom, at a player count this game is unlikely to reach soon. But it is
the resource an attacker can spend on your behalf, which is why §7.3 bounds it explicitly.

C4's trigger accumulation does not move this number, and it is worth saying why rather than
re-measuring: the work it adds inside a trace is one `u32` OR against a leaf the BSP descent already
visits, and one fraction comparison to clamp gathering to the traversed prefix. That is arithmetic
on data already in cache, in the same loop, and it is far inside the 100× factor above — which
exists precisely because the real tracer is unwritten. The number that would genuinely need
re-deriving is the tracer's own cost, and that measurement is not available until C8 exists.

---

## 5. Leaderboard data model

Postgres. Schema sketch; types and constraints are the design, names are negotiable.

### 5.1 Tables

```sql
-- Identity ------------------------------------------------------------------

create table players (
  id            uuid primary key,
  display_name  text not null,                  -- unique on lower(display_name)
  created_at    timestamptz not null default now(),
  country       char(2),
  banned_at     timestamptz,
  ban_reason    text
);
create unique index on players (lower(display_name));

-- One player, many OAuth providers (D3: Discord first, GitHub second).
create table identities (
  provider          text not null,              -- 'discord' | 'github'
  provider_user_id  text not null,
  player_id         uuid not null references players(id),
  handle            text,                       -- display only; providers let people change it
  avatar_url        text,
  linked_at         timestamptz not null default now(),
  primary key (provider, provider_user_id)
);
create index on identities (player_id);

-- Immutable physics facts ---------------------------------------------------

-- Every row is immutable. Tuning CPM inserts a row, it never updates one.
create table physics_profiles (
  id            int primary key generated always as identity,
  kind          text not null check (kind in ('vq3','cpm')),
  label         text not null,                  -- 'CPM (2026-08)' — shown to players
  digest        bigint not null unique,         -- PhysicsProfile::digest() (C6)
  profile_bits  bytea not null,                 -- exact f32 bit patterns, never decimal text
  layout_version smallint not null,
  created_at    timestamptz not null default now()
);

create table sim_builds (
  id                 int primary key generated always as identity,
  sim_version        text not null,
  git_sha            text not null,
  build_hash         bigint not null unique,
  native_verifier_ok boolean not null default false,  -- C2 cross-target check passed
  wasm_hash          bigint,                          -- the artifact the browser is served
  retired_at         timestamptz,
  created_at         timestamptz not null default now()
);

create table maps (
  id                  int primary key generated always as identity,
  slug                text not null unique,
  name                text not null,
  author              text,
  source_sha256       bytea not null,           -- the .map text (C7)
  source_key          text not null,            -- object-store key for the .map
  collision_digest    bigint not null,          -- the compiled hulls (C7)
  map_compiler_version text not null,
  has_start_trigger   boolean not null,
  has_finish_trigger  boolean not null,
  added_at            timestamptz not null default now()
);
create unique index on maps (source_sha256, collision_digest, map_compiler_version);

-- Runs ----------------------------------------------------------------------

create type run_status as enum
  ('pending','verified','did_not_finish','rejected','divergent','error');

-- Append-only. A run is never edited after verification; a re-verification
-- under different physics creates a new row (see 5.4).
create table runs (
  id              uuid primary key,
  player_id       uuid not null references players(id),
  map_id          int  not null references maps(id),
  profile_id      int  not null references physics_profiles(id),
  sim_build_id    int  not null references sim_builds(id),
  tick_rate_hz    smallint not null,

  status          run_status not null default 'pending',
  time_ms         integer,                       -- SERVER-COMPUTED. null unless verified.
  commands        integer not null,
  demo_sha256     bytea not null,                -- of the bytes; storage dedup and diagnostics
  run_digest      bytea not null,                -- canonical_digest() (C5) — the identity of the RUN
  demo_key        text,                          -- object-store key; null once pruned
  demo_bytes      integer not null,
  attempt_id      uuid references attempts(id),  -- the ticket this arrived under

  client_time_ms  integer,                       -- what the client displayed; diagnostic only
  client_rolling_digest bigint,                  -- folded over every command (§1.3)
  server_rolling_digest bigint,
  divergence_at   integer,                       -- first disagreeing command, once localised

  submitted_at    timestamptz not null default now(),
  verified_at     timestamptz,
  reject_reason   text
);
create index on runs (map_id, profile_id, time_ms) where status = 'verified';
create index on runs (player_id, submitted_at desc);

-- GLOBAL, not per-player. This is the constraint that makes a run belong to
-- whoever submitted it first; see §8.3. A per-player index here would be an
-- idempotency key and nothing more, and would let anyone re-post a demo they
-- downloaded from the leaderboard and have it rank as their own.
create unique index on runs (run_digest);
create index on runs (demo_sha256);

-- One live attempt per ticket. Issued on request, consumed by a submission.
create table attempts (
  id           uuid primary key,
  player_id    uuid not null references players(id),
  map_id       int  not null references maps(id),
  profile_id   int  not null references physics_profiles(id),
  issued_at    timestamptz not null default now(),
  expires_at   timestamptz not null,
  consumed_at  timestamptz,                      -- set when a run is accepted under it
  consumed_by  uuid references runs(id)
);
create index on attempts (player_id, issued_at desc);
create index on attempts (expires_at) where consumed_at is null;

-- Current personal best per category. Derived; rebuildable from `runs` alone.
create table leaderboard_entries (
  map_id      int not null references maps(id),
  profile_id  int not null references physics_profiles(id),
  player_id   uuid not null references players(id),
  run_id      uuid not null references runs(id),
  time_ms     integer not null,
  set_at      timestamptz not null,
  primary key (map_id, profile_id, player_id)
);
create index on leaderboard_entries (map_id, profile_id, time_ms, set_at);

-- Records that ever held first place, so history survives being beaten.
create table record_history (
  map_id     int not null references maps(id),
  profile_id int not null references physics_profiles(id),
  run_id     uuid not null references runs(id),
  time_ms    integer not null,
  held_from  timestamptz not null,
  held_until timestamptz,
  primary key (map_id, profile_id, run_id)
);
```

**`time_ms` is `integer` milliseconds, everywhere, including in every JSON response.** No column,
API field or TypeScript type in this platform is a duration in seconds.

### 5.2 The category key

A leaderboard category is **(map, physics profile)**. VQ3 and CPM are separate boards — they are
different games, which is exactly why `PhysicsProfile` is data.

Tick rate is *recorded and displayed* but is not part of the category key, subject to §11 decision
C. Note carefully that this is only defensible if the ranked rate is fixed: rate changes both the
physics (Q3's `com_maxfps` jump behaviour, which D2 deliberately reproduces) and the timing
resolution (C4). A board mixing 125 Hz and 250 Hz runs is not a fair comparison, it is two
overlapping games sharing a table.

### 5.3 Ranking

Boards are small (hundreds to low thousands of rows per map/profile), so rank is computed on read
rather than stored:

```sql
select rank() over (order by e.time_ms asc, e.set_at asc) as rank,
       p.display_name, e.time_ms, e.set_at, e.run_id
from leaderboard_entries e
join players p on p.id = e.player_id
where e.map_id = $1 and e.profile_id = $2 and p.banned_at is null
order by e.time_ms asc, e.set_at asc
limit $3 offset $4;
```

Ties break by **who got there first**, which is both conventional and the only tiebreak that cannot
be gamed. Ties will be frequent because C4 quantises times to the command duration; the UI should
show equal ranks rather than inventing an ordering.

### 5.4 The problem most leaderboard schemas ignore: physics changes

The CPM constants in `profile.rs` are marked `TODO(wave2): community-reconstructed`, and the
operator is expected to tune them against reference demos. **Every such change invalidates every
stored time.** The schema above is built so this is detectable and recoverable:

- `runs.profile_id` and `runs.sim_build_id` bind a time to the physics that produced it.
- Demos are inputs, so they can be re-simulated under new physics for the measured ~2 ms each
  (§4.3). Re-verifying 250,000 stored demos is well under an hour on one core.
- Re-verification under a new profile writes a *new* row, and that does not collide with the global
  unique index on `run_digest`: `canonical_digest` covers the `PhysicsIdent`, which covers
  `profile_digest`, so the same inputs under different physics are correctly a different run. The
  same property makes the digest a safe identity across a `sim_build` bump.

Two policies, and the choice is the operator's (§11, decision G):

- **Before public launch — re-verify and restate.** Run every stored demo under the new profile,
  update `time_ms`, rebuild `leaderboard_entries`, and tell players their times moved. Cheap, honest,
  and only tolerable while the player base is small.
- **After public launch — seasons.** A profile change opens a new board. Old boards freeze with
  their profile label attached and stay viewable forever. Nobody's record is silently rewritten.

Either way the machinery is the same, and it is machinery this schema already has. Retrofitting it
after the first tuning pass would mean a leaderboard whose entries cannot be explained.

---

## 6. Accounts, OAuth, sessions

D3: third-party OAuth only, Discord first, GitHub second, no passwords.

### 6.1 Flow

Authorization Code with PKCE. The backend is a confidential client and holds the secret; PKCE is used
anyway, because it costs nothing and removes the interception class of attack entirely.

```
GET  /auth/discord/start
     → generate state (32 random bytes) + code_verifier
     → store both in a short-lived, signed, HttpOnly cookie (10 min)
     → 302 to discord.com/oauth2/authorize
          ?client_id=…&response_type=code&scope=identify
          &redirect_uri=…&state=…&code_challenge=…&code_challenge_method=S256

GET  /auth/discord/callback?code=…&state=…
     → constant-time compare state against the cookie; reject on mismatch
     → POST to Discord's token endpoint, server-side, with the code_verifier
     → GET /users/@me with the access token
     → upsert identities(provider='discord', provider_user_id=…) → players
     → issue a session, clear the OAuth cookie, 302 to the return path
     → DISCARD the OAuth access and refresh tokens; we never call Discord again
```

**Scope is `identify` and nothing else.** Not `email`. We do not want an email address: not having
one removes a category of breach exposure and a category of obligation, and nothing in this product
needs to send mail. GitHub's equivalent is no scope at all, which yields the public profile.

Discarding the provider tokens is deliberate. Storing a refresh token means storing a credential to
someone else's account for a capability we never use.

### 6.2 Sessions

**Opaque random tokens, stored server-side. Not JWTs.**

```sql
create table sessions (
  id           bytea primary key,               -- sha256 of the cookie value
  player_id    uuid not null references players(id),
  issued_at    timestamptz not null default now(),
  expires_at   timestamptz not null,            -- 30 days, sliding
  revoked_at   timestamptz,
  user_agent   text,
  ip_prefix    inet                             -- /24 or /48, for abuse triage, not identification
);
```

Cookie: `s3_session=<32 random bytes, base64url>; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=…`

The reason not to use a JWT is specific to this product: **cheaters get banned, and a ban must take
effect now.** Stateless tokens make revocation an exercise in denylists that reintroduces the
database lookup you adopted JWTs to avoid. There is no scale argument on the other side — a session
lookup is an indexed primary-key hit on a table with one row per active player.

Only the hash of the token is stored, so a database disclosure does not hand over live sessions.

### 6.3 CSRF and origin

`SameSite=Lax` covers the top-level-navigation case. For state-changing requests the API additionally
requires `Origin` to match, and rejects requests without it. The client is same-origin and the
submission endpoint is not form-encoded, so this is sufficient and simpler than a double-submit
token. `POST /v1/runs` uses a custom content type, which independently prevents it being reached by
a simple cross-origin form post.

### 6.4 Anonymous play, and claiming a run afterwards

D4 puts a playable URL ahead of any backend, so the first thing that exists is a client with no
account. That shapes the account model rather than being an afterthought:

- With no session, runs are verified **locally** by the same `straf3-replay::verify` the server runs,
  and stored in IndexedDB with their `.s3d` bytes. The player has personal bests immediately.
- The finish screen offers "sign in to put this on the leaderboard". Signing in returns to the page
  with the demo still in IndexedDB, and it is submitted then.
- After signing in, the client offers to submit every stored local PB in one pass.

This works precisely because the demo is the run. There is no client state to reconcile, no "trust me
about my earlier time" — the bytes replay, or they do not. It is also why the local-PB path is not
throwaway work: it is the submission path minus one HTTP call.

**One awkward interaction, stated rather than glossed:** a run recorded before sign-in has no
attempt ticket, because there was no session to issue one. The claim path must therefore be exempt
from the ticket requirement of §3.1, and that exemption is precisely the path a copier would prefer.
It is tolerable because the ticket was never the load-bearing defence — the global uniqueness of
`run_digest` is (§8.3, step 1), and it applies to claimed runs identically. What the ticket buys is
resistance to *bulk* automated resubmission, so the claim path is bounded to match: a small cap on
claimed runs per account, allowed only within a window of the account's creation, and rate-limited.
A player who has genuinely accumulated dozens of local PBs before signing in is a good problem to
have and can be handled by raising the cap deliberately, not by leaving the path unbounded.

**A second interaction, sharper and narrower.** That cap is per-account, which silently assumes one
attacker is one account; sock puppets are cheap when the only gate is an OAuth handle. Combined with
first-submitter-wins ownership (§8.3 step 2), this opens an attack the §8.3 analysis does *not*
cover, because it runs in the opposite direction: rather than stealing a ranked run from the public
archive, an attacker who obtains a player's inputs **before that player submits** can claim them
first under a throwaway account, and the genuine player's own submission then fails with `409`.

This needs a capability §8.3 does not grant — the public archive only serves demos of runs already
verified and ranked, so by construction the honest submission is already on record. It requires a
different leak: a shared machine, a scraped IndexedDB store, a demo file passed to a friend. That is
a real but materially different threat, and it is called out here rather than folded into §8.3's
"bounded, not closed", which does not reach it.

Two things follow, neither of them cryptographic. **First-submitter-wins is a tiebreak convention,
not a proof of authorship** — it settles the common case cheaply and is not evidence in a dispute.
**So run ownership must be administratively reassignable.** The `runs` row carries `player_id` as a
mutable column, and a dispute is settled the same way every other dispute on this platform is
settled: by looking at the demos. The genuine player has the recording, its local IndexedDB
timestamps, and usually the surrounding attempts on the same map; the puppet account has one run and
no history. This is not automated and should not be — the cost of the attack is already high enough
that manual handling is proportionate.

### 6.5 Display names

The provider handle is a starting suggestion, not the identity. Players pick a `display_name` unique
on `lower()`, changeable with a rate limit, because Discord handles change and a leaderboard entry
should not silently rename itself. Provider handle and avatar are stored for display and refreshed
only at login.

---

## 7. The verification service

### 7.1 Shape

```
web/
  api/         axum: HTTP, OAuth, sessions, leaderboard reads, submission intake
  verifier/    separate binary: pulls pending runs, re-simulates, writes verdicts
  wasm/        wasm-bindgen wrapper over straf3-sim + straf3-replay + straf3-map
  frontend/    TypeScript, served static
```

Verification is a **separate binary from the HTTP server**. Re-simulation is unbounded CPU driven by
untrusted input; it does not belong in the same process as the request path, where it would compete
with the async runtime for cores and where a pathological input becomes an outage rather than a slow
job. Separating them also lets the verifier be sandboxed and scaled independently.

The queue is a Postgres table — `runs where status = 'pending'` claimed with
`for update skip locked`. There is no throughput case for anything more elaborate; §4.3 puts the
sustained rate in the single digits per second.

### 7.2 What intake does, and what the verifier does

**Intake (`POST /v1/runs`, in the API process)** does everything that is cheap and must be
synchronous, because a rejection is worth issuing before any CPU is spent simulating:

1. Require a session, and a live unconsumed `attempts` row whose ticket matches, whose
   `map_id`/`profile_id` match the recording's, and whose `expires_at` has not passed.
2. `decode` under `Limits` (C5) — parsing only, no simulation — and compute
   `canonical_digest` (C5).
3. Insert the `runs` row. The **global** unique index on `run_digest` decides what happens next, and
   the response depends on who owns the existing row:
   - no conflict → `202 Accepted`, queued for verification;
   - conflict, and the existing row is **this player's** → `200 OK` with that run. This is
     idempotency: a retried upload, or the same run re-encoded, returns the original;
   - conflict, and the existing row is **another player's** → `409 Conflict`, "this run has already
     been submitted". The run belongs to whoever got there first (§8.3).
4. Mark the attempt consumed, whichever of those happened.

**The verifier (separate binary)** then does the expensive half:

1. Fetch the `.s3d` from object storage; `decode` under `Limits` (C5).
2. Reject unless `PhysicsIdent` names a `sim_build` this verifier *is*, a known
   `physics_profiles` row, and a known `maps` row. Reject on any mismatch — do not "helpfully"
   substitute the current profile.
3. Compile the map (cached; keyed by `collision_digest`) and build the `World`.
4. Re-simulate every command through `straf3-replay::verify`.
5. **Take the time from `SimState.run`.** The client's declared time is stored in
   `runs.client_time_ms` for diagnostics and is never ranked, never compared against, and never
   shown as authoritative.
6. Compare **the rolling digest**, not the end state (§1.3). A mismatch sets `status = 'divergent'`;
   the checkpoint trail is then binary-searched to fill in `divergence_at`.
7. On `Finished`, write `time_ms`, and upsert `leaderboard_entries` if it beats the existing PB.

**The security property is not "we checked their time". It is "we computed the time; theirs was
never an input".** That distinction is what makes the checksum a diagnostic rather than a
gate — see §8.2.

### 7.3 Bounds, because this endpoint spends CPU on request

- `Limits.max_commands` = 150,000 (20 minutes at 125 Hz). Longer submissions are rejected at decode.
- Compressed body ≤ 1 MiB; decompressed ≤ 8 MiB; the zstd decoder is given an explicit window limit.
- A wall-clock deadline per verification (say 5 s); exceeded means `status = 'error'` and an alert,
  because at ~200 ms expected it means something is wrong, not slow.
- Bounded verifier concurrency — cores minus two, matching the existing throughput assumptions.
- Per-player rate limit (e.g. 30 submissions/minute, 500/day) and a per-IP limit ahead of auth.
- Idempotency is the global unique index on `run_digest` plus the ownership check in §7.2, not a
  client-supplied header: a retried or re-encoded upload of the same run returns the existing row
  rather than queueing work, and the same digest from a different player is a `409`.
- **Attempt tickets:** issued only to a signed-in session, TTL bounded (say 30 minutes — longer than
  `Limits.max_commands` allows a run to be, so it never truncates legitimate play), single use, and
  rate-limited themselves. A small cap on live unconsumed tickets per player stops a bulk harvest of
  tickets ahead of a bulk resubmission.

### 7.4 Build versioning

The browser client is served by us, so the client build is current within one deploy. The awkward
case is a run in progress when a deploy lands. Handling: keep the **previous** `sim_build`'s
verifier available for 24 hours, accept submissions naming either, and reject anything older with a
`409` explaining that the physics changed and the run cannot be ranked. This is honest and it is
rare; pretending an old build's run is comparable would not be.

### 7.5 API surface

```
GET  /v1/maps                                  list, with per-profile record times
GET  /v1/maps/:slug                             detail, spawn info, download key for the .map
GET  /v1/maps/:slug/leaderboard?profile=cpm&limit=&offset=
GET  /v1/maps/:slug/leaderboard/me              rank and time for the session's player
POST /v1/attempts                               start a run: returns a ticket (§3.1, §7.3)
POST /v1/runs                                   submit (§3.1)
GET  /v1/runs/:id                               status, and time_ms once verified
GET  /v1/runs/:id/demo                          the .s3d, for ghosts and playback — unauthenticated,
                                                but only once the run is verified and ranked (§8.3)
GET  /v1/players/:name                          profile: PBs, records held, totals
GET  /auth/:provider/start, /auth/:provider/callback, POST /auth/logout
GET  /v1/meta                                   current sim_build, profiles, wasm artifact hash
```

`/v1/meta` matters more than it looks: it is how the client discovers which physics it should be
running, and how a stale tab learns it must reload before its run can be ranked.

---

## 8. What replay verification does and does not buy

D1 chose server-side verification, correctly. Being precise about its limits is part of designing it.

### 8.1 What it defeats, completely

- **Edited times.** The time is computed, not accepted.
- **Modified client physics.** Faster gravity, more air control, a longer double-jump window — the
  server re-simulates under the profile the recording names and the run comes out however it comes
  out. A client with different constants produces a recording that either fails to finish or finishes
  slower than claimed.
- **Spliced runs.** The command stream starts at spawn and is continuous; there is nowhere to insert
  a better segment without simulating the join.
- **Teleporting, noclip, position editing.** There is no position in the format. Only inputs.

### 8.2 What the rolling digest is actually for

It is **not** an anti-cheat mechanism. An attacker computes a valid digest for their own inputs
trivially, and the server ignores the submitted value when computing the time anyway.

Its job is to distinguish two states that are otherwise indistinguishable and demand opposite
responses:

- *We agree.* The player's client and the server produced identical state at every command; the
  ranked time is the time they watched on screen.
- *We disagree.* Something is wrong: a determinism regression, an un-fixed float path (§1), a
  browser doing something unexpected, or a modified client. The checkpoint trail then localises it
  and `divergence_at` turns a mystery into a bug report.

Without it, a determinism regression looks exactly like ordinary run variance and would be found by
players noticing their times are wrong. **With only an end-state checksum it would be worse than
absent** — it would report agreement on runs that had diverged (§1.3), which is a false negative
presented as a positive assurance.

### 8.3 What it does not defeat, and cannot

**A bot.** A scripted input stream is a valid input stream. Replay verification proves a run is
*physically achievable under our physics*; it proves nothing about whether a human produced it. This
is inherent, not a gap in the design, and it should be stated to players rather than implied away.

Partial mitigations, with honest limits:

- **Input plausibility heuristics** — yaw traces with zero jitter, angular accelerations no hand
  produces, key transitions aligned to exact command boundaries every time. These catch naive bots
  and are trivially defeated by adding noise. Worth having; not worth trusting.
- **Human review of the top of each board**, using the demo playback the platform has anyway. A
  record run being watched by people who know the map is the strongest filter that exists, and it
  costs nothing to enable because ghosts are already a feature.
**Someone else's run, resubmitted as yours.** This deserves its own treatment rather than a bullet,
because the platform hands the attacker the artifact: ghosts and top-of-board review both require
`GET /v1/runs/:id/demo` to be public, and a `.s3d` re-simulates identically no matter who uploads
it. Nothing in the recording says who produced it, and — this is the part that has no fix —
**nothing can.** The artifact is inputs. Inputs are copyable, and any value the client computes over
them, including a digest folded around a server-issued nonce, the copier can compute too, because
the copier is running the same deterministic simulation on the same inputs. There is no client-side
computation that binds identity when the client is the adversary.

So the defence is not authentication. It is making the copy lose a race it starts behind in, and
making the perturbed copy visible:

1. **The run digest is canonical and globally unique.** `canonical_digest` (C5) hashes the decoded
   command stream, spawn and `PhysicsIdent` — not the file bytes. This matters: the bit-packed
   encoding has slack (an angle may be seeded absolutely or as a delta, the checkpoint trail is
   optional, there are reserved bytes), so a hash of the *file* is perturbable into a fresh value
   without altering a single simulated frame. Hashing the run instead means a re-encoded copy
   collides with the original, and §7.2 rejects it with `409`.
2. **First submitter owns it, and the honest player submits first by construction.** A demo only
   becomes downloadable once its run is verified and ranked, so the original row exists before the
   copy can be obtained. The thief is racing someone who has already finished.
3. **An attempt ticket is required (§3.1).** Resubmission therefore cannot be a batch job over the
   public archive; it costs a live, rate-limited, signed-in attempt per run.
4. **Near-duplicate detection over the canonical command stream.** A copier who perturbs the
   *inputs* — one flipped yaw LSB, or a dead command appended after the finish trigger, which
   changes nothing because the time is already latched — defeats step 1 by producing a genuinely
   different run. A similarity check over quantised yaw deltas catches this, and now has a
   well-defined thing to compare against: the canonical stream, not a compressed file. It will never
   be airtight.
5. **Human review at the top of each board**, which is what actually settles it.

State plainly what survives all five: someone who runs through a live attempt while feeding a
perturbed copy of another player's inputs can get a run ranked, and re-simulation cannot tell that
from a real run, in principle. Steps 1–3 reduce it from "trivial and scriptable" to "manual, per
run, and detectable"; step 4 raises the cost of evading detection; none of them make it impossible.
This is the same exposure every input-demo leaderboard has, Defrag included, and it is better
disclosed than papered over.

The realistic security posture for a niche movement game: **verification makes cheating require
writing a movement bot, which is a substantially more interesting project than editing a time, and
the people capable of it mostly want to post the bot rather than the record.** Design for that, keep
the demos so accusations can be settled by watching, and do not claim more.

---

## 9. Repository layout and build order

### 9.1 Layout (D5)

```
web/
  api/              Cargo: axum, tokio, sqlx, oauth2
  verifier/         Cargo: straf3-sim, straf3-replay, straf3-map
  wasm/             Cargo: cdylib, wasm-bindgen over sim + replay + map + render
  frontend/         TypeScript, Vite; imports the wasm package
  migrations/       sqlx migrations
  deploy/           container definitions, CI workflow
docs/web/           this document and its successors
```

**One Cargo workspace, not two.** The web crates become members of the root workspace. This is
uncomfortable — it puts tokio in the tree — but the alternative is worse: a second workspace means a
second `Cargo.lock`, which means the browser and the server can resolve different `glam` versions,
which means §1 happens again for a reason nobody is looking for. One lockfile is the only way "one
implementation of the physics" is true at the artifact level rather than just the source level.

The discomfort is handled by C11: `default-members` keeps the game crates as the default build, and
the seam check gains the web crates in `ABOVE_THE_LINE` so nothing below the line can reach them.
The existing denylist already forbids tokio *below* the line, and that stays exactly as it is — the
dependency runs the safe direction.

### 9.2 Build order, and a recommendation on ordering

Given that `straf3-game`, `straf3-platform` and `straf3-render` are all stubs — there is no window,
renderer or input path anywhere in the tree, and criterion 4 has been accepted unmet — the browser is
not a port of anything. It is a greenfield client, and so is the Windows one.

**Recommendation: build the browser first and treat it as the primary client.** The reasons are not
sentimental:

1. **`wgpu` and `winit` target web and native from one implementation.** The renderer that makes the
   browser playable *is* the renderer that makes the Windows binary playable. Choosing web-first
   throws away no work.
2. **The physics question is settled in the browser's favour, structurally.** The wasm module
   statically links its own maths and imports no trigonometry from the host, so no browser engine
   can introduce a divergence — the probe measured headless Chrome and Node as identical, and to
   native musl (§1.0). The browser is the *most* self-contained target we have, not the riskiest,
   and its current answers are the ones the glibc builds have to move onto (§1.2).
3. **This machine cannot evaluate a native client.** Under WSL2 the GPU path is `lavapipe` and
   presentation goes through software-composited RDP; `docs/environment.md` and the README both say
   anything it reports about frame pacing is fiction. A browser build opened in Chrome on Windows
   sidesteps that entirely, today, with no cross-compilation and no linker configuration.
4. **D4 already leans this way**, and a URL is a better artifact than an unsigned executable for
   everything from feedback to bug reports.

The risk this paragraph used to carry — that `wgpu`/`winit` might not cross-compile to web at a
tolerable size — **has been measured away.** A working WebGPU skeleton is 132 KiB of gzipped wasm
and it ran in Chrome 146 (§1.5). The residual — stage D, the game crates with `parry3d` expected to
arrive transitively — has since been weighed too, at 131 KiB: no larger than the skeleton, and with
no `parry3d` in it at all, that crate having been called by nothing and since removed.

**Order:**

| Step | Content | Depends on |
|---|---|---|
| 0 | **C1 + C2** — deterministic trig and the cross-target CI check | nothing |
| 1 | **C3, C4, C9** — 16-bit angles, run clock and triggers, `CommandPump` | C1 |
| 2 | **Playable URL, no accounts, no backend.** Static site, wasm sim, hardcoded arena, local PBs in IndexedDB | C9 |
| 3 | **C5, C6, C7** — `.s3d`, physics identity, `.map` ingestion | C4 |
| 4 | **Backend skeleton** — axum, Postgres, OAuth, players | D3 |
| 5 | **Submission, verification, leaderboards** | 3 + 4 |
| 6 | **Demo playback and ghosts** — the recorder and `verify` from step 2 and 3, rendered | 2 + 3 |
| 7 | **Open arbitrary `.map` in the browser** | C7 |

Step 0 is hours of work, touches three lines of `step.rs`, and every later step is wrong without it.
Step 2 is the artifact rev 5 §M values most and it needs no backend at all.

**Step 2 assumes rev 6 §T resolves to the hardcoded arena**, which is the answer this ordering wants
and the one rev 6 recommends. The operator's standing decision to import `.map` files rather than
author them governs the *geometry layer* and is not in question here; §T asks only whether the first
playable thing waits for that pipeline. If it does, step 2 moves behind C7 and the browser-first
argument loses its best property — that it produces something to feel within days. Two ramps matter
more than they sound: they are where CPM and VQ3 diverge most, and they are the discrete branches
the 1-ULP finding in §1 warns about, so a hardcoded arena is also the cheapest place to find out
whether that warning was real.

### 9.3 Deployment consequences the operator already flagged

Rev 5 §O anticipated this: deploying a website needs a git remote, and pushing ~700 MiB of build
artifacts through `history-slim` first. Both remain deferred decisions; nothing before step 4 needs
either. Flagging that step 4 is where they stop being deferrable.

---

## 10. If a further divergence appears — and the insurance to build anyway

§1 makes the good case a measured result rather than a hope, and C1 is decided. This section is
therefore no longer about the original question — it is about the *next* divergence: a collision
implementation with a target-specific path (C8), a glam SIMD type creeping in below the seam (C10),
a future target that is not IEEE-conforming in some corner.

One candidate is now firmly off the list: **a browser engine will not surprise us.** §1.0's mechanism
— the wasm module statically links its own maths and imports no trigonometry — is what makes that a
structural property rather than a lucky measurement, and the probe confirmed it on headless Chrome
as well as Node.

**The failure is not "no anti-cheat".** Server-authoritative re-simulation still works with a
divergent client: the server computes a time and ranks it. The failure is subtler and worse for the
product — **the time the player watched is not the time they got.** A run that finishes at 42.13 on
screen ranks at 42.31, or does not rank at all because the server's simulation missed a jump the
player made. A leaderboard that disagrees with the game is not a leaderboard.

Fallbacks, in the order I would take them:

1. **Verify by executing the wasm module, not the native build.** Run the *same* `.wasm` artifact the
   browser runs, inside `wasmtime`, in the verifier. This is bit-identical to the client **by
   construction**, because it is the client's binary: wasm's semantics for f32/f64 arithmetic are
   fully specified and deterministic, so any conforming runtime produces the same bits (the only
   spec-sanctioned nondeterminism is NaN payloads, and a `SimState` containing a NaN is already
   broken). It costs a `wasmtime` dependency and roughly 1.5–3× the CPU of native — against §4.3's
   200 ms budget, entirely affordable.

   This is compatible with D2's intent: Q3 physics still exists exactly once, as one crate, compiled
   from one source. Only the execution engine differs. **Build the verifier behind a
   `trait RunVerifier` with `NativeVerifier` and `WasmVerifier` implementations from the start,** so
   this is a configuration change rather than a rewrite. That is the cheapest insurance in this
   document and I would buy it regardless of C1.

2. **Fixed-point arithmetic.** `num.rs` exists precisely for this, and it is the total solution:
   every target identical by construction, and no trig question at all. It is also the most
   expensive, and it carries a real risk the project should not discover late — **overbounce, ramp
   boosts and edge clipping are partly artefacts of f32 rounding.** `num.rs`'s own doc comment says
   widening to f64 would "quietly file them off"; moving to fixed-point could too. This changes the
   game, and it is a movement-feel decision before it is an arithmetic one.

3. **Hybrid trust — verify only the top N.** Explicitly what D1 rejected. Listed for completeness,
   not recommended: it fails at exactly the runs anyone cares about.

**Note that C1 is required even if fallback 1 is adopted.** The wasm-verifier route makes browser
runs verifiable, but the native Windows client (§1.1(d)) still disagrees. If a time set in the native
client is ever to appear on the same board as a browser time, the trigonometry has to be shared.

**And the tempting non-fix is worth naming once more:** matching the server's libc to the client's
(deploying on musl) makes today's checksums agree without fixing anything. It survives exactly until
someone changes the base image, and it fails silently when they do. §1.0.

---

## 11. Decisions for the operator

Everything below is a real fork where I have a recommendation but not the authority. Nothing here
blocks drafting; all of it blocks building.

Three things that are *not* on this list because they are already settled: the trigonometry fix
(C1 — decided by the probe session, pre-verified, scheduled after Wave 2), the choice of `.map`
import over map authoring (decided by the operator), and the web bundle shape (rev 6 §Q2 — WebGPU
only, no compiled-in fallback, no `egui`; C9 states it, and it is recorded here so it is not
re-litigated during Wave 3).

**A — Amend the determinism scope from "same binary, same machine" to "same source, any target".**
*Recommend: yes.* Rev 1 deferred cross-platform bit-exactness; verified leaderboards need it, and
§1 shows the current code does not have it — including between Linux and Windows, independent of any
web work. C1 delivers it; this amendment is what makes C2's CI check a requirement rather than a
nicety, and what stops the property being quietly lost again. It is the one specification change
this document asks for.

**B — Adopt the rolling-digest rule as a project-wide invariant, not just a format detail.**
*Recommend: yes.* §1.3 shows an end-state checksum certifying a run that had diverged. The same
trap exists in `tests/determinism.rs`, which compares final states and would have reported those 29
divergent commands as agreement. Making "compare the fold over every command, never the end state"
the rule everywhere — tests, CI check, format, verifier — costs a few lines and closes the class.

**C — Fix the ranked tick rate at 125 Hz, at least initially.** *Recommend: yes.* Rate changes both
the physics and the timing resolution (§5.2, C4), so a mixed-rate board compares runs that are not
comparable. Record the rate always; refuse to rank anything but 125 Hz until there is a reason.
Alternative: separate boards per rate, which triples the board count for a distinction most players
will not understand.

**D — Adopt C3's stronger form (16-bit angles inside `UserCmd`) or the weaker one (constructors
only).** *Recommend: the stronger form.* It is what Q3 did, it makes recordings exact by
construction, and doing it later means re-recording every fixture. It costs a day and touches tests
Wave 2 just wrote — which is exactly why the decision wants making now rather than in three weeks.

**E — Confirm browser-first as the primary client.** *Recommend: yes*, for the four reasons in §9.2.
This was contingent on the render-stack half of the rev 5 probe; **that contingency has resolved in
favour of the browser** — 132 KiB of gzipped wasm for a WebGPU skeleton running in Chrome 146
(§1.5). The bundle-shape question underneath it is settled by rev 6 §Q2 and is not reopened here:
WebGPU only, backend chosen in JS before entering wasm, WebGL2 only ever as a separate on-demand
bundle (C9). What is left for you is only the ordering call, and if it goes the other way the
ordering changes while the contract does not.

**F — Map content licensing.** *No recommendation; I cannot resolve this.* `.map` files reference
textures in `.pk3` archives whose redistribution rights vary and sometimes inherit id's. Hosting the
Defrag corpus for browser download is a legal question, not an architectural one. It does not block
anything before step 7, and the first client should render untextured geometry regardless because
texture download would dominate the bundle.

**G — Physics-change policy: re-verify and restate, or seasons.** *Recommend: re-verify while
pre-launch, seasons after.* The CPM constants are explicitly unverified and will be tuned; §5.4 has
the machinery for either, but only if it is in the schema from the start.

**H — Demo retention for non-PB attempts.** *Recommend: discard the bytes, keep the times.* §4.2
shows keeping everything costs about $19/month at 10,000 players, so this is a product question —
does anyone want to re-watch a failed run? — and not a cost one.

**I — Confirm the anti-copy posture in §8.3, which trades a little friction for the only defence
that exists.** *Recommend: yes.* Publishing demos is what makes ghosts and top-of-board review
possible, and it is also what hands a copier a working run. There is no cryptographic fix — the
artifact is inputs, and the client is the adversary (§8.3). The design answers with global
uniqueness on the canonical run digest, an attempt ticket per submission, and near-duplicate
detection, which together make a copy lose a race and cost a manual interaction. The friction you
are approving is small but real: players must call `POST /v1/attempts` before a ranked run, and
claimed anonymous runs are capped per account (§6.4). The alternative — keeping demos private —
costs ghosts, playback and human review, which are worth more than the exposure. If you would rather
not carry even that friction, say so and the ticket drops out; the digest uniqueness, which is the
load-bearing part, stands without it.

---

## Appendix A — reproducing every measurement

These are **my** experiments. The authoritative determinism evidence — six builds, headless Chrome,
the 977-angle sweep, NaN payloads, and the verified Cody-Waite implementation — lives at
`probes/wasm-determinism/` with `run-all.sh` reproducing it, and should be read first. What follows
reproduces the narrower design-facing measurements: the divergence onset in wall-clock terms, the
throughput figure, and the demo-size figures, none of which the probe was scoped to produce.

All of it ran on the WSL2 box described in `docs/environment.md`, with the pinned toolchain
(`1.97.1`) and `node v22.22.1`. **The repository was not modified**; every probe was built in `/tmp`
against the crate by path, and the trig experiment used a copy.

**1. `straf3-sim` compiles to wasm unchanged**

```sh
rustup target add wasm32-unknown-unknown          # already present on this box
CARGO_TARGET_DIR=/tmp/t cargo +1.97.1 build --release \
  --target wasm32-unknown-unknown -p straf3-sim --lib
```

**2. std vs `libm` divergence** — a `/tmp` crate depending on `libm = "0.2.16"`, sweeping
`deg ∈ [-180, 180)` at 0.001° and comparing `(deg * PI*2/360).sin_cos()` against
`libm::sinf`/`cosf` bit for bit. Result: 4,629 / 359,387 sines differ (1.288%), max 1 ulp.

**3. The browser's implementation** — build any `x.sin()` as a `cdylib` for
`wasm32-unknown-unknown` and read the strings:

```sh
strings target/wasm32-unknown-unknown/release/*.wasm | grep sinf
# _RNvNtNtNtCs…17compiler_builtins4math9libm_math4sinf4sinf
```

**4. Native vs wasm checksums** — a `/tmp` crate with a `path` dependency on `crates/straf3-sim`,
exporting `probe_checksum(n: u32) -> u64`, which builds the command stream from
`tests/determinism.rs` and returns `run(..).checksum()`. Built for native and for wasm; the wasm
module runs under Node with no imports:

```js
import { readFileSync } from 'node:fs';
const { instance } = await WebAssembly.instantiate(readFileSync(process.argv[2]), {});
console.log('0x' + BigInt.asUintN(64, instance.exports.probe_checksum(Number(process.argv[3])))
                     .toString(16).padStart(16, '0'));
```

Bisecting `n` between 500 and 2,000 gives last-agreeing 1,790 and first-diverging 1,791.

**5. Corroborating the diagnosis** (not the chosen fix — see §1.2) — `cp -r crates/straf3-sim
/tmp/simfix`, replace the three `.sin_cos()` calls with `libm::sinf`/`libm::cosf`, add
`libm = "0.2.16"`, repoint the probe at the copy, and rebuild for `x86_64-unknown-linux-gnu`,
`x86_64-pc-windows-gnu` (run under WSL interop) and `wasm32-unknown-unknown`. All three return
`0x037ac3485af83b4b` at n = 400,000. What this establishes is *sufficiency*: replacing only those
three calls, and nothing else in the crate, is enough. The owned Cody-Waite implementation C1
actually adopts was built and verified by the probe, not here.

**6. Throughput** — the headless runner's input format supports a repeat count, so one line is a
whole run with no parse cost:

```sh
printf 'rate 125\nprofile cpm\nworld flat 0\nspawn 0 0 64\nyaw 90\ncmd 5000000 127 64 0 - 0 91.37\n' \
  > /tmp/pure5M.txt
time /tmp/t/release/straf3-headless /tmp/pure5M.txt
# 1.5–2.4 s for 5,000,000 commands, single core, against FlatGround
```

**7. Demo encoding** — a Python implementation of the §3.2 bit-packer over synthetic strafejump
streams at three mouse-jitter levels, measuring bit-packed size and then `zlib` level 9. Table in
§4.1.

---

## Appendix B — what this document assumes, and what could still invalidate it

**Verified here, first-hand:**

- `straf3-sim` builds for `wasm32-unknown-unknown` unmodified.
- glibc-native and wasm-in-V8 diverge at command 1,791 on the crate's own determinism stream —
  14.3 s of play.
- The cause is `sin_cos`; nothing else in the crate calls a transcendental.
- The wasm module resolves `sinf` to `compiler_builtins::math::libm_math`, i.e. it imports no
  trigonometry from its host.
- Replacing only those three calls makes three targets agree through 400,000 commands.
- Linux and Windows native builds already disagree on `sin_cos`.
- `RunState::start` / `finish` are called from nowhere in the workspace.
- Every determinism assertion in `tests/determinism.rs` compares final-state checksums, including
  the cross-process one — which is the gap §11 decision B addresses.
- Throughput and demo-size figures, as measured above.

**Taken from probe session `as_8c4119ef80d54651b279`, not re-verified here** (its evidence is at
`probes/wasm-determinism/`, which is not visible from this worktree):

- Native musl, wasm-in-Node and wasm-in-headless-Chrome-146 are bit-identical on every measurement;
  glibc is the sole outlier.
- 1 ULP on 12/977 sin and 4/977 cos angles; `sqrt`, `abs`, `min`/`max`/`clamp` and
  `VectorNormalize` identical across all builds; no FMA contraction, no x87 excess precision.
- The Cody-Waite + Taylor implementation produces identical per-command and final checksums on all
  four builds, within 1 ULP of libm.
- The reconvergence case: one run where the final checksum matched across builds while 29 of 1,200
  intermediate per-command checksums did not. §3.2's rolling digest exists because of this
  measurement, so if it is wrong, that field is over-engineering — an 8-byte one.
- The render-stack figures: 132 KiB / 727 KiB / 2.50 MiB gzipped wasm for stages A/B/C, measured
  with `stat` and `gzip -9`, with stage A running in Chrome 146; that `wgpu` crashes rather than
  degrading when both backends are compiled in and `requestAdapter()` returns null; and that
  `egui-winit` 0.36.1 cannot compile for `wasm32`. Relayed via spec rev 6 §P2/§Q2, whose full report
  is `artifact_0809bd0014d447c08766`.

**Assumed, and stated as assumptions:**

- Real brush-tracing collision costs 20–100× `FlatGround`. Guessed from the shape of the algorithm,
  not measured — the tracer does not exist. If it is 1,000× the verification budget in §4.3 needs
  revisiting, though the conclusion (one small machine) probably survives.
- Player-population figures in §4.2 are illustrative. The conclusion is robust across two orders of
  magnitude, which is why the table spans them.
- A 45-second median run length, from the Defrag corpus's general character.
- That `wasmtime` and V8 agree on wasm f32 semantics. This follows from the wasm specification
  rather than from a measurement I made, and it is only load-bearing under §10 fallback 1.

**Could still invalidate parts of this:**

- ~~**Stage D — the game crates plus `gltf`, with `parry3d` arriving transitively.**~~ **RESOLVED,
  and it did not invalidate anything.** It sat here unmeasured because the first probe seat's
  worktree was destroyed before it got there. Measured since: 131 369 B gzipped wasm, marginally
  under stage A's skeleton (`probes/wasm-render/sizes.txt`). `parry3d`, named here as the largest
  unknown, weighed ~0 — `straf3-collision` traces by hand and never called it — and has since been
  removed from the workspace. No dependency question for the web build, no change to C9 or §9.2.
- **That the ticket and digest mitigations in §8.3 are worth their friction.** They rest on a
  judgement about attacker economics, not a measurement: that requiring a live attempt per
  submission moves copying from scriptable to manual. If copying turns out to be popular anyway,
  the answer is human review and near-duplicate detection, not more protocol.
- **A divergence in the collision layer** once it exists (C8) — the same class of bug as §1, in code
  not yet written. C2 is the defence.
- **Wave 2's `pmove_msec` sub-stepping**, noted as a TODO at `step.rs:147`. It changes results, so it
  must land before any reference demo is recorded. The format handles it (per-command durations are
  representable), but every demo recorded before it is invalidated by it.
</content>
</invoke>

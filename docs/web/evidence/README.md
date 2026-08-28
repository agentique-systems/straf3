# Evidence for the web wave

Artefacts, and the commands that reproduce them. Nothing in this directory is
a claim on its own — each file exists because a document elsewhere makes an
assertion, and this is where the thing the assertion is about lives.

The rule `README.md` states applies here in reverse. Numbers do not belong in
prose, so prose points at commands; this directory is where a command's output
is kept when the output is the deliverable.

---

## `r6-*` — a browser-recorded run re-simulates natively to the same rolling digest

Requirement r6, and the criterion `PLAYING.md`'s "Not proven yet" names as what
would retire its largest gap.

| file | what it is |
|---|---|
| `r6-native-subject.txt` | the harness run against a **natively** produced `.s3d`. Not r6 — this is the instrument being shown to work on a subject with a known answer. |
| `r6-selftest.txt` | the harness shown to be able to *fail*: one acceptance and four refusals, exit statuses asserted. |
| `r6-browser.s3d` | the run recorded **in the browser**, if one exists. Absent means no browser run has been captured; see `PLAYING.md`. |
| `r6-browser.txt` | the comparison for that run. |

**Still absent as of the browser wave.** `r6-browser.s3d` was not captured, and
the reason is worth recording because it is not a browser limitation.
`RunSink::finished` fires only on crossing the finish trigger, and `coil` cannot
be walked to that trigger: its last jump clears 288 units onto a ledge whose
front face is 272 units tall and unclimbable, so the run needs ~425 ups, and the
ramp wave before it needs ~575 ups to avoid the speed trap coil's own header
documents. A scripted `drive.mjs` run reaches 470 ups and the ramp wave, and
stops there. What is missing is a *player*, not a capability —
`r17-browser.txt` §3 shows the client taking pointer lock, turning with the
mouse and strafejumping past ground speed under that same driver.

---

## `r17-*` — straf3 runs in a browser, on a real GPU, at a URL

| file | what it is |
|---|---|
| `r17-browser.txt` | the transcript: URL, Chrome version, WebGPU adapter, bundle size, map compile, frame pacing in two windows with the host contention measured beside each, and the swiftshader flag comparison. |
| `r17-browser-window.png` | coil's strafe corridor, rendered by the wasm build through WebGPU and captured out of the browser window. |

Reproduce:

```sh
crates/straf3-game/web/build.sh
node crates/straf3-game/web/serve.mjs 8790 &
CHROME="C:/Program Files/Google/Chrome/Application/chrome.exe" \
CHROME_FLAGS="--no-first-run --no-default-browser-check" \
  node crates/straf3-game/web/drive.mjs \
       crates/straf3-game/web/steps/r17-evidence.json --headful
```

**`CHROME_FLAGS` must be overridden.** `drive.mjs` defaults it to
`--enable-unsafe-webgpu --use-angle=swiftshader`, written for a GPU-less WSL2
box. Measured here (§5 of the transcript): with those flags Chrome offers **no**
WebGPU adapter at all on this host and the client refuses — so the default fails
loudly rather than silently recording on software, but it still tests nothing.

**Why the adapter is named from `adapter.info` and not from the render log.**
`crates/straf3-render/src/gfx.rs` prints wgpu's `AdapterInfo`, and on the WebGPU
backend that is `adapter="" type=Other` — the WebGPU spec exposes neither an
adapter name nor a device type to wgpu, so that line structurally cannot
identify the GPU in a browser. `navigator.gpu.requestAdapter()` then
`adapter.info` gives `vendor "nvidia" / architecture "ampere"`, which is what a
stock Chrome will say about a GA104.

**Pacing numbers carry their contention.** Both windows are published: a quiet
one (p99.9 = 7.3 ms, 0.05 % of frames over budget) and one taken while the host
sat at 100 % CPU (p99.9 = 24.3 ms, 0.36 % over). The median is 6.1 ms in both.
A pacing number without the contention beside it is not a measurement, and
`probes/pacing`'s contention watcher was never committed, so `typeperf` by hand
is the instrument.

Reproduce:

```sh
# the instrument, on a native subject
tools/straf3-webcheck/selftest.sh

# a run, browser or native
tools/straf3-webcheck/target/debug/webcheck resim <run.s3d>
```

**What is compared, and why it is not the obvious thing.** The rolling digest,
folded over every command's state checksum in order — not the end-state
checksum. `docs/web/ARCHITECTURE.md` §0 item 4 records a measured run whose
final checksum matched across builds while 29 of its 1,200 intermediate
checksums did not; an end-state comparison would have called that run
identical. `webcheck` prints the end-state comparison too, beside the rolling
one and labelled insufficient, so the case where the two verdicts differ is
visible rather than argued.

**What makes it a cross-implementation check.** The state checksums are
produced by a native x86-64 build stepping `straf3-sim` against a natively
compiled `assets/maps/coil.map`. The digest in the file's header was folded by
a `wasm32-unknown-unknown` build in a browser, stepping its own compilation of
the same source. Two implementations, one number. **That is the whole of the
cross-implementation claim** — the browser's wasm build folded a digest, the
native build re-folded one from its own stepping, and the two agree.

**What `--expect-digest` does, and what it does not.** It establishes
*provenance*: the browser reports its run digest to the page through
`onRunFinished`, out of band from the file, and passing that value binds this
file to that browser-reported run. That is worth having here for a mundane
reason — `webcheck from-text` produces a native `.s3d` on the same map under the
same profile that also agrees and exits 0, so the two artefacts are not
otherwise distinguishable from the report alone.

It does **not** prove the browser ran the simulation, and no write-up should say
it does. Both numbers come from a single `Recording::claimed()` inside one wasm
call, so a header and an out-of-band digest fabricated together would agree.
The check that does catch a fabricated header lives in
`crates/straf3-replay/src/codec.rs`, which re-folds the run digest from the
file's own per-command checksums at load time and refuses with
`DigestNotDerivedFromTrace`.

**And that check is gated on the trace being present.** It sits inside the
`if trace_present` arm, so a run written with `to_bytes()` instead of
`to_bytes_with_checksums()` gets no derivation check at all. The trace is not
merely how a divergence is *localised* to a command index — it is what makes
the anti-tamper rule apply to the file in the first place. That is why
`PageRunSink::finished` writes the trace, and why a traceless evidence run
should be treated as a bug rather than shipped.

---

## `r8-map-compiler-crosstarget.txt` — the map compiler agrees across targets

| file | what it is |
|---|---|
| `r8-map-compiler-crosstarget.txt` | `assets/maps/coil.map` compiled twice — once by a native `x86_64-pc-windows-msvc` build, once by a `wasm32-unknown-unknown` build inside Chrome — producing the same collision digest `0x47263b8845d8bb4b`, 26 hulls, 4 triggers. |

Worth its own entry because it is **not** what r19 asks for and does not need a
recorded run to exist. `cargo xtask determinism` builds `straf3-det-runner` and
never compiles a `.map`, so the Valve 220 pipeline had no cross-target evidence
on any host — while being load-bearing for every run, since a `.s3d` binds a
`collision_digest` and `commands_for` refuses geometry that compiled
differently. `crates/straf3-game/src/web.rs` names exactly this gap on
`install_map`. This is the first host whose browser would start, so it is the
first time the browser's answer could be read next to a native one.

r19 remains the rolling digest of a recorded run, and still requires a run
recorded in a browser.

---

## `r12-*` — native play is not weakened by any of this

| file | what it is |
|---|---|
| `r12-baseline-before.txt` | `cargo xtask check-seam` and the four-target `cargo xtask determinism`, captured against the untouched tree at `a0e62d4` before any of this wave's work landed. |
| `r12-after.txt` | the same two commands, re-run afterwards. |

Reproduce, and compare:

```sh
cargo xtask check-seam
cargo xtask determinism
tools/straf3-webcheck/target/debug/webcheck physics
```

`webcheck physics` prints `PhysicsProfile::digest()` for every named profile by
running the code that defines it. That is deliberate: r12 asks whether the
physics digest moved, and a number transcribed into a document cannot answer
that question — run the command before and after and diff the two.

Two hazards, both of which have bitten this repository:

- An inherited `CARGO_TARGET_DIR` makes `cargo xtask determinism` fail as
  though determinism had broken. Unset it before believing a failure.
- A scale literal like `s(1000.0)` inside `crates/straf3-sim` can trip
  `check-seam` even in a test.

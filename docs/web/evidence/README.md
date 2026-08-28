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
the same source. Two implementations, one number. `--expect-digest` closes the
last gap by checking the file's header against the digest the browser reported
to the page out of band, so a header written by a code path that never ran the
simulation cannot pass.

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

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

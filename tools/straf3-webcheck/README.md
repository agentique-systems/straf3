# `straf3-webcheck` — stand the ecosystem up, and check what it produced

Two jobs that belong together because one exists to test the other.

1. **`ecosystem.sh`** stands the whole web ecosystem up on one origin, and
   takes it down again leaving no orphan process.
2. **`webcheck`** takes a `.s3d` recorded by the browser, re-simulates it
   natively, and compares the rolling digest folded over every command.

---

## Standing it up

```sh
cp tools/straf3-webcheck/env.example .env      # then fill it in; it is gitignored
tools/straf3-webcheck/ecosystem.sh up
tools/straf3-webcheck/ecosystem.sh status
tools/straf3-webcheck/ecosystem.sh logs
tools/straf3-webcheck/ecosystem.sh down
```

`up` builds the browser bundle, starts both records-service binaries and the
site, and waits for each port before reporting it. Everything a browser touches
is under `http://localhost:8787`; the records service is on `127.0.0.1:8788`
and no page ever addresses it.

| path | served from |
|---|---|
| `/v1/*` | proxied to the records service |
| `/client/*` | `crates/straf3-game/web/pkg` |
| `/assets/maps/*` | `assets/maps` |
| everything else | `web/site` |

Three properties worth knowing about, because each is a decision rather than an
accident:

- **The credential is never in this directory.** `ecosystem.sh` reads the
  gitignored `.env` at the repository root and hands `DATABASE_URL` to the
  service through its environment, where `sqlx` reads it from anyway — so it
  reaches no command line, no process title and no log. `status` prints which
  variables are *set*, never their values. `.env` is read as **data**, not
  sourced: a backtick inside a password is a character, not a command.
- **A missing piece is reported and skipped, not fatal.** The site without a
  records service is a genuinely useful state — it is the `no_records_service`
  503 the site must render as *unanswerable* rather than as an empty
  board — so `up` produces it deliberately when the service is absent.
- **`down` kills process groups, not pids.** The service runs under `cargo
  run`; killing the recorded pid would kill cargo and leave the binary it
  spawned holding port 8788. Each process is started with `setsid` so the whole
  tree goes. `down` then checks both ports really are free and says so loudly
  if something it does not own is still listening.

On this WSL2 box Chrome needs `--enable-unsafe-webgpu --use-angle=swiftshader`
before it offers a WebGPU adapter at all, and it will be slow. Nothing timed on
a software-only adapter says anything about frame pacing or latency.

---

## Checking a run

```sh
cd tools/straf3-webcheck && cargo build && cd ../..
tools/straf3-webcheck/target/debug/webcheck resim <run.s3d> [--expect-digest <hex16>]
```

Exit status is 0 only when every comparison agreed, so this is a gate and not
only a report.

It compares the **rolling digest** — folded over every command's state checksum
in order — and not the end-state checksum. `docs/web/ARCHITECTURE.md` §0 item 4
records a measured run whose final checksum matched across builds while 29 of
its 1,200 intermediate checksums did not, so an end-state comparison would have
called that run identical. Where the file carries a checksum trace, every
intermediate checksum is compared too and the *count* of disagreements is
reported alongside the first diverging index. The end-state comparison is
printed as well, labelled insufficient, so the case where the two verdicts
differ is visible rather than argued.

**Record evidence runs with `Recording::to_bytes_with_checksums()`, not
`to_bytes()`.** Without the trace the rolling fold is still sticky, so a
disagreement is still *detected* — but it cannot be *localised*, and the
diverging command index is the finding worth having. It costs eight bytes a
command.

### Other commands

```sh
webcheck from-text <fixture.txt> --map <file.map> --out <run.s3d>
```

Converts one of `straf3-game`'s text recordings into a `.s3d` carrying the
trace. This is how the harness gets a native subject with a known answer: a
harness first exercised on the artefact it was built to judge has never been
shown to work.

```sh
webcheck physics
```

Prints `PhysicsProfile::digest()` for every named profile by running the code
that defines it. For r12: run it before and after and `diff`, rather than
trusting a number transcribed into a document.

---

## Showing it can fail

```sh
tools/straf3-webcheck/selftest.sh
```

One acceptance and four refusals, with each exit status asserted: a digest the
recording does not claim, one flipped byte, a map recompiled to different
hulls, and a harness whose dependencies resolved differently from the
workspace's. That last one is not hypothetical — it fired on this tool's first
ever build, catching `glam 0.33.4` against the workspace's `0.33.3`.

The one control not here is a run manufactured to disagree with its own header,
which would need a forgery tool this repository is better off without. That
path is pinned instead by `crates/straf3-replay/src/tests.rs`
(`a_divergence_is_reported_with_the_command_it_started_on`, and its
no-trace sibling), which is where it belongs — `webcheck` only prints what
`Recording::verify` returns.

---

## Why this crate is standalone

The empty `[workspace]` table in `Cargo.toml` excludes it from the root
workspace, the pattern every crate under `probes/` uses. `tools/det-runner` is
a *member* and says why: so it resolves through the workspace `Cargo.lock`,
because "a check built against a different resolution would be verifying a
different tree".

That reasoning is right, and it is answered rather than ignored. `webcheck`
reads both lock files on every run and refuses to report any verdict when a
shared package resolved differently — which is strictly stronger than the
membership it stands in for, because a member crate shares a lock file and
never says so out loud.

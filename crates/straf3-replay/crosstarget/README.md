# `.s3d` on four targets — criterion 7's evidence

`verify.sh` builds `straf3-replay` for `x86_64-unknown-linux-gnu`,
`x86_64-unknown-linux-musl`, `x86_64-pc-windows-gnu` and
`wasm32-unknown-unknown`, runs its cross-target report on each, and fails
unless the four reports are identical apart from the two lines naming the
target they ran on.

```
crates/straf3-replay/crosstarget/verify.sh
```

## What it measures

Criterion 2 already proves that a reference command stream *compiled into the
binary* produces the same bits everywhere. This is the other half: a run that
arrived as **bytes**. Between a saved run and its replay on another machine sit
an encoder, a decoder, a length field, a UTF-8 string, twenty-two `f32` bit
patterns and a `usize` that is 64 bits on three of these targets and 32 on the
fourth.

So each target does the whole path — record, encode, decode, re-simulate — and
publishes every number that has to agree:

| published | what a disagreement would mean |
| --- | --- |
| `compact_len`, `compact_content` | the encoder wrote a different file for the same run |
| `traced_len`, `traced_content` | ditto, for the form that carries per-command checksums |
| `digest` | re-simulating the **decoded** recording produced a different run |
| `sim_time_ms`, `run_time_ms` | the time differs — criterion 7's headline claim |
| `round_trips` | bytes → `Recording` → bytes is not the identity on that target |
| `verifies` | the decoded recording did not reproduce what it claims |
| `refuses_stale` | a recording bound to other geometry was **accepted** (C6) |
| `checksums N ...` | all 400 per-command state checksums, so a divergence names its command |

There is no golden value. The check is relative: four targets against each
other, so a legitimate physics change re-numbers everything and no fixture
needs re-recording.

## Result, 2026-08-16

All four agree, byte for byte, on the whole report — 21 KB of text including
every one of the 1 200 per-command checksums:

```
grand 60596f922702f7d2   all-ok true
```

The Windows report was produced by running the real `.exe` through WSL
interop against the real Windows loader, not under an emulator — `verify.sh`
prints a warning and falls back to wine only if the direct run fails, and it
did not. The wasm report was rendered *inside* the `.wasm` and copied out of
linear memory by `run-node.mjs`, so V8 did no formatting of its own; the module
imports nothing.

The committed reports in `results/` are that run's output.

## Reproducing it

Needs the four `rustup` targets and `node`. Roughly 30 s from cold. If a target
is missing the script says which and exits non-zero rather than quietly
comparing three.

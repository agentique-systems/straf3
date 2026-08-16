#!/usr/bin/env bash
#
# Criterion 7's cross-target check for the .s3d format.
#
# Builds `straf3-replay` for all four targets, runs its cross-target report on
# each, and fails unless every report is byte-identical apart from the two
# lines that name the target it ran on.
#
#     crates/straf3-replay/crosstarget/verify.sh
#
# Reports land in crosstarget/results/<triple>.txt and are committed, so a
# reviewer can read the numbers without a musl toolchain.
#
# # Why a script and not an xtask subcommand
#
# `cargo xtask determinism` is criterion 2's, and its comparator is written
# around that report's shape — cases, per-command checksums, a known report
# version. Teaching it a second, differently shaped report would put two
# criteria's evidence behind one command, where a change made for one can
# quietly weaken the other. This drives the same four targets the same way and
# compares with `diff`, which for "these texts must be identical" is not a
# weaker tool than a parser.
#
# # Why the comparison is a plain diff
#
# Because the report is designed so that it can be. Every number that must
# agree is in the text, including every per-command checksum, so two targets
# agreeing means every one of those numbers agreed — there is no folded value
# standing in for numbers the comparison never saw. The two lines that legitimately
# differ (`target` and `platform`) are stripped, and everything else must match
# exactly.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
results="$here/results"
mkdir -p "$results"

pkg=straf3-replay
bin=straf3-s3d-crosstarget

targets=(
  x86_64-unknown-linux-gnu
  x86_64-unknown-linux-musl
  x86_64-pc-windows-gnu
  wasm32-unknown-unknown
)

fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; }
note() { printf '\033[36m%s\033[0m\n' "$*"; }

skipped=()
ran=()

for triple in "${targets[@]}"; do
  note "── $triple ─────────────────────────────────────────────"

  if [[ "$triple" == wasm32-unknown-unknown ]]; then
    # On wasm the binary has nothing to run in; the library carries the
    # exports.
    build_args=(build --release -p "$pkg" --lib --target "$triple")
    artefact="$root/target/$triple/release/straf3_replay.wasm"
  else
    build_args=(build --release -p "$pkg" --bin "$bin" --target "$triple")
    artefact="$root/target/$triple/release/$bin"
    [[ "$triple" == *windows* ]] && artefact="$artefact.exe"
  fi

  if ! (cd "$root" && cargo "${build_args[@]}" --offline); then
    fail "could not build for $triple (rustup target add $triple?)"
    skipped+=("$triple")
    continue
  fi

  if [[ ! -f "$artefact" ]]; then
    fail "$artefact was not produced"
    skipped+=("$triple")
    continue
  fi

  out="$results/$triple.txt"
  case "$triple" in
    wasm32-*)
      # V8, via Node. Same engine family the browser runs.
      node "$here/run-node.mjs" "$artefact" >"$out"
      ;;
    *windows*)
      # WSL interop runs the real .exe against the real Windows loader. Wine
      # only as a fallback: a determinism result from an emulator is worth
      # less, so if it is used the report says so on stderr.
      if ! "$artefact" >"$out" 2>/dev/null; then
        fail "the .exe did not run directly; falling back to wine"
        wine64 "$artefact" >"$out" 2>/dev/null || wine "$artefact" >"$out"
      fi
      ;;
    *)
      "$artefact" >"$out"
      ;;
  esac
  status=$?

  if [[ $status -ne 0 ]]; then
    fail "$triple: the report exited $status — a case failed its own assertions"
    exit 1
  fi
  ran+=("$triple")
  note "  $(grep '^grand ' "$out")  $(grep '^all-ok ' "$out")  $(wc -c <"$out") bytes"
done

if [[ ${#ran[@]} -lt 2 ]]; then
  fail "only ${#ran[@]} target(s) ran; there is nothing to compare"
  exit 1
fi

# The two lines that legitimately differ between targets.
strip() { grep -v -E '^(target|platform) ' "$1"; }

reference="${ran[0]}"
worst=0
for triple in "${ran[@]:1}"; do
  if diff <(strip "$results/$reference.txt") <(strip "$results/$triple.txt") >/dev/null; then
    note "  $triple == $reference"
  else
    fail "  $triple DIFFERS from $reference:"
    diff <(strip "$results/$reference.txt") <(strip "$results/$triple.txt") | head -20 >&2
    worst=1
  fi
done

if [[ ${#skipped[@]} -gt 0 ]]; then
  fail "not verified: ${skipped[*]}"
  worst=1
fi

if [[ $worst -eq 0 ]]; then
  note "all ${#ran[@]} targets agree on every number: $(grep '^grand ' "$results/$reference.txt")"
fi
exit $worst

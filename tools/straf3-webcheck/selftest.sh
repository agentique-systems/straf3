#!/usr/bin/env bash
# Show that `webcheck` can fail.
#
# The r6 verdict is worth exactly as much as the harness's ability to return
# the other answer. A check that has only ever agreed has not been shown to
# detect anything, so this runs the harness against four inputs it must refuse
# and one it must accept, and asserts the exit status of each.
#
# What it does NOT test is the one control that would need a tool for
# manufacturing a run that disagrees with its own header — a forgery tool this
# repository is better off not containing. That property is pinned instead by
# `straf3-replay`'s own suite, which is where it belongs:
#
#   crates/straf3-replay/src/tests.rs
#     a_divergence_is_reported_with_the_command_it_started_on
#     a_divergence_without_a_trace_says_so_rather_than_guessing
#
# `webcheck` prints what `Recording::verify` returns, so those two tests are
# the evidence that the DIVERGE branch and its command index are real.
#
# Run from the repository root:  tools/straf3-webcheck/selftest.sh

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root" || exit 1

bin="tools/straf3-webcheck/target/debug/webcheck"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fixture="probes/coil-course/results/coil-run.txt"
subject="$work/coil-native.s3d"

pass=0
fail=0

# `want` is the exit status the harness must produce. Asserting the status and
# not only the text is deliberate: this is used as a gate, and a gate that
# prints "DISAGREE" and exits 0 is not a gate.
check() {
  local want="$1" name="$2"; shift 2
  local out status
  out="$("$@" 2>&1)"; status=$?
  if [ "$status" = "$want" ]; then
    printf '  ok    %-46s (exit %s)\n' "$name" "$status"
    pass=$((pass + 1))
  else
    printf '  FAIL  %-46s (exit %s, wanted %s)\n' "$name" "$status" "$want"
    printf '%s\n' "$out" | sed 's/^/        /'
    fail=$((fail + 1))
  fi
}

echo "building the harness"
( cd tools/straf3-webcheck && cargo build --offline ) || exit 1
( cd tools/straf3-webcheck && cargo test --offline --quiet ) || exit 1

echo
echo "preparing a native subject from a committed run of coil"
"$bin" from-text "$fixture" --map assets/maps/coil.map --out "$subject" || exit 1

echo
echo "the harness must accept a run that reproduces"
check 0 "a native .s3d re-simulates to its own digest" \
  "$bin" resim "$subject"

echo
echo "the harness must refuse each of these"

# 1. The number the browser reported out of band does not match the file's
#    header. Catches a header written by something other than the simulation.
check 1 "a digest the recording does not claim" \
  "$bin" resim "$subject" --expect-digest 0x0000000000000001

# 2. A single flipped byte anywhere in the file. The content digest is checked
#    before any length field is believed, so this fails at load rather than
#    parsing into a plausible recording with half its commands missing.
corrupt="$work/corrupt.s3d"
cp "$subject" "$corrupt"
printf '\xff' | dd of="$corrupt" bs=1 seek=200 count=1 conv=notrunc status=none
check 1 "one flipped byte" \
  "$bin" resim "$corrupt"

# 3. The map compiled to different geometry. The recording binds itself to a
#    collision digest, so this is refused BEFORE anything is simulated —
#    otherwise the run would diverge because the world differs, and the report
#    would blame the browser.
#
#    The edit has to MOVE a plane, not merely change the text. Line 72 is the
#    underside of the start-room floor, three points all at z=-32; the first
#    attempt at this control shifted one of those points along x instead, which
#    is the same plane through different points and correctly left the digest
#    alone. The digest is a fold over compiled hulls, not over source bytes —
#    `straf3-map`'s `recolouring_a_face_does_not_change_the_collision_digest`
#    pins the same property from the other side.
mkdir -p "$work/maps"
sed '72s/-32 )/-33 )/g' assets/maps/coil.map > "$work/maps/coil.map"
cmp -s assets/maps/coil.map "$work/maps/coil.map" && {
  echo "  FAIL  the geometry control did not change the map"; fail=$((fail + 1)); }
check 1 "a map recompiled to different hulls" \
  "$bin" resim "$subject" --maps "$work/maps"

# 4. A harness whose dependencies resolved differently from the workspace's.
#    Demonstrated with a lock file naming a different glam; the real thing
#    happened by itself the first time this tool was built.
cat > "$work/fake.lock" <<'LOCK'
[[package]]
name = "glam"
version = "0.0.1"
LOCK
check 1 "a harness out of lockstep with the workspace" \
  "$bin" resim "$subject" --lock "$work/fake.lock"

echo
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]

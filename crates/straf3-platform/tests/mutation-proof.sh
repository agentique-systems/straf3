#!/usr/bin/env bash
# Mutation proof for the Wave 3 seam evidence (spec rev 6, criterion 4).
#
#   bash crates/straf3-platform/tests/mutation-proof.sh
#
# Every test in seam_oracle.rs claims to guard something. This script breaks
# each of those things in turn and checks that the corresponding test actually
# goes RED. A test that stays green while the thing it guards is broken is not
# evidence — and this repository has shipped four unconditionally-passing tests
# before, which is why this exists.
#
# A mutation that fails to COMPILE proves nothing either, so compile errors are
# reported as INVALID rather than counted as a catch.
#
# Files under tests/ are edited in place and restored from a snapshot
# afterwards, including on failure. Nothing outside tests/ is touched.
#
# One case (5b) is EXPECTED to come out GREEN. That is the finding, not a
# defect: it is what proves end-state-only comparison is blind.
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
TESTS="$HERE"
SNAP="$(mktemp -d)"
LOG="${LOG:-$ROOT/target/mutation-report.txt}"
mkdir -p "$(dirname "$LOG")"
trap 'rm -rf "$SNAP"' EXIT

cp -r "$TESTS/." "$SNAP/"
: > "$LOG"

restore() { rm -rf "$TESTS"; mkdir -p "$TESTS"; cp -r "$SNAP/." "$TESTS/"; }

# run_case <label> <test-filter> <mutation-python>
run_case() {
  local label="$1" filter="$2" mutation="$3"
  restore
  if ! python3 -c "$mutation"; then
    printf '%-56s | %-26s | %s\n' "$label" "$filter" "MUTATION DID NOT APPLY" | tee -a "$LOG"
    return
  fi
  local out verdict
  out=$(cd "$ROOT" && cargo test -p straf3-platform --test seam_oracle "$filter" 2>&1)

  # Order matters: cargo prints "error: test failed, to rerun pass ..." for an
  # ordinary RED test, so a naive compile-error grep scores every catch as a
  # compile failure. Test results are authoritative; only their ABSENCE means
  # the mutation failed to build.
  if echo "$out" | grep -q "^test result: FAILED"; then
    verdict="RED (caught)"
  elif echo "$out" | grep -qE "^error\[|could not compile|^error: expected|^error: cannot"; then
    verdict="INVALID (did not compile)"
  elif echo "$out" | grep -q "^test result: ok"; then
    if echo "$out" | grep -q "0 passed"; then
      verdict="INVALID (filter matched no test)"
    else
      verdict="GREEN (NOT CAUGHT - test is worthless)"
    fi
  else
    verdict="INDETERMINATE"
  fi

  printf '%-56s | %-26s | %s\n' "$label" "$filter" "$verdict" | tee -a "$LOG"
  {
    echo "--- $label"
    echo "$out" | grep -E "^test .* \.\.\. (ok|FAILED)|^test result:|panicked at|diverge at tick|^error" | head -6 | sed 's/^/      /'
    echo "$out" | grep -A3 "diverge at tick\|assertion" | head -8 | sed 's/^/      /'
    echo
  } >> "$LOG"
}

F="$TESTS/fixtures/strafe_jump_cpm.txt"
M="$TESTS/support/mod.rs"

# 1. A fixture on disk drifts from its definition.
run_case "fixture edited on disk (right_move 127 -> 126)" \
  "fixtures_match_their_definitions" "
t=open('$F').read()
assert 'cmd 1 0 127 0 - 0 0.55 0' in t
open('$F','w').write(t.replace('cmd 1 0 127 0 - 0 0.55 0','cmd 1 0 126 0 - 0 0.55 0',1))"

# 2. The same drift, seen by the PER-TICK replay comparison itself.
run_case "fixture drift reaches the per-tick replay comparison" \
  "headless_binary_and_in_process_simulation_agree_per_tick" "
t=open('$F').read()
assert 'cmd 1 0 127 0 - 0 0.55 0' in t
open('$F','w').write(t.replace('cmd 1 0 127 0 - 0 0.55 0','cmd 1 0 126 0 - 0 0.55 0',1))"

# 3. A one-ULP-scale view drift, mid-run. This is the sensitivity the
#    Cody-Waite trig change (spec rev 6 Q1) is about.
run_case "sub-degree mid-run yaw drift (0.55 -> 0.5500001)" \
  "headless_binary_and_in_process_simulation_agree_per_tick" "
t=open('$F').read()
assert 'cmd 1 0 127 0 - 0 0.55 0' in t
open('$F','w').write(t.replace('cmd 1 0 127 0 - 0 0.55 0','cmd 1 0 127 0 - 0 0.5500001 0',1))"

# 4. The renderer loses precision, so fixtures stop round-tripping.
run_case "render_scalar rounds to 3dp (loses bits)" \
  "rendered_scalars_round_trip_exactly" "
t=open('$M').read()
old='pub fn render_scalar(v: Scalar) -> String {\n    format!(\"{v}\")'
assert old in t, 'render_scalar body not found'
open('$M','w').write(t.replace(old,'pub fn render_scalar(v: Scalar) -> String {\n    format!(\"{v:.3}\")',1))"

# 5. THE mutation: the comparator degrades to an end-state-only check, which is
#    the exact bug spec rev 6 section R records as having really happened.
run_case "comparator truncated to end-state only" \
  "divergence_detector_catches_what_a_final_state_check_would_hide" "
t=open('$M').read()
old='pub fn assert_digests_match(left_label: &str, left: &[u64], right_label: &str, right: &[u64]) {'
assert old in t
new=old+'\n    let (left, right) = (&left[left.len().saturating_sub(1)..], &right[right.len().saturating_sub(1)..]);'
open('$M','w').write(t.replace(old,new,1))"

# 5b. EXPECTED GREEN, and it is the whole point. With an end-state-only
#     comparator, a drift that re-converges before the run ends is invisible.
#     This case is measured, not hypothetical: nudging right_move by 1 on one
#     8 ms command changes 88 of 322 ticks, then re-converges to the identical
#     final checksum 0xa446bc22001b5457. A GREEN here proves the end-state
#     comparator is blind; case 5c proves the per-tick comparator is not.
run_case "EXPECTED-GREEN: end-state-only misses a re-converging drift" \
  "headless_binary_and_in_process_simulation_agree_per_tick" "
t=open('$M').read()
old='pub fn assert_digests_match(left_label: &str, left: &[u64], right_label: &str, right: &[u64]) {'
new=old+'\n    let (left, right) = (&left[left.len().saturating_sub(1)..], &right[right.len().saturating_sub(1)..]);'
open('$M','w').write(t.replace(old,new,1))
f=open('$F').read()
open('$F','w').write(f.replace('cmd 1 0 127 0 - 0 0.55 0','cmd 1 0 126 0 - 0 0.55 0',1))"

# 5c. The real-fixture hiding test must also go red under that comparator.
run_case "end-state-only comparator vs the real hiding case" \
  "a_mid_run_divergence_can_hide_behind_a_matching_final_checksum" "
t=open('$M').read()
old='pub fn assert_digests_match(left_label: &str, left: &[u64], right_label: &str, right: &[u64]) {'
assert old in t
new=old+'\n    let (left, right) = (&left[left.len().saturating_sub(1)..], &right[right.len().saturating_sub(1)..]);'
open('$M','w').write(t.replace(old,new,1))"

# 6. Two runs made identical -> the independence check must notice.
run_case "strafe_jump_vq3 duplicated from strafe_jump_cpm" \
  "every_run_produces_a_distinct_and_evolving_digest_stream" "
t=open('$M').read()
old='strafe_jump(\"strafe_jump_vq3\", Profile::Vq3)'
assert old in t
open('$M','w').write(t.replace(old,'strafe_jump(\"strafe_jump_vq3\", Profile::Cpm)',1))"

# 7. The CSV parser becomes lenient and returns a short stream.
run_case "parse_trace_csv returns a stub instead of rejecting" \
  "trace_csv_parser_rejects_malformed_output_instead_of_shortening" "
t=open('$M').read()
old='    assert!(\n        !digests.is_empty(),'
assert old in t
new='    if digests.is_empty() { return vec![0];  }\n    assert!(\n        !digests.is_empty(),'
open('$M','w').write(t.replace(old,new,1))"

# 8. The empty-stream refusal removed.
run_case "assert_digests_match accepts two empty streams" \
  "empty_streams_are_not_evidence_of_equivalence" "
t=open('$M').read()
old='        !left.is_empty() && !right.is_empty(),'
assert old in t
open('$M','w').write(t.replace(old,'        true,',1))"

restore
echo
echo '===== SUMMARY ====='
grep -E "\| (RED|GREEN|INDETERMINATE|INVALID|MUTATION)" "$LOG"

#!/bin/sh
# Take every pacing measurement this probe publishes, in one pass.
#
# Run it from the repository root. That is not a style preference:
# `assets/maps/coil.map` is resolved relative to the working directory, and from
# anywhere else the client silently loads an empty plane and measures a scene
# with no geometry in it.
#
# The sets are deliberately separate directories, because they are different
# workloads and pooling them would be the error this whole probe is built to
# avoid.
#
# Runs discarded on host contention are kept, in `results/discarded/`. They are
# not deleted: an exclusion a reader cannot see is one they cannot judge, and
# the tree already decided this twice — `Stats::dropped` reports warm-up
# intervals rather than dropping them silently, and v2 moved the swapchain
# warm-up into a header named for what it is instead of deleting the sample.
set -eu

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

EXE=./target/x86_64-pc-windows-gnu/release/straf3.exe
PLAY=probes/coil-course/results/coil-run.txt
# Windows' WDDM driver package version. The process cannot see this — the
# adapter reports its own Vulkan driver string, which is a different identifier
# — so it is supplied here and lands in the header under a `host_` prefix that
# marks it as the caller's claim rather than a measured one.
WDDM=wddm_driver=32.0.15.6094

if [ ! -x "$EXE" ]; then
    echo "build it first: cargo build --release --target x86_64-pc-windows-gnu -p straf3-game --bin straf3" >&2
    exit 1
fi

# ── 1. the long steady-state distribution, both present modes ────────────────
#
# `--no-pb` rather than a personal-best directory: with one, a run that finishes
# the course writes a best that the *next* run loads and draws as a ghost, and
# the three pooled runs stop being three samples of one workload.
cargo xtask pacing --no-build --no-pb --runs 3 --exit-after 62000 \
    --note "$WDDM" --note workload=static-coil-noghost \
    --out probes/pacing/results/static --table

# ── 2. the same binary under a real movement workload ────────────────────────
#
# A stationary session measures the renderer drawing one scene from one camera.
# This drives the client from a recorded run of the course, so the frames
# measured are of the game moving — which is what Proof 3's "real rendering
# workloads" asks for and what set 1 cannot supply.
#
# The throwaway run is the seeding run: it writes the personal best that the
# three counted runs then race and *draw*, so all three render the same scene.
# Counting it as run 1 would make run 1 the one without a ghost.
rm -rf target/pacing-pb
mkdir -p target/pacing-pb
echo "=== seeding run (not published): produces the personal best the counted runs race ==="
"$EXE" --exit-after 7500 --play "$PLAY" --pb-dir target/pacing-pb >/dev/null 2>&1 || true

cargo xtask pacing --no-build --runs 3 --exit-after 7500 --play "$PLAY" \
    --note "$WDDM" --note workload=play-coil-run-ghost \
    --out probes/pacing/results/play --table

# ── 3. the one latency straf3 chooses rather than inherits ───────────────────
#
# `desired_maximum_frame_latency` defaults to 2; on Vulkan that is a swapchain
# image count of 3. This is a PACING CONTROL and nothing more: queue depth
# changes *when* a frame is displayed, not *how often*, and under FIFO every
# frame still lands on a vblank whatever the depth. A pacing log cannot see the
# quantity this knob moves. If the interval distribution is unchanged, the only
# thing that may be published is "the knob did not perturb pacing" — never "it
# made no difference". FIFO only: with vsync off nothing queues.
cargo xtask pacing --no-build --no-pb --runs 3 --mode fifo --exit-after 62000 \
    --frame-latency 1 --note "$WDDM" --note workload=static-coil-noghost \
    --out probes/pacing/results/frame-latency-1 --table

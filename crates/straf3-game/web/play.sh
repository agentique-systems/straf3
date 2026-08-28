#!/usr/bin/env bash
# Open straf3 in a real browser for a HUMAN to play, and capture the run.
#
#   crates/straf3-game/web/play.sh
#
# `drive.mjs` normally dispatches every input itself. This does not: it opens
# the window, arms a frame-pacing sampler, and then blocks on
# `straf3_last_run() !== null` for up to 30 minutes while a person plays. When
# a run crosses the finish line the driver pulls the bytes straight out of wasm
# and writes `docs/web/evidence/r6-browser.s3d`.
#
# WHY NOT JUST DOWNLOAD IT. The page's `onRunFinished` offers the `.s3d` as a
# download named `<digest16>.s3d`, which works, but the digest in that filename
# and the digest in the file's header come from the same `Recording::claimed()`
# call — so checking one against the other proves only that one wasm call was
# self-consistent. Driving it this way additionally captures the page's own
# console line, out of band, and the `[harness]` transcript around it. The
# genuinely independent number is still the native re-simulation, which is what
# `webcheck resim` produces afterwards.
#
# The bundle must already be built (`build.sh`) and the server already running
# (`node serve.mjs 8790`); this checks both rather than guessing.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
port="${PORT:-8790}"
url="http://127.0.0.1:${port}/play/coil?p=cpm"

# The default flags are for a GPU-less WSL2 box and yield NO WebGPU adapter on
# a machine that has a GPU — measured, see docs/web/evidence/r17-browser.txt §5.
# A run captured through them would not exist at all, so this refuses to guess.
if [[ -z "${CHROME:-}" ]]; then
    for candidate in \
        "/c/Program Files/Google/Chrome/Application/chrome.exe" \
        "/c/Program Files (x86)/Google/Chrome/Application/chrome.exe" \
        "$(command -v google-chrome || true)"; do
        [[ -x "$candidate" ]] && { CHROME="$candidate"; break; }
    done
fi
[[ -n "${CHROME:-}" ]] || { echo "set CHROME to your chrome executable" >&2; exit 1; }
export CHROME
export CHROME_FLAGS="${CHROME_FLAGS:---no-first-run --no-default-browser-check}"

[[ -f "$here/pkg/straf3_game_bg.wasm" ]] || {
    echo "no bundle at $here/pkg — run crates/straf3-game/web/build.sh first" >&2
    exit 1
}
curl -fsS -o /dev/null "http://127.0.0.1:${port}/play/coil" || {
    echo "nothing serving on ${port} — run: node $here/serve.mjs ${port}" >&2
    exit 1
}

cat <<EOF

  straf3 — $url

  A Chrome window is opening. Click the canvas to capture the mouse, then play
  coil to the finish line.

    WASD move · mouse look · Space jump · Ctrl crouch · Esc release · R respawn

  Coil wants a circle jump out of the start room, then strafejumping down the
  corridor. The ramp wave rewards 575 ups, the gully wants 600, and the last
  jump needs 425 or better onto the ledge. Falling in the gully costs seconds,
  not the run — you can climb out and carry on.

  Retry as often as you like: this waits for the first run that FINISHES.
  Ctrl-C here if you want to stop without recording one.

EOF

exec node "$here/drive.mjs" "$here/steps/r18-operator-play.json" --headful

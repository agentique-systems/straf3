# Step files for `drive.mjs`

Each file is a JSON array in the vocabulary `crates/straf3-game/web/drive.mjs`
implements. They are committed because the transcripts in
`docs/web/evidence/` are only reproducible if the input that produced them is.

| file | what it is for |
|---|---|
| `r17-evidence.json` | the r17 capture: adapter, map compile, pointer lock, a strafejump run down coil's corridor, frame pacing, screenshot. Produces `docs/web/evidence/r17-browser.txt` run A. |
| `r17-pacing.json` | pacing only, short enough to bracket with a concurrent `typeperf`. Run B of the same transcript, taken under a saturated host. |
| `r18-coil-attempt.json` | the scripted attempt to *finish* coil, which does not succeed. Kept because "the bot gets to 470 ups and is stopped by the ramp wave" is a claim someone should be able to re-run. |
| `r18-strafe-tuning.json` | the turn-rate response measurement: four strafe rates from a standing start, sampled. This is what established that the technique works under CDP at all and roughly where the useful rate is. |

**Run them with the flags overridden.** `drive.mjs`'s defaults are for a
GPU-less WSL2 box and yield no WebGPU adapter on a real machine:

```sh
node crates/straf3-game/web/serve.mjs 8790 &
CHROME="C:/Program Files/Google/Chrome/Application/chrome.exe" \
CHROME_FLAGS="--no-first-run --no-default-browser-check" \
  node crates/straf3-game/web/drive.mjs \
       crates/straf3-game/web/steps/r17-evidence.json --headful
```

Two things bite on a fresh host, both fixed in the driver rather than here:
`recenter` exists because `look_to` leaves the virtual cursor against a viewport
bound, where half of a subsequent strafe's mouse deltas are silently dropped;
and input events are fired rather than awaited, because awaiting each one caps
the driver near 1.5 iterations a second, which is too slow to strafejump at all.

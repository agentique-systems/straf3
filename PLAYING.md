# Playing straf3

This is the run document: how to build it, how to start it, what you see, and
how to check that what you saw was real. It describes what is in the tree right
now. Anything not yet landed is marked as such rather than described as if it
were.

What exists: a native client that compiles a Valve 220 `.map` into the geometry
you collide with *and* the geometry you see, a Q3/CPM movement model at a fixed
125 Hz command rate, mouse-look and WASD, an on-screen telemetry overlay, and a
record/replay path whose checksums three separate readers agree on.

The governing document is [`docs/VISION.md`](docs/VISION.md). Where this file
and the vision disagree about what the game is for, the vision wins.

---

## Before you start: this is a headless Linux box

> **There is no GPU here and, in this shell, no working Wayland socket.**
> Vulkan resolves to the software rasteriser `llvmpipe`, so the window opens
> and the loop runs, and that is all these instructions verify. **No frame
> rate, smoothness or latency number produced on this machine means anything**
> — the vision's pacing budgets (`docs/VISION.md`, "Frame pacing and latency")
> are measured on the native Windows build and nowhere else. See the README for
> the Windows recommendation.

`WAYLAND_DISPLAY` is set here to a socket that does not exist, and winit tries
Wayland before X11 and then gives up rather than falling back:

```
[ERROR straf3_game::app] could not create an event loop: os error at .../wayland/event_loop/mod.rs:89: Could not find wayland compositor
```

Unsetting it for the one command is the whole fix, and the X11 socket
(`/tmp/.X11-unix/X0`, `DISPLAY=:0`) works:

```
WAYLAND_DISPLAY= cargo run -p straf3-game --bin straf3
```

Every windowed command below assumes you have done that if you hit the error.
Nothing headless — replay, the offscreen renders, the test suite — needs it.

---

## Run it

```
cargo run -p straf3-game --bin straf3
```

That compiles `assets/maps/coil.map` and drops you at its `info_player_start`,
under the `cpm` profile at 125 Hz (8 ms commands). A real start on this machine
prints:

```
[INFO  straf3_game::scene] map: 26 hulls, 4 triggers, 312 triangles, collision digest 0x47263b8845d8bb4b
straf3-render: backend=Vulkan adapter="llvmpipe (LLVM 20.1.2, 256 bits)" type=Cpu
straf3-render: map is 312 triangles
[INFO  straf3_game::app] straf3 0.1.0 — world Map, cpm profile, 125 Hz (8 ms commands). Click to capture the mouse, Esc to release, R to respawn.
```

The first line is the compile: the same pass produces the 26 convex hulls you
collide with and the 312 triangles you see, so there is no way to be shown a
different world from the one you are hitting. The `collision digest` is over the
hulls only — it is the number that must match on every target.

The two `straf3-render:` lines come from the renderer unprefixed; the rest are
`log::info!` and carry the logger's `[INFO ...]` prefix.

If you also see this, it is WSLg's desktop portal, not straf3 — harmless:

```
[ERROR sctk_adwaita::config] XDG Settings Portal did not return response in time: timeout: 100ms, key: color-scheme
```

### A map is data

`--map <file.map>` plays any Valve 220 source. `coil.map` is the course this
repository ships and is the default; it is authored here, so the repository
carries no map of unresolved licence. The compiler is below the seam, so a map
that compiles here compiles identically on every target.

`--world flat` and `--world empty` need no map at all. They are the two worlds
`straf3-headless` can also reproduce, which is why a recording made in them can
be replayed by a program with no renderer in it.

---

## Controls

- **WASD** — move
- **mouse** — look
- **Space** — jump
- **Ctrl** — crouch
- **Shift** — walk
- **click** the window to capture the mouse; **Esc** releases it
- **R** — respawn

---

## What the overlay shows

Four readouts, which are what a movement run is judged by:

```
                        0:12.480        run time, m:ss.mmm
                         -0.312         split against the ghost

                          487 ups       horizontal speed
                          AIR           ground / slide / air

 241 fps   vz -120   tick 1560   sim 12480 ms
```

- **Run time**, top centre. `--:--.---` before you cross the start line, white
  while the clock runs, gold once it stops. It is `u32` milliseconds summed
  from command durations, never read from a clock, so the same inputs give the
  same time on a 60 fps laptop and a 240 fps desktop.
- **Split**, under the clock. Signed like a motorsport split: **negative is
  good**. Green when you are ahead of the ghost, red when you are behind.
  Absent entirely when no ghost is loaded — it is never `+0.000`, because that
  would claim you are level with a personal best that is not there.
- **Speed**, centre. Horizontal only, whole units per second: vertical speed is
  gravity's, and a total-speed readout swells on every fall. It is tinted green
  while you are gaining speed and red while you are bleeding it, which is a
  display heuristic and not something the simulation knows about.
- **Ground / slide / air**, under the speed. Three states, not two: `SLIDE`
  (amber) means you are touching a plane too steep to walk on — velocity is
  clipped to it, but friction does not apply and you cannot jump. Collapsing
  that into "on the ground" would hide the technique.
- **Corner line**, bottom left, dim. Frame rate, vertical speed, tick count and
  simulation time. `sim` is the sum of command durations, not wall time; the
  two disagreeing is the interesting case. (The `fps` number is real as a
  readout and meaningless as a claim — see the warning at the top.)

### Where it draws today

The overlay lives in `crates/straf3-devtools` and is complete: composition,
colours, and the wgpu draw. Its layout is covered by unit tests that read the
drawn strings back out of egui, and it has been rendered to a real texture on
this machine through the software adapter.

**It is now called from the windowed client.** `straf3-game` hands it the frame
every tick, and the ghost is drawn into the same pass. That wiring landed at the
close of this wave, together with the PB and ghost work described below.

One honest limit on that claim: this box has no GPU, and the windowed client was
never launched here to look at it. The wiring is landed, compiles, and the whole
workspace suite is green — but **nobody has yet watched the overlay on screen in
the windowed client.** Until someone runs it on a real GPU, the offscreen
renderer below is still the only place its pixels have actually been seen:

```
cargo run -p straf3-devtools --example hud-offscreen
# writes target/hud-offscreen/*.ppm — four states of a run, at 1280x720
```

```
hud-offscreen: backend=Vulkan adapter="llvmpipe (LLVM 20.1.2, 256 bits)" type=Cpu
hud-offscreen: before-the-line       3629 pixels painted over the background  ok
hud-offscreen: mid-run-ahead         7146 pixels painted over the background  ok
hud-offscreen: on-a-ramp-behind      7991 pixels painted over the background  ok
hud-offscreen: finished              6877 pixels painted over the background  ok
```

PPM is what `ffmpeg -i` and most viewers read directly:

```
ffmpeg -i target/hud-offscreen/mid-run-ahead.ppm /tmp/hud.png
```

### The console readout

Once a second (`RUST_LOG=info` is the default), the client logs where the
simulation is:

```
[INFO  straf3_game::app] speed    0.0 ups   origin (  -320.0   -736.0     24.1)   ground   tick 125   sim 1000 ms   41 fps
```

The same four things, in a terminal. It stays useful after the overlay lands:
it is the only readout that survives into a redirected log file.

---

## Options

```
usage: straf3 [options]                     open a window and play
       straf3 --replay <file> [options]     run a recorded file, no window

  --map <file.map>            Valve 220 map to compile and play (default
                              assets/maps/coil.map)
  --world <map|flat|empty>    geometry to play in (default map). `flat` and
                              `empty` need no map and are the two worlds
                              straf3-headless can reproduce.
  --profile <cpm|vq3>         movement constants (default cpm)
  --rate <hz>                 command rate, 1..=1000 (default 125)
  --record <file>             write every command produced to <file>, in
                              straf3-headless's input format
  --exit-after <ms>           close the window after <ms> of wall time, so an
                              unattended run can be recorded and replayed
  -h, --help                  this

replay options (no window is opened and no GPU adapter is created):
  --replay <file|->           run a recorded command file, `-` for stdin
  --trace                     print one line per tick, not just the final state
  --csv                       print in straf3-headless's CSV form
  --frame-ms <a,b,c,...>      drive the replay on this frame schedule, in whole
                              wall milliseconds, cycled.
```

`--map`, `--world`, `--profile` and `--rate` only take effect when opening a
window. A replay always runs under the world, profile and rate recorded in the
file itself; passing them alongside `--replay` is silently ignored.

A map that cannot be read or compiled is a warning, not a failure — the client
drops to the flat world, so a missing file still gives you a window you can
move in rather than a process that dies.

An unattended run, for scripting:

```
cargo run -p straf3-game --bin straf3 -- --exit-after 2000
```

---

## Record a run and check it reproduces

Every recording names the world it was made in, and every reader prints the
same 64-bit checksum of the final state. That equality is the point: a last-bit
divergence is invisible to the eye and obvious to the number.

```
cargo run -p straf3-game --bin straf3 -- --world flat --record /tmp/flat.rec
# play — strafe, jump — then close the window
cargo run -p straf3-game --bin straf3 -- --replay /tmp/flat.rec
cargo run -p straf3-sim --bin straf3-headless -- /tmp/flat.rec
cargo run -p straf3-game --bin straf3 -- --replay /tmp/flat.rec --frame-ms 1,97,3,250,8
```

All four print one checksum. Verified here on a 180-command session:

```
  checksum      0xe08d7c7726883be5      # straf3 --replay
  checksum      0xe08d7c7726883be5      # straf3-headless
  checksum      0xe08d7c7726883be5      # straf3 --replay --frame-ms 1,97,3,250,8
```

The last one is the one that matters most: the same input on a deliberately
hostile frame schedule reaches the identical state. Rendering is decoupled from
simulation stepping, and this is what says so.

Play for a few seconds before closing — an unattended, input-free recording
reproduces trivially and proves nothing.

> **The recording is written only on a clean exit** — closing the window, or
> letting `--exit-after` end the run. The file is written after the event loop
> returns, so killing the process (Ctrl-C, `kill`, a closed terminal) skips
> that write entirely: you get no file rather than a truncated one, however
> long you played, and nothing is printed to say so.

`straf3-headless` lives below the seam and only knows `empty` and `flat <z>`.
It has no way to spell a compiled map, so it refuses a map recording by name
and exits non-zero rather than silently running it somewhere else:

```
$ cargo run -p straf3-sim --bin straf3-headless -- /tmp/coil.rec
straf3-headless: /tmp/coil.rec: line 12: unknown world `map` (empty|flat <z>)
```

---

## Checking the build

```
cargo test --workspace                 # everything
cargo xtask check-seam                 # nothing below the line reaches above it
cargo xtask determinism                # one command stream, four targets, one digest
cargo run -p straf3-render --example offscreen        # the world, to PPM
cargo run -p straf3-devtools --example hud-offscreen  # the overlay, to PPM
```

The two offscreen examples need a GPU adapter — including a software one — and
are examples rather than tests for exactly that reason: a test that fails on a
machine with no adapter punishes the correct environment.

---

## Not in this build

Stated plainly so nothing above is read as a promise:

- **The overlay is drawn by the windowed client, but has not been watched on
  screen** (see above). Landed and compiling, never visually confirmed here.
- **A personal best is saved and a ghost is raced — in code that has not been
  played.** The run clock, the `.s3d` format, PB persistence at
  `runs/<map>.<profile>.s3d` and the re-simulated ghost all landed this wave and
  are covered by unit tests. What was verified end to end on this box is the
  headless path: the shipped binary replays `coil-run.txt` against `coil.map`
  and produces a 5096 ms run with checksum `0x9a854d1a3653d8b7`. What was *not*
  verified is a human playing a run, saving a PB and racing it in the window.
- **No browser client.** The wasm build is proven bit-identical to native by
  `cargo xtask determinism`; a playable URL is deferred (spec rev 2,
  criterion 9).
- **No sound, weapons, menus, multiplayer or leaderboards.** None of these are
  in scope for this wave.

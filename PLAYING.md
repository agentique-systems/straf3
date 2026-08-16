# Playing straf3

This is Wave 3's first playable build: a hardcoded arena, mouse-look,
keyboard movement, and a strafe-jump-capable CPM movement model. No maps,
weapons, sound or menus yet.

> **Software rendering under WSL2.** This box has no GPU passthrough, so
> Vulkan resolves to the software rasteriser `llvmpipe`. The window opens and
> the render loop runs — that is what these instructions verify. Whether it
> looks and feels right, and whether live input moves the player, is yours
> to judge; these instructions make no claim about frame rate, smoothness or
> latency "feel". See the README for the native-Windows recommendation for
> actually tuning movement.

## Run it

```
cargo run -p straf3-game --bin straf3 -- --world arena
```

This opens a window on the hardcoded arena, under the `cpm` profile at
125 Hz (8 ms commands). A real run on this machine prints, on start:

```
straf3-render: backend=Vulkan adapter="llvmpipe (LLVM 20.1.2, 256 bits)" type=Cpu
straf3-render: arena is 5044 triangles
straf3 0.1.0 — world Arena, cpm profile, 125 Hz (8 ms commands). Click to capture the mouse, Esc to release, R to respawn.
```

If you also see a line like this, it is WSLg's desktop portal, not straf3 —
harmless, and unrelated to whether the window works:

```
[ERROR sctk_adwaita::config] XDG Settings Portal did not return response in time: timeout: 100ms, key: color-scheme
```

Once a second, a speed readout is logged to the terminal (`RUST_LOG=info` by
default) — this wave has no on-screen HUD, so this line is how you judge a
strafe-jump run while playing:

```
speed  xxx.x ups   origin (...)   ground|slide |air      tick N   sim N ms   N fps
```

## Controls

- **WASD** — move
- **mouse** — look
- **Space** — jump
- **Ctrl** — crouch
- **Shift** — walk
- **click** the window to capture the mouse; **Esc** releases it
- **R** — respawn

## Options

```
usage: straf3 [options]                     open a window and play
       straf3 --replay <file> [options]     run a recorded file, no window

  --world <arena|flat|empty>  geometry to play in (default arena)
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
                              wall milliseconds, cycled. The output must be
                              identical to the regular schedule's — that
                              equality is what criterion 5 means.
```

An unattended run, for scripting:

```
cargo run -p straf3-game --bin straf3 -- --world arena --exit-after 2000
```

## Record and replay

Every recording names the world it was made in. `straf3 --replay` understands
all three, so an arena session replays in the arena. `straf3-headless` (in
`straf3-sim`, below the seam) only knows `empty` and `flat <z>` — it has no
way to spell the arena, so it refuses an arena recording by name and exits
non-zero rather than silently running it somewhere else:

```
$ cargo run -p straf3-game --bin straf3 -- --world arena --exit-after 2000 --record /tmp/arena.rec
...
straf3: recording written to /tmp/arena.rec
$ cargo run -p straf3-game --bin straf3 -- --replay /tmp/arena.rec
...
  world         Arena
...
  checksum      0x38078f5270e2ecad
$ cargo run -p straf3-sim --bin straf3-headless -- /tmp/arena.rec
straf3-headless: /tmp/arena.rec: line 12: unknown world `arena` (empty|flat <z>)
```

The round-trip below uses `--world flat` so both readers can run the same
file and their checksums can be compared.

Play for a few seconds — strafe, jump — before closing the window; an
unattended, input-free recording reproduces trivially and proves nothing
about the record/replay path.

```
cargo run -p straf3-game --bin straf3 -- --world flat --record /tmp/flat.rec
# play, then close the window (or let --exit-after end it)
cargo run -p straf3-game --bin straf3 -- --replay /tmp/flat.rec
cargo run -p straf3-sim --bin straf3-headless -- /tmp/flat.rec
```

> **The recording is written only on a clean exit** — closing the window or
> letting `--exit-after` end the run. The file is written after the event
> loop returns, so killing the process (Ctrl-C, `kill`, a closed terminal)
> skips that write entirely: you get no file, not a truncated one, however
> long you played, and nothing is printed to say so.

Both print the same checksum. Replay also accepts a different frame
schedule; the resulting checksum is identical to the regular schedule's,
which is criterion 5's proof that rendering is decoupled from simulation
stepping:

```
cargo run -p straf3-game --bin straf3 -- --replay /tmp/flat.rec --frame-ms 1,97,3,250,8
```

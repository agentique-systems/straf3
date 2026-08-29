# Playing straf3

This is the run document: how to build it, how to start it, what you see, and
how to check that what you saw was real. It describes what is in the tree right
now. Anything not yet landed is marked as such rather than described as if it
were, and any claim this document's author did not verify personally is marked
**`[reported]`**.

What exists: a native client that compiles a Valve 220 `.map` into the geometry
you collide with *and* the geometry you see, a Q3/CPM movement model at a fixed
125 Hz command rate, mouse-look and WASD, an on-screen telemetry overlay, and a
record/replay path whose checksums three separate readers agree on.

The governing document is [`docs/VISION.md`](docs/VISION.md). Where this file
and the vision disagree about what the game is for, the vision wins. If you are
here to play and report back rather than to develop,
[`PLAYTEST.md`](PLAYTEST.md) is the shorter document you want.

### Before anything: the numbers in this file age differently

Two kinds of hexadecimal number appear below, and confusing them will make you
distrust the wrong one.

- A **collision digest** — `0x47263b8845d8bb4b` for `coil` — is folded over the
  compiled map's convex hulls and trigger volumes, behind a version tag, and
  over nothing else (`CompiledMap::collision_digest`, in `straf3-map`). No
  change to the physics can reach it. It is stable, and it is the number that
  must match on every target.
- A **state checksum** is folded over the whole simulation state, including
  every timer the movement code branches on
  (`crates/straf3-sim/src/state.rs`). That is deliberate — a field the
  simulation branches on is a way for two builds to disagree about a run — and
  it means that adding a mechanic which needs a new timer changes the checksum
  under `straf3`, `vq3` and `cpm` alike, without canon movement having moved a
  millimetre.

  **This is not hypothetical, and it has already happened here.**
  `SimState::checksum` folds `Timers::slide_ms`, `Timers::dash_ms`,
  `Timers::wall_contact_ms` and `PlayerState::wall_normal` — the candidate
  mechanics' state, which is present and permanently zero under every canon
  profile. Any checksum published before those were folded in no longer
  reproduces, without a millimetre of movement having changed. Determinism is
  untouched by that: the property is same-build reproducibility, and it still
  holds exactly.

So every state checksum printed below is **an illustration of what one command
printed on one build**, not a value to compare against. What this file claims is
the *invariant*: that the three readers agree with each other, and that a
deliberately hostile frame schedule reaches the same state as a regular one.
Where a literal is quoted, the build that produced it is named.

That last rule is the one this file has broken before. A literal attributed to
"this tree" is a claim with an expiry date nobody can see; a literal attributed
to a **commit** is history, and history does not expire — it just stops being
the latest. Where a number below is quoted at all, it is quoted that way.

For the same reason, where this file cites a specification or a decision it
states what the decision *was*, not only which revision carried it. A bare
revision number is a citation whose meaning changes without notice. The same
goes for line numbers: this file cites symbols, which survive an edit above
them.

---

## Run it on the real GPU

This is the play-and-tune path, and it runs from the WSL shell: no Windows-side
Rust install, no leaving the terminal. The mechanism is recorded and verified in
[`docs/environment.md`](docs/environment.md) §3 — the cross-linked `.exe`
executes through WSL interop as a genuine Windows process, and wgpu reaches the
host's discrete adapter rather than any WSLg software path.

```
rustup target add x86_64-pc-windows-gnu   # once; rust-toolchain.toml lists it too
cargo build --release --target x86_64-pc-windows-gnu -p straf3-game --bin straf3
./target/x86_64-pc-windows-gnu/release/straf3.exe
```

**Run it from the repository root.** `assets/maps/coil.map` is resolved relative
to the working directory, and for an interactive session a map that cannot be
read is a warning rather than a failure — so from anywhere else you get a flat
plane, one line on stderr you will not be looking at, and a session that
measures nothing. (A *replay* refuses instead; see below. The asymmetry is
deliberate.)

On the host this was developed on, that reaches an RTX 3060 Ti over Vulkan
(`DiscreteGpu`, driver NVIDIA 560.94). Two caveats from `docs/environment.md` §3
matter if you are timing anything:

- The exe lives on the Linux filesystem and Windows reaches it over 9p. Process
  execution is native; **file I/O is not**. For pacing work, build into a
  Windows-native path:
  `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_DIR=/mnt/c/straf3-target`.
- `-gnu` emits DWARF, so Windows-native debuggers will not read symbols.
  `RUST_BACKTRACE=1` inside the process still works.

> **Hole, marked rather than hidden.** `tools/straf3-capture` — a repeatable
> screenshot of the running client on the real GPU — is being built in this wave
> and its invocation, output path and window-not-found behaviour are not yet in
> this document. Nothing else in this section is pending: `--play`,
> `--profile experimental` and `--pacing-log` have landed and are described
> against the binary.

**Standing rule for images: no picture committed to this repository shows
anything but the straf3 window.** Capture the window, never the screen. A
full-desktop grab carries whatever else was open — browser tabs, accounts,
notifications — into a git history that is hard to un-publish, and it does so
while looking exactly like a correct screenshot. Use the capture command rather
than a system screenshot key, and if you need a full-desktop grab to diagnose
something, leave it under `target/`, which `.gitignore` already excludes.

---

## Run it on Linux, and what that is worth

> **There is no GPU here and, in this shell, no working Wayland socket.**
> Vulkan resolves to the software rasteriser `llvmpipe`, so the window opens
> and the loop runs, and that is all these instructions verify. **No frame
> rate, smoothness or latency number produced on this machine means anything**
> — the vision's refresh-class targets (`docs/VISION.md` §9, Native game and
> browser game: the 240 Hz class on desktop where hardware permits it, and
> roughly the 120 Hz class in the browser on capable systems) are measured on
> the Windows build above and nowhere else.

That warning is about *numbers*, not about the build. Everything headless —
replay, the offscreen renders, the whole test suite — is exactly as valid here
as anywhere, and that is most of the working day.

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
Nothing headless needs it.

That compiles `assets/maps/coil.map` and drops you at its `info_player_start`,
under the **`straf3` profile** at 125 Hz (8 ms commands) — Straf3's own frozen
canon, and what a session runs when you name no profile. A start **on the
software adapter** prints:

```
[INFO  straf3_game::scene] map: 26 hulls, 4 triggers, 312 triangles, collision digest 0x47263b8845d8bb4b
straf3-render: backend=Vulkan adapter="llvmpipe (LLVM 20.1.2, 256 bits)" type=Cpu
straf3-render: map is 312 triangles
[INFO  straf3_game::app] straf3 0.1.0 — world Map, cpm profile, 125 Hz (8 ms commands). Click to capture the mouse, Esc to release, R to respawn.
```

The first line of that block was reproduced on this tree while writing this
document; the three that follow it come from a windowed start, which was not
re-run here — opening a window on the software adapter proves nothing that the
headless paths do not.

**That last line is a transcript from before the default moved, which is why it
says `cpm profile` where a start today says `straf3 profile`.** It is left as
captured rather than edited to match: a windowed start could not be re-run on
this host, and rewriting the one word inside a block labelled as observed output
would make it a fabrication instead of a record. The rest of the line is
unaffected — the two profiles are numerically equal, so nothing but the name
changed.

The `adapter=... type=Cpu` line is what tells you which of the two sections you
are in; on the Windows build it names the discrete GPU instead.

The first line is the compile: the same pass produces the 26 convex hulls you
collide with and the 312 triangles you see, so there is no way to be shown a
different world from the one you are hitting. The `collision digest` is over the
hulls and triggers only — the stable number described at the top of this file.

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
- **R** — respawn, which starts a **new attempt**: the clock resets, the ghost
  returns to the start line, and **the recording begins again**. A respawn is
  not a command, so a recording that spanned one could not be re-simulated;
  the recorder is replaced rather than continued
  (`crates/straf3-game/src/game.rs`). Everything recorded before your last R is
  gone. This is the easiest way to lose a session — see the recording rules
  below.

Under `--play`, live movement input and `R` are ignored: the recording drives
the session. `Esc` and closing the window still work.

---

## What the overlay shows

Four readouts, which are what a movement run is judged by. The block below is an
**illustration** — the values are the ones the overlay's own layout tests use,
not a transcript of a session:

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
  readout and meaningless as a claim when you are on the software adapter.)

### Where it draws

The overlay lives in `crates/straf3-devtools`: composition, colours, and the
wgpu draw. Its layout is covered by unit tests that read the drawn strings back
out of egui.

It is called from the windowed client — `straf3-game` hands it the frame every
tick, and the ghost is drawn into the same pass.

**`[reported]`: it has been watched on screen on a real GPU, but not by this
document's author, and no screenshot of that session exists in this
repository.** A prior session played the Windows build on the RTX 3060 Ti and
reported the overlay legible. See "Not proven yet" below for exactly what that
rests on and what would retire it.

What anyone can reproduce here, with no GPU, is the offscreen render:

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

The same four things, in a terminal. It stays useful alongside the overlay: it
is the only readout that survives into a redirected log file.

---

## Options

Copied from the binary's own `--help`, which is authoritative — if the two ever
disagree, the binary is right and this file has a bug.

```
usage: straf3 [options]                     open a window and play
       straf3 --play <file> [options]       open a window and watch a recorded
                                            run drive it
       straf3 --replay <file> [options]     run a recorded file, no window

  --play <file>               drive the windowed, rendering session from a
                              recorded command file instead of from the
                              keyboard. The file's own rate, profile, world,
                              spawn and yaw are used, exactly as --replay does,
                              so the run on screen is the run in the file and
                              lands on the same checksum. Live movement input
                              and R (respawn) are ignored; Esc and closing the
                              window still work. When the stream runs out the
                              final state is held and the window stays open —
                              --exit-after is what ends an unattended session.
  --map <file.map>            Valve 220 map to compile and play (default
                              assets/maps/coil.map)
  --world <map|flat|empty>    geometry to play in (default map). `flat` and
                              `empty` need no map and are the two worlds
                              straf3-headless can reproduce.
  --profile <straf3|cpm|vq3|experimental>
                              movement constants (default straf3, the ruleset
                              frozen in docs/movement-canon.md Part 3). `cpm`
                              and `vq3` are the two games straf3 was
                              reconstructed beside and are ranked alongside it.
                              `experimental` carries the three candidate
                              mechanics canon rejected — crouch slide, dash and
                              wall jump — so it is playable and recordable, but
                              its personal bests are kept under their own name
                              (runs/<map>.experimental.s3d) and are never ranked
                              against a canon time.
  --rate <hz>                 command rate, 1..=1000 (default 125)
  --record <file>             write every command produced to <file>, in
                              straf3-headless's input format
  --pb-dir <dir>              where personal bests are kept (default runs/).
                              The best saved run for this map and profile is
                              raced as a ghost, and a finished run that beats
                              it is written there as <map>.<profile>.s3d
  --no-pb                     neither load a ghost nor save a personal best
  --exit-after <ms>           close the window after <ms> of wall time, so an
                              unattended run can be recorded and replayed
  --pacing-log <file>         write one high-resolution frame delta per frame to
                              <file> as CSV when the session ends. Measurement
                              only: the simulation keeps taking whole-millisecond
                              deltas from exactly the path it uses without this
                              flag. Needs a window, so not with --replay.
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

`--trace`, `--csv` and `--frame-ms` are **refused** without `--replay` rather
than ignored: silently accepting them would let you believe you had measured a
frame schedule when you had opened a window instead.

`--exit-after` counts wall time **from process start**, not from when the
session begins — adapter creation and window mapping are inside the budget.
Allow roughly two seconds of startup on top of however long you mean to run,
or an unattended recording ends early and looks complete.

### What a replay does and does not take from the command line

`--profile` and `--rate` only take effect when opening a window for live play. A
replay or a playback runs at the rate and under the profile recorded in the file
itself, and passing them alongside changes nothing.

**`--map` and `--world` are different.** They are read before the replay runs,
and a recording that says `world map` means "whichever map this process has
installed" — the fixture format carries no map identity of its own
(`crates/straf3-game/src/replay.rs`). So they decide what the run is replayed
*against*, and getting them wrong used to produce a complete, plausible, wrong
answer with a success exit code. It now refuses:

```
$ straf3 --replay probes/coil-course/results/coil-run.txt
straf3: probes/coil-course/results/coil-run.txt: this file was recorded in the `map` world and this process has no map installed, so there is nothing to replay it against. Pass `--map <file.map>` naming the map it was recorded on. (Refusing rather than falling back to the flat world: that would print a trace, a checksum and a run time for a run that happened somewhere else.)
$ echo $?
1
```

`--play` refuses the same case with its own message, also exit 1.

The asymmetry with interactive play is deliberate rather than an inconsistency.
Starting `straf3` with an unreadable map still drops to the flat world and warns,
because a window you can move in beats a process that will not start. A replay
or a playback refuses, because there the **output is the claim** — a trace, a
checksum and a run time are evidence, and evidence about the wrong world is
worse than no evidence.

So pass `--map` explicitly whenever you replay a recording made in a map,
exactly as the probe's own verify command does
(`probes/coil-course/results/coil.txt`):

```
$ straf3 --replay probes/coil-course/results/coil-run.txt --map assets/maps/coil.map
  world         Map
  run           5096 ms  (5.096 s, start 1800 ms, finish 6896 ms)
  crossings     start@1800ms/tick225 checkpoint0@4048ms/tick506 checkpoint1@6264ms/tick783 finish@6896ms/tick862
  checksum      0xf3cabd183c90d8d7
```

The `crossings` line is the run's **route**: which of the map's timing volumes
it went through, and when. `coil` declares four — a start, two checkpoints and a
finish — and the run above passes all four in the order the map declares them.
The clock cannot tell you that: a shortcut from the start volume straight to the
finish is `Finished` too, with a better time. Read it against the map when a run
time is being offered as evidence.

That checksum was printed by this tree on 2026-08-29. It had been
`0x9a854d1a3653d8b7`, and it moved without a millimetre of movement changing —
see the state-checksum note at the top of this file.

An unattended run, for scripting:

```
cargo run -p straf3-game --bin straf3 -- --exit-after 2000
```

### The canon profile, and the two it was reconstructed beside

`straf3` is the default and needs no flag. `--profile cpm` and `--profile vq3`
select the two games it was reconstructed beside; all three are canon, and their
times are ranked.

`straf3` is **numerically equal to `cpm`** in this tree. Canon Part 2 judged
three candidate mechanics and rejected all three, so no inherited constant
moved and the freeze came out equal to the reconstruction it started from
(`docs/movement-canon.md` §3.8). That is a finding, not a link: `cpm` is a
reconstruction of somebody else's game and may be corrected against a CPMA demo
one day, and `straf3` must not move when it is.

What this means in practice is that the two differ **only in name** — which is
not nothing, because the name is what a run is filed and ranked under. See
"Where your personal best goes" below.

### The experimental profile

`--profile experimental` is accepted, playable and recordable, and its personal
bests are filed at `runs/<map>.experimental.s3d` so they are never ranked
against a canon time.

**It is where the rejected mechanics still live.** `PhysicsProfile::experimental()`
is spelled `..Self::cpm()` with eight constants overridden — `slide_entry_speed`
400, `dash_speed` 400, `wall_jump_velocity` 200 and the rest
(`crates/straf3-sim/src/profile.rs`) — so it is canon's numbers today, since
canon and `cpm` are equal, plus crouch slide, dash and wall jump switched on.
Deliberately only those eight: anything the lab measures between the two is then
attributable to the mechanics and to nothing else. Canon Part 2 rejected all
three, and this profile is what they were measured in and what
`tools/straf3-lab` still compares against canon. It did not become redundant
when canon landed; measuring against it is the job it was built for.

So the client says what you are getting at startup:

```
[WARN  straf3_game::app] profile `experimental` is not canon: its personal bests are kept under their own name and are never ranked against a canon time
[WARN  straf3_game::app] `experimental` carries the three candidate mechanics canon Part 2 rejected — crouch slide, dash and wall jump. It is kept so they stay measurable (tools/straf3-lab), not because they are returning
```

An earlier version of this section described a third line, conditional on the
profile still holding CPM's constants because `straf3-sim` had not landed its
own yet. It has landed them, so that condition is permanently false and the line
is gone; what it was guarding — that `experimental` is not canon under another
name — is asserted in `profile.rs`'s
`experimental_is_the_rejected_candidates_switched_on_not_canon_renamed`, which
fails on the commit that breaks it rather than only for whoever happens to play
that profile.

### Where your personal best goes, and what moving the default did to it

A finished run is filed as `runs/<map>.<profile>.s3d`, so the default session's
personal best is now `runs/coil.straf3.s3d` where it used to be
`runs/coil.cpm.s3d`.

**Nothing committed was orphaned by that** — no `.s3d` exists anywhere in this
tree. What it affects is a `runs/` directory you already have locally:

- The old file is **not** touched, and `--profile cpm` still loads and races it
  exactly as before. The physics digest did not move at the freeze, so
  `ghost.rs`'s mismatch check — which binds a recording to the world and the
  physics, never to a name — still passes.
- A default session, though, looks for `runs/coil.straf3.s3d`, does not find it,
  and starts **with no personal best and no ghost** until you set one. That is
  by design and not a fault: a time set under a profile called `cpm` is filed
  and ranked as a `cpm` time.
- **Do not migrate it by copying the file.** A `.s3d` records the profile name
  it was set under, and the client refuses to *race* one whose name is not the
  session's — so `cp runs/coil.cpm.s3d runs/coil.straf3.s3d` gives you a
  straf3 session with no ghost and this in the log:

  ```
  [WARN  straf3_game::app] not racing a run set under the `cpm` profile in a `straf3` session. […]
  [WARN  straf3_game::app] the personal best at runs/coil.straf3.s3d is not being raced this session
  ```

  **The refusal covers the ghost and not the clock.** A recording that cannot be
  raced is still adopted as the time to beat — deliberately, so that your own
  record under geometry the compiler has since changed still has to be beaten —
  and that rule does not distinguish a changed world from a foreign profile
  name. So the copied `cpm` time *does* gate whether your straf3 run is saved,
  while never appearing on screen.

  It costs nothing today, because the two profiles are numerically equal and the
  times are genuinely comparable. It would cost something with a file copied out
  of `runs/<map>.experimental.s3d`, whose time was set with crouch slide and
  wall jump available. Either way the honest way to get a straf3 personal best
  is to set one; the digest cannot help here, since equal profiles fold to equal
  digests and the recorded name is the only thing separating them.

---

## Record a run and check it reproduces

Every recording names the world it was made in, and every reader prints the same
64-bit checksum of the final state. **That equality is the claim**, and it is
what survives a change to the simulation state; the literal value is an
illustration of one build's output.

Two rules decide whether you get a usable file:

> - **The recording is written only on a clean exit** — closing the window, or
>   letting `--exit-after` end the run. The file is written after the event loop
>   returns, so killing the process (Ctrl-C, `kill`, a closed terminal) skips
>   that write entirely: you get no file rather than a truncated one, however
>   long you played, and nothing is printed to say so.
> - **R discards the recording so far.** See Controls. Play an attempt and then
>   exit; do not press R and then close the window.

The destination itself is no longer a way to lose a session. A missing directory
is created — `--record runs/tonight/attempt.txt` makes `runs/tonight/` and
proceeds, the same choice `pb::store` already made for personal bests. A
destination that genuinely cannot be written is refused **before the window
opens**, exit 1, rather than discovered afterwards:

```
straf3: cannot record to <path>: <the OS's reason>. Refusing to start rather than discovering this after the session, when the commands only exist in a process that has exited.
```

The text after the colon is the platform's and differs between Linux and
Windows for the same fault; the straf3 half is the part to match on. An existing
recording keeps its contents through that check — the file is opened, not
truncated — so a session that fails does not also destroy the last good file. A
stray zero-byte file means a session recorded nothing, not that a run was lost.

The three readers, on a flat-world recording that needs no map:

```
cargo run -p straf3-game --bin straf3 -- --world flat --profile cpm --record /tmp/flat.rec
# play — strafe, jump — then close the window
cargo run -p straf3-game --bin straf3 -- --replay /tmp/flat.rec
cargo run -p straf3-sim --bin straf3-headless -- /tmp/flat.rec
cargo run -p straf3-game --bin straf3 -- --replay /tmp/flat.rec --frame-ms 1,97,3,250,8
```

> **`--profile cpm` on the first line is a workaround, marked rather than
> hidden.** It would otherwise be unnecessary — the client's default is
> `straf3`. But a recording is written in `straf3-headless`'s own input format
> and carries the profile it was made under as a `profile <name>` directive, and
> **`straf3-headless` does not know the name `straf3`.** Its parser accepts
> `cpm` and `vq3` only, so it refuses a default recording and exits 1:
>
> ```
> $ cargo run -p straf3-sim --bin straf3-headless -- /tmp/flat.rec
> straf3-headless: /tmp/flat.rec: line 2: unknown profile `straf3` (cpm|vq3)
> ```
>
> `tools/straf3-webcheck`'s fixture parser carries the same two-name table and
> has the same gap.
>
> Recording under `cpm` sidesteps it and costs nothing here: the two profiles
> are numerically equal, so the three readers are being compared on the same run
> either way. It is a real hole all the same — the client can now produce a
> recording the tree's own headless reader refuses — and the fix is a `straf3`
> arm in both parsers, which is a change below the seam and not in this client.
> Until it lands, a recording you intend to hand to `straf3-headless` has to be
> made under a name it knows.

All four print one checksum, and the three readers must agree. The last one is
the one that matters most: the same input on a deliberately hostile frame
schedule reaches the identical state. Rendering is decoupled from simulation
stepping, and this is what says so.

Play for a few seconds before closing — an unattended, input-free recording
reproduces trivially and proves nothing.

That property is demonstrable on a fixture that ships, rather than on a
temporary file only its author ever had. On the release build of this tree:

```
$ straf3 --replay probes/coil-course/results/coil-run.txt --map assets/maps/coil.map
  checksum      0xf3cabd183c90d8d7
$ straf3 --replay probes/coil-course/results/coil-run.txt --map assets/maps/coil.map \
         --frame-ms 1,97,3,250,8
  checksum      0xf3cabd183c90d8d7
```

Identical, as required. Expect the *value* to change when the simulation state
gains a field; expect the *equality* not to.

Which is exactly what happened: these two literals read `0x9a854d1a3653d8b7`
until 2026-08-29, they were re-derived on this tree and they now read
`0xf3cabd183c90d8d7`. The sentence above predicted it before it happened. The
equality it protects never moved.

**`[reported]`:** the full loop — play, record, replay — has since been closed on
the 3060 Ti. A session driven by `--play` and re-recorded produced 864 commands,
a 5096 ms run, the same checksum as its source, and directives and `cmd` lines
byte-identical to `coil-run.txt`. This document's author did not run that.

`straf3-headless` lives below the seam and only knows `empty` and `flat <z>`.
It has no way to spell a compiled map, so it refuses a map recording by name and
exits non-zero rather than silently running it somewhere else:

```
$ cargo run -p straf3-sim --bin straf3-headless -- probes/coil-course/results/coil-run.txt
straf3-headless: probes/coil-course/results/coil-run.txt: line 10: unknown world `map` (empty|flat <z>)
$ echo $?
1
```

That is also why the coil fixture above is replayed through `straf3` in replay
mode — which opens no window and creates no adapter — rather than through
`straf3-headless`.

---

## Checking the build

```
cargo test --workspace                 # everything
cargo xtask check-seam                 # nothing below the line reaches above it
cargo xtask determinism                # one command stream, four targets, one digest
cargo run -p straf3-render --example offscreen        # the world, to PPM
cargo run -p straf3-devtools --example hud-offscreen  # the overlay, to PPM
```

> **`cargo xtask determinism` must run with `CARGO_TARGET_DIR` unset.** It looks
> for each target's artefact at the workspace-relative path, so an override makes
> it exit non-zero saying the binary "was not produced". That is a loud
> infrastructure failure rather than a silent pass — nobody gets a false green —
> but an agent following ordinary shared-build-cache hygiene will hit it and
> conclude determinism has broken when it has not.

`cargo xtask determinism` is worth understanding precisely, because it is easy
to overstate. It builds **`tools/det-runner`** — not the game — and runs one
reference command stream on four targets: `x86_64-unknown-linux-gnu`,
`x86_64-unknown-linux-musl`, `x86_64-pc-windows-gnu`, and
`wasm32-unknown-unknown` under Node. Every target must agree with every other,
bit for bit, across all cases. So what is proven is that **the simulation
produces identical results on a wasm target, on musl, and on Windows** — a
stronger and narrower statement than "the wasm build matches native".

The two offscreen examples need a GPU adapter — including a software one — and
are examples rather than tests for exactly that reason: a test that fails on a
machine with no adapter punishes the correct environment.

---

## Not proven yet

Stated plainly so nothing above is read as a promise. Each item says what would
retire it.

- **The overlay has been watched on a real GPU — `[reported]`.** A prior session
  played the Windows build on the RTX 3060 Ti and reported the overlay legible:
  ground 320 ups, a strafejump to 648 ups, the start trigger firing, and 7.928 s
  on the clock when `--exit-after` ended the session. This document's author did
  not see it, and **no screenshot of that session exists in this repository** —
  the images in the tree are from the `wasm-render` probe and, since the browser
  wave, `docs/web/evidence/r17-browser-window.png`, which is the *browser*
  client on that GPU and says nothing about the native overlay. *Retired by:* a
  window-only screenshot of the native session committed to the tree and cited
  here by path.

- **The personal-best and ghost loop has been closed once, and its evidence has
  not been committed.** This is the sharpest remaining gap and it is worth
  stating exactly, because the code is not the problem.

  `[reported]`: a run was completed on the 3060 Ti, saved as a first personal
  best at `0:05.096`, and raced in a second session against a ghost re-simulated
  over 638 states, with a slower variant finishing `+16 ms` against it. What does
  **not** exist is the evidence: no screenshot, because the capture tool was not
  ready and a hand-taken desktop grab is forbidden by the standing image rule
  above — the right call was to take none rather than take a forbidden one. And
  the `.s3d` was deliberately not committed.

  That last decision is the interesting one. `crates/straf3-replay/src/identity.rs`
  folds the physics digest from an exhaustive destructure of `PhysicsProfile`
  with no `..`, on purpose — a new movement constant is a new way for two builds
  to disagree, so it must be a new input to the digest — and
  `crates/straf3-game/src/ghost.rs` turns a mismatch into a refusal to load. So a
  `.s3d` captured **before** straf3's own movement constants land stops loading
  **after** they do. Committing it now would ship an artefact guaranteed to be
  rejected by the code that reads it. *Retired by:* a screenshot and a `.s3d`
  captured after those constants are final — not by the loop having worked, which
  it has.

- **No browser run has been recorded, and nobody has played the browser client.**
  The URL half of this gap is now closed and the run half is not, so read the
  two separately.

  *What is now shown*, in `docs/web/evidence/r17-browser.txt`, captured on this
  Windows host against the RTX 3060 Ti: the bundle builds (230,724 B gzipped),
  `node crates/straf3-game/web/serve.mjs` serves it, and
  `http://127.0.0.1:8790/play/coil?p=cpm` opens in a stock Chrome 151 and
  **runs**. A hardware WebGPU adapter is acquired (`adapter.info` reports vendor
  `nvidia`, architecture `ampere`); `coil.map` is fetched and compiled by the
  wasm build to collision digest `0x47263b8845d8bb4b` — **the same digest a
  native `x86_64-pc-windows-msvc` build compiles it to**, which is a
  cross-target result the determinism gate does not cover, because that gate
  builds `straf3-det-runner` and never compiles a `.map`
  (`docs/web/evidence/r8-map-compiler-crosstarget.txt`). The session runs `cpm`
  physics (`4350ccc31bec5d4c`) at 125 Hz. Pointer lock is taken, mouse-look
  turns the view, and strafejumping accelerates past ground speed to 470 ups
  down the full strafe corridor. Frame pacing while playing: median 6.1 ms
  (~164 fps) on a 165 Hz display, p99 6.2 ms, p99.9 7.3 ms, 0.05 % of frames
  over budget — and, re-measured with the host pinned at 100 % CPU, the same
  6.1 ms median with the tail degrading to p99.9 24.3 ms and 0.36 % over.
  There is a screenshot: `docs/web/evidence/r17-browser-window.png`.

  *What is still not shown, and is the whole of what remains.* **No human has
  played it** — every input in that transcript was dispatched over the DevTools
  protocol by `crates/straf3-game/web/drive.mjs`, so nothing here measures input
  latency as a hand feels it. That is exactly where a browser would be expected
  to lose: the browser offers `fifo` as its **only** present mode, with
  `frame_latency=2`, so the latency-for-tearing trade the native build can make
  is unavailable. And **no run has been recorded in the browser**, because
  `RunSink` fires only at the finish trigger and coil cannot be walked there —
  its last jump needs ~425 ups onto a ledge that cannot be climbed, and the ramp
  wave before it needs ~575 ups. The scripted driver reaches 470 ups and is
  stopped by the course, which is a fact about the bot and not about the
  browser. So the digest round trip is **unproven in both directions**: no
  `.s3d` was captured out of a browser and none was re-simulated natively.
  *Retired by:* a run played in the browser by a person, carried out as a
  `.s3d`, that `webcheck resim` agrees with — both digests and both commands
  recorded, and the report's `command rate` line reading 125 Hz.

- **No leaderboards, no records service.** Also in scope in that parallel
  session, for the same reason and with the same caveat. This was previously
  listed here as out of scope; that is no longer true.

- **Pacing measurements now exist. The 240 Hz claim still does not.** This entry
  used to say no frame-time numbers had ever been taken from the real GPU. That
  is no longer true: `probes/pacing/` holds sixteen CSVs from the RTX 3060 Ti at
  1920×1080/165 Hz, across three workloads (static, `--play`, frame-latency) in
  both present modes, with two contended runs quarantined in
  `results/discarded/` rather than deleted. Each file's own header records the
  present mode the surface **granted** — not merely the one requested, which is
  all the client used to record, and which meant every uncapped number ever
  taken from it asserted a mode nobody had confirmed.

  What is *still* unproven here is narrower and worth keeping: **the vision's
  240 Hz-class budget remains unvalidated, because there is no 240 Hz display on
  this machine.** No number in `probes/pacing/` speaks to it, and none can.
  *Retired by:* the same measurements taken on a 240 Hz panel.

  One caveat that belongs with the numbers rather than beside them: the
  host-contention verdict recorded in each run counts **build processes by
  name**. A saturating process that is neither `cargo` nor `rustc` — a runaway
  `rustfmt`, for instance, which happened during this wave — is not counted. The
  load average recorded alongside it is the complete instrument; the named check
  is not. Read the two together.

- **Regenerating a committed run artefact costs a GPU session.** A `.s3d` can
  also be produced headlessly by the test harness, and that is the cheapest way
  to make a staleness check go green — but doing so silently converts *evidence
  of a run on real hardware* into *a fixture made by a test harness*, while this
  document goes on describing it as the former. If you regenerate headlessly,
  change the claim in the same commit.

- **No sound, weapons, menus or live multiplayer.** Out of scope for this wave.

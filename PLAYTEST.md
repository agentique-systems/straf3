# Playtesting straf3

You play; the lab reads. Two things come back from a session: your answers to
the checklist in §3, and the `.rec` files from §2. Free text beats a rating —
"I lost 80 ups on that ramp and I don't know why" is worth more than any score
out of ten, because the first one is a lead and the second one isn't.

Read this before playing. It is four sections and it is short on purpose.

[`PLAYING.md`](PLAYING.md) is the longer document, for when you want to know how
something works rather than what to do.

---

## §1 — A build that runs on your GPU

From the repository root, in the WSL shell:

```sh
cargo build --release --target x86_64-pc-windows-gnu -p straf3-game --bin straf3
./target/x86_64-pc-windows-gnu/release/straf3.exe
```

That is a native Windows process on your real GPU, launched from WSL. It is not
the software-rendered Linux build, and the difference is the whole point: frame
pacing and input latency measured on the Linux side are fiction.

Three things that quietly ruin a session:

- **Run it from the repository root.** `assets/maps/coil.map` is resolved
  relative to the working directory. From anywhere else you get a flat empty
  plane and one warning line you won't be looking at — a perfectly playable
  session that measures nothing.
- **Watch the console the first time.** The adapter line should name your GPU.
  If it says `llvmpipe` / `type=Cpu`, you are on the software path.
- Click the window to capture the mouse; **Esc** releases it.

To watch a recording back in the rendering client rather than playing it:

```sh
./target/x86_64-pc-windows-gnu/release/straf3.exe --play playtest-01.rec \
    --map assets/maps/coil.map
```

`--play` takes the recording's own rate, profile and world; your movement input
and **R** are ignored while it runs, and the window stays open when the stream
ends — so `--exit-after` is what ends an unattended playback.

> **Not in the tree yet, and marked rather than left blank:** the repeatable
> screenshot command. It is being built now, and this section gets it — with its
> output path — when it lands.

**When you want to show us something on screen, use that capture command — not
your system's screenshot key.** It photographs the straf3 window and nothing
else, so no part of the rest of your screen travels with it: no browser tabs, no
taskbar, no whatever else you had open. Anything you send may end up in the
repository's history, and history is hard to un-publish. Until the command
exists, describe what you saw in words rather than sending a picture of your
desktop.

---

## §2 — Recording, so the run comes back as data

```sh
./target/x86_64-pc-windows-gnu/release/straf3.exe --record playtest-01.rec
```

The destination looks after itself: a missing directory is created, and a
destination that genuinely cannot be written makes the client **refuse to start**
rather than discovering it after you have played. If it starts, your recording
has somewhere to go.

**Three rules still decide whether the file is worth anything.** The first two
have each already cost somebody a session:

1. **The file is written only on a clean exit.** Close the window normally, or
   pass `--exit-after <ms>` to end it on a timer. Ctrl-C, killing the terminal,
   or closing the shell writes *nothing* — however long you played, and with
   nothing printed to say so.
2. **R restarts the recording.** Respawn resets the clock, sends the ghost back
   to the start line, and **throws away everything recorded since your last
   respawn**. A respawn isn't a command, so a recording spanning one couldn't be
   re-simulated. Play an attempt, then exit. Do not press R and then close the
   window.
3. **`--exit-after` counts from when the process starts, not from when you start
   playing.** Adapter creation and window mapping are inside the budget, so
   budget about two seconds of startup on top of however long you mean to play.
   Cutting it fine ends the session early and hands back a recording that looks
   complete and isn't — which is the exact failure this document exists to
   prevent.

One file per attempt, named so you can tell them apart: `playtest-01.rec`,
`playtest-02.rec`. A zero-byte file means that session recorded nothing; it
doesn't mean a run was lost.

To check what you recorded, re-simulated with no window:

```sh
./target/x86_64-pc-windows-gnu/release/straf3.exe \
    --replay playtest-01.rec --map assets/maps/coil.map
```

Pass `--map`. Without it, a recording made in the map world is **refused** —
exit 1, with a message telling you to pass it — rather than quietly replayed
against a flat plane and reported as though it were your run.

---

## §3 — What to feel for

Every question below has an answer that is a **number, a place, or a yes/no** —
nothing here should be answerable with "felt fine". They come from the five
things `docs/VISION.md` asks of a movement mechanic: simple to invoke but hard
to master, composable with other mechanics, deterministic and clearly
attributable, productive of route *choices* rather than one mandatory
execution, and readable to a player watching.

Answer what you can; skipping half of it is fine and says something too.

**A. Speed you can read** *(is it attributable?)*

1. Circle jump from the spawn — what is the highest ups the overlay shows you in
   one jump?
2. Find the longest open stretch you can and strafe it for as long as it lasts.
   Does the number plateau, and at what? Or was it still climbing when the
   stretch ran out?
3. The last time you lost speed: could you tell *why* from what was on screen,
   or only *that* you had? One concrete example.

**B. The emergent vocabulary** *(is it learnable?)*

4. Take the ramp at roughly 320 ups, then again at roughly 450. Two numbers out.
   Did the faster entry pay more, less, or the same?
5. Did any landing give you a large, sudden speed gain you didn't ask for — or
   kill your speed dead? Where, and did anything on screen distinguish that drop
   from one a fraction taller that behaved normally?
6. Step up an edge and read the number before and after. What did it cost?
7. Did the amber `SLIDE` state appear anywhere you didn't expect it? Which
   surface?

**C. Route** *(does it produce choices, or one execution?)*

8. Did you find more than one line through `coil`? Which was faster, and by how
   much on the clock?
9. Is there a section with exactly one way through and no alternative? Which?

**D. Readability** *(can a player follow it?)*

10. Can you read the clock and the split while moving at speed, or do you have
    to stop watching the world to do it?
11. Anything you went looking for on the overlay and couldn't find?

**E. Connection** *(does it feel like yours?)*

12. Does turning fast feel connected, or does the view trail the mouse?
13. Did anything feel like the game deciding for you rather than you executing?

**F. `--profile experimental`** — crouch slide, dash, wall interaction.

**One gate, and the client answers it for you.** Start it and read the console.
If you see this line, stop — there is nothing to measure yet:

```
`experimental` is currently CPM's constants — straf3-sim has not landed
PhysicsProfile::experimental() yet, so this session is experimental in name and
record-keeping only, not in how it plays
```

That line comes from the profile comparing itself against `cpm`, so it cannot
be wrong about its own state, and it removes itself when the real constants
land. Past it:

14. **Dash** — a movement tool or a teleport? Does it compose with a strafejump,
    or replace the need for one?
15. **Crouch slide** — does it preserve speed you earned, or hand you speed you
    didn't?
16. **Wall interaction** — when it didn't fire, could you tell from feedback
    why?
17. For each of the three: **keep, cut, or revise** — and one sentence why.
    Cutting all three is a success, not a failure.

Personal bests set under `experimental` are filed separately
(`runs/<map>.experimental.s3d`) and never ranked against `cpm` or `vq3`, so you
can play it freely without polluting anything.

---

## §4 — What to send back

- The `.rec` files.
- Your answers to §3, in any form.
- Anything that surprised you, especially if you can't explain it. The
  unexplained ones are the ones worth chasing.
- Images **only** from the capture command in §1, never from your system's
  screenshot key — it captures the game window alone, and that is the point.

If a session produced no file, say so and say what you did — that is a bug
report about §2, and §2 is where playtests are lost.

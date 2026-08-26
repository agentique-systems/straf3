# Playtesting straf3

You play; the lab reads. Two things come back from a session: your answers to
the checklist in §3, and the `.rec` files from §2. Free text beats a rating —
"I lost 80 ups on that ramp and I don't know why" is worth more than any score
out of ten, because the first one is a lead and the second one isn't.

Read this before playing. It is four sections and an appendix, and it is short
on purpose.

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

To photograph what is on screen, with the game running, from the repository
root:

```sh
cargo build --release --target x86_64-pc-windows-gnu -p straf3-capture --bins
./target/x86_64-pc-windows-gnu/release/straf3-capture.exe --out shot.png
```

It writes exactly where `--out` says, so put it where you want it. Useful
options: `--wait-ms 20000` keeps looking while the client starts up,
`--settle-ms` waits after finding the window so a frame is on screen (default
300 ms), and `--list` shows every window that is open with the process behind
it. `--help` has the rest.

**It writes nothing at all rather than write the wrong picture**, and the exit
code says which refusal you hit:

| exit | what happened |
|---|---|
| 0 | captured, written, and checked not to be blank |
| 3 | written, but blank — the reason is on stderr, and the file is kept so you can look at it |
| 4 | no straf3 window found; **nothing written** |
| 5 | the window is covered by something else; **nothing written** |

Exit 4 is the one to expect if you run it before the game is up, or after it
has closed. It tells you what it looked at, and it names any window that had a
matching *title* but was not the straf3 process — an editor with a straf3 file
open is titled `straf3 — something` and is not the game. That distinction is
not cosmetic: capturing such a window produces a perfectly valid, perfectly
non-blank picture of your document, and it happened here before the check
existed.

**When you want to show us something on screen, use that capture command — not
your system's screenshot key.** It photographs the straf3 window and nothing
else, so no part of the rest of your screen travels with it: no browser tabs, no
taskbar, no whatever else you had open. Anything you send may end up in the
repository's history, and history is hard to un-publish.

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
nothing here should be answerable with "felt fine". They are tagged against the
eight qualities section 4.2 of `docs/VISION.md` asks of a strong mechanic:
understandable at a basic level; difficult to perfect; responsive;
deterministic; composable with other mechanics; useful in multiple situations;
capable of supporting player expression;
readable through visual, audio, and diagnostic feedback.
The tags are many-to-many: one quality is tested by several questions, and one
question tests several qualities. The appendix records every tag that changed.

Answer what you can; skipping half of it is fine and says something too.

**A. Speed you can read**
*Tests: understandable at a basic level; difficult to perfect;
readable through visual, audio, and diagnostic feedback.*

1. Circle jump from the spawn — what is the highest ups the overlay shows you in
   one jump?
2. Find the longest open stretch you can and strafe it for as long as it lasts.
   Does the number plateau, and at what? Or was it still climbing when the
   stretch ran out?
3. The last time you lost speed: could you tell *why* from what was on screen,
   or only *that* you had? One concrete example.

**B. The emergent vocabulary**
*Tests: understandable at a basic level; difficult to perfect; deterministic;
composable with other mechanics; useful in multiple situations;
readable through visual, audio, and diagnostic feedback.*

4. Take the ramp at roughly 320 ups, then again at roughly 450. Two numbers out.
   Did the faster entry pay more, less, or the same?
5. Did any landing give you a large, sudden speed gain you didn't ask for — or
   kill your speed dead? Where, and did anything on screen distinguish that drop
   from one a fraction taller that behaved normally?
6. Step up an edge and read the number before and after. What did it cost?
7. Did the amber `SLIDE` state appear anywhere you didn't expect it? Which
   surface?

**C. Route**
*Tests: useful in multiple situations; capable of supporting player expression.*

8. Did you find more than one line through `coil`? Which was faster, and by how
   much on the clock?
9. Is there a section with exactly one way through and no alternative? Which?

**D. Readability**
*Tests: readable through visual, audio, and diagnostic feedback.*

10. Can you read the clock and the split while moving at speed, or do you have
    to stop watching the world to do it?
11. Anything you went looking for on the overlay and couldn't find?

**E. Connection**
*Tests: responsive; capable of supporting player expression.*

12. Does turning fast feel connected, or does the view trail the mouse?
13. Did anything feel like the game deciding for you rather than you executing?

**F. `--profile experimental`** — crouch slide, dash, wall interaction.
*Tests: deterministic; composable with other mechanics;
capable of supporting player expression;
readable through visual, audio, and diagnostic feedback.*

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

---

## Appendix — reconciling §3 to section 4.2 of the vision

Section 4.2 of `docs/VISION.md` lists eight qualities a strong mechanic should
generally have. §3's preamble used to cite a shorter, superseded list, and the
group headers A–E carried tags from that list. **The preamble and the group tags
were rewritten. No question was added, deleted, renumbered or reworded** — all
seventeen still stand in §3 exactly as they read before, and each has a row in
the table below.

Two of the superseded criteria — clear attribution, and route *choices* rather
than one mandatory execution — are not literally among the eight. The questions
that hung off them (3, 8, 9) were retained and re-tagged, not dropped: each
still tests something section 4.2 asks for.

Tags are many-to-many, so a question can appear under more than one quality and
a quality under more than one question. The per-question tags below are the
covering map fixed by the conservation decision, with one disclosure: that map
does not name question 9, because question 8 already covers the qualities it
would contribute. Question 9 is tagged here to the same two qualities as the
rest of its group, which is what it tests — a section with exactly one way
through is a section where the mechanic is *not* useful in multiple situations
and where expression is closed off.

| Question | Wording | Old tag | New tag(s) |
|---|---|---|---|
| 1 | unchanged | A — "is it attributable?" | understandable at a basic level |
| 2 | unchanged | A — "is it attributable?" | difficult to perfect |
| 3 | unchanged | A — "is it attributable?" | readable through visual, audio, and diagnostic feedback |
| 4 | unchanged | B — "is it learnable?" | difficult to perfect; composable with other mechanics |
| 5 | unchanged | B — "is it learnable?" | deterministic; readable through visual, audio, and diagnostic feedback |
| 6 | unchanged | B — "is it learnable?" | understandable at a basic level |
| 7 | unchanged | B — "is it learnable?" | useful in multiple situations |
| 8 | unchanged | C — "does it produce choices, or one execution?" | useful in multiple situations; capable of supporting player expression |
| 9 | unchanged | C — "does it produce choices, or one execution?" | useful in multiple situations; capable of supporting player expression |
| 10 | unchanged | D — "can a player follow it?" | readable through visual, audio, and diagnostic feedback |
| 11 | unchanged | D — "can a player follow it?" | readable through visual, audio, and diagnostic feedback |
| 12 | unchanged | E — "does it feel like yours?" | responsive |
| 13 | unchanged | E — "does it feel like yours?" | capable of supporting player expression |
| 14 | unchanged | F — none; the group carried no property tag | composable with other mechanics |
| 15 | unchanged | F — none; the group carried no property tag | deterministic |
| 16 | unchanged | F — none; the group carried no property tag | readable through visual, audio, and diagnostic feedback |
| 17 | unchanged | F — none; the group carried no property tag | capable of supporting player expression |

Every quality section 4.2 names is tested by at least one question: 1 and 6
(understandable at a basic level), 2 and 4 (difficult to perfect), 12
(responsive), 5 and 15 (deterministic), 4 and 14 (composable with other
mechanics), 7, 8 and 9 (useful in multiple situations), 8, 9, 13 and 17
(capable of supporting player expression), 3, 5, 10, 11 and 16 (readable
through visual, audio, and diagnostic feedback).

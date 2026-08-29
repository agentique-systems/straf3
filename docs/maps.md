# The Straf3 map portfolio

Two first-party maps, and two facets that were commissioned and not built.

Every number here is measured unless it says otherwise, and every measurement
names the artefact that produced it. Where a claim could not be established, the
question is written down as open rather than answered by guess — that is this
milestone's evidence bar, set by operator decision: no claim may rest on a human
having played something.

| Map | Facet | Source | Measurements | Completing run |
|---|---|---|---|---|
| `coil` | The line. One route, three gates, no choices. | `assets/maps/coil.map` | `probes/coil-course/results/coil.txt` | `probes/coil-course/results/coil-run.txt` |
| `cleave` | Route choice. One course, two ways through the middle of it. | `assets/maps/cleave.map` | `probes/course-lab/results/cleave.txt` | `probes/course-lab/results/cleave-hybrid.txt` |
| *precision* | Tight landing windows and exact angles. | **NOT BUILT** — out of time | — | — |
| *flow* | Sustained rhythm rather than discrete gates. | **NOT BUILT** — out of time | — | — |

The two missing maps are named rather than left to be inferred. They were
commissioned, they were not started, and nothing in this document should be read
as covering them.

## coil — the line

Straf3's first course. A circle jump in the start room, 1536 units of corridor,
a ramp wave, a gully and a finish leap. Its header is the documentation standard
the rest of the portfolio is held to: every section gates on a number that was
measured, and every claim says *where* it holds. The 90-degree arc it describes
is scoped "executed inside the room", before the start line — that scoping is
the specific quality to copy.

Measured gates: 575 ups for the ramp-wave V, 600 to clear the gully, a 425..900
landing window at the finish.

## cleave — route choice

A common corridor, then a fork: a **high line** entered by climbing *west*,
across the course rather than along it, leading to a 448-unit leap; against a
**low line** that is flat, ungated and longer, sweeping east then back west
around two blocks.

Measured gates, swept over speed × approach yaw so the numbers are
minimum-over-approaches rather than one column:

- **the leap** — 600 ups head-on, 620 across a ±15° band. Fall short from
  560..590 ups and you leave the slot with 145..153. You keep about a quarter.
- **the finish** — 510 ups head-on, 530 across the band.

**The fork does not work on the numbers measured.** Fork to rejoin: low line
5888 ms, high line 7040 ms, deliberate hybrid 10560 ms. The harder branch is
1152 ms *slower*, so as measured it costs a second and buys nothing. That is
published as a negative with its coverage rather than left as an open question,
and the map was **not** widened to turn the number around. Only the hybrid has
been driven end to end, so the other two times are segments of lines that failed
later — the caveats are in the results file.

Whether any run has yet *engaged* the fork is unresolved: the launch predicate
that would settle it separates jumped from walked, not leapt from declined.

## A design rule the portfolio earned, for whoever cuts the next map

**Prefer a gate with two bounds to a gate with one.**

There is no absolute speed clamp in the simulation — `max_speed` 320 is the
`g_speed` wish-speed base, not a ceiling — and CPM air control accumulates speed
on a sustained turn. So a gate expressed purely as a *minimum* entry speed is
monotone in the resource it gates on: more is always weakly better, and the gate
discriminates on patience rather than on execution. A gate whose landing is
bounded — by a far edge, a pit or a ceiling — is non-monotone, has an optimum
you have to hit, and tests execution.

Two things this rule is **not**. It is not "threshold plus open region equals
defect", which would condemn coil's own start room. And it is not a claim that
farming speed is a bypass: the clock runs while you circle, so a reservoir bought
with 43 seconds of simulated time is a bad run, not an exploit. The measured
number that matters is speed *delivered at the gate* on a time-competitive line,
and on cleave that is 620 ups against a 620 threshold — nothing that was trying
to go fast arrived fast.

## Conventions

- **Checkpoint numbering comes from source order**, and only from source order;
  the compiler assigns it while walking the entity list. The `count` key is read
  and reported by `tools/straf3-agent` and deliberately **not** enforced by it.
  coil writes 1-based counts, cleave writes 0-based; both pass, because the
  check compares the *sequence* rather than the base. Unifying them is unfinished
  work.
- **Checkpoint semantics are per-map, declared in the map.** cleave declares
  `"checkpoint_mode" "splits"` on worldspawn — a run crossing START and FINISH
  without visiting a checkpoint is complete and valid. Nothing in the tree reads
  that key today; it is a declaration awaiting a reader, and the engine does not
  enforce it.
- **Digests are quoted with the cut they were measured on.** cleave's collision
  digest is `0xd2d7d01e8f9d03ef`, reproduced on this host at this commit. No
  cross-target claim is made: map compilation is not in the determinism suite,
  and nothing has measured it in either direction.
- **Print a compiled map's geometry** — per-solid hull extents, trigger volumes
  and the collision digest — with
  `cargo run -p straf3-map --bin mapdump -- assets/maps/<map>.map`.

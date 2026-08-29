# What the two first-party courses actually look like

Measured, from `straf3-agent plan`, on the tree at `0c0e1f5` plus this crate.
The full printouts are `coil-plan.txt` and `training-crouch-slide-plan.txt`;
this file is the one table worth reading side by side, because it decides what
the agent's search has to be able to do.

## The courses

| | `coil` | `training-crouch-slide` |
|---|---|---|
| solid hulls | 26 | 10 |
| trigger volumes | 4 (all timing) | 4 (all timing) |
| compiler warnings | none | none |
| spawn in open air | yes | yes |
| steps | start, cp0, cp1, finish | start, cp0, cp1, finish |
| course length (sum of legs, spawn excluded) | 3 409.7 | 3 264.0 |
| total rise | +80 | 0 |
| leg bearings | 90.0, 90.0, 90.0 | 90.0, 90.0, 90.0 |
| **sharpest turn between course legs** | **0.0°** | **0.0°** |

`coil`'s printout reports a sharpest turn of 22.2°, and that number is the
approach: the spawn sits at `x = -320` and the start line's aim is at `x = 0`,
so walking onto the line is a diagonal. Between the start and the finish, coil
turns by zero degrees, three times.

## What that means for the agent

**Neither first-party map turns.** Both are straight `+y` corridors with the
finish at the far end. That is why `probes/coil-course`'s greedy
one-step-per-window search, whose entire objective is `origin.y + 0.25 * speed`,
completes `coil` at all: on a course with no turn, maximising a single world
axis *is* following the route.

The two maps also share their corridor dimensions — both 448 units wide, with
timing volumes spanning `x −224..224`. Those shared numbers are precisely what a
two-map overfit would key on. None of them appears in `straf3-agent`'s source,
and its test fixtures deliberately use different dimensions so that reusing them
could not disguise it.

Two consequences, and they pull in opposite directions:

1. Completing `training-crouch-slide` is a real test of the **goal derivation** —
   the volumes, their order, and the aim points all come from a map this crate
   has never been tuned for — and of the agent's **input alphabet**: its lintel
   descends to `z = 48` and refuses a standing hull, so a run has to crouch, an
   input `coil` never asks for. It is *not* a test of the search.
2. Completing it is **not** evidence that the search reasons about route, and
   r11 now says so in its own text: completing a monotone corridor such as
   `coil` or `training-crouch-slide` does not discharge it, because the
   sort-triggers-by-axis heuristic completes those too. A map that would demand
   route reasoning does not exist in this tree yet.

Recorded here rather than discovered later: an agent that completes both of
these maps has cleared a lower bar than "generalises beyond coil" suggests, and
saying so is cheaper now than defending a number afterwards.

## The numbers' provenance

Every figure above is read off the two printouts, which are the tool's unedited
stdout. `straf3-agent plan` is a pure function of the map and the profile —
`the_same_map_derives_the_same_plan_twice` in `tests/first_party.rs` is what
says so — so re-running the two commands in the printout headers regenerates
these files rather than merely resembling them.

The bearing and turn columns go through `atan2`, which is not IEEE-specified.
They are printed to a tenth of a degree, they describe the course rather than
steer through it, and no decision in this crate is taken on one.

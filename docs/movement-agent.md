# The movement agent

`tools/straf3-agent` runs a straf3 course without a person. This document says
what it can do, what its numbers are worth, and which of its claims nobody has
checked. It is maintained as a description of the tree rather than as a plan, so
where a section says *not yet*, that is the current state and not an omission.

**State: the goal derivation is built and published. The search is not.** The
agent can read any compiled map and say where the course goes; it cannot yet run
one. Everything in *Times* below is therefore about the prior art, which is the
only thing in this tree that has completed a course.

## What it is, and why it is not `probes/coil-course`

`probes/coil-course` already completes `coil` without human input, and emits a
command stream the shipped `straf3 --replay` reproduces exactly: a 5 096 ms run,
clock started at 1 800 ms and stopped at 6 896 ms, 864 commands. That is real
and it is the baseline this crate has to beat on generality rather than on time.
Its published digest no longer reproduces, for a reason that is not a
determinism break — see *Checksums and the build they belong to*.

Two things about it do not survive a second map, and its own comments say both:

- **Its goals are hand-written.** The finish aim `mins.z + 48`, the switch at
  `y > mins.y - 384`, and the objective `origin.y + 0.25 * speed` are facts
  about `coil.map` typed into a program.
- **It is a probe**, with its own lockfile, deliberately not a workspace member.
  A probe is published evidence of a past run. Editing one after its results are
  published stops it being evidence, which is why this work is a new crate and
  `probes/coil-course/` is untouched.

`tools/straf3-agent` is a workspace member, resolving `glam` and the `straf3-*`
crates through the workspace `Cargo.lock` — the same reason `tools/det-runner`
is one. A run produced against a different resolution could not have its
checksum compared with one the shipped binary produces, which is the entire
evidence the agent exists to collect.

## The goal derivation

```sh
cargo run --release -p straf3-agent -- plan assets/maps/coil.map
```

prints the course a map implies: the start volume, the checkpoints in order, the
finish, an aim point for each, and the legs between them. Committed printouts
for both first-party maps are in `tools/straf3-agent/results/`.

Everything comes from the map. `straf3_map::CompiledMap::triggers` supplies the
volumes and their classification; `CompiledMap::collider()` — the same world the
shipped game collides against — supplies the ground; `PhysicsProfile` supplies
the player's size. **No coordinate, threshold, aim point or axis assumption from
any particular map appears in the crate's source**, and the command is the same
for every map: `straf3-agent plan <map>`. Nothing selects behaviour by map name.

### The two general rules

- **Horizontal — the centre of the volume's bounds.** A trigger is authored to
  be crossed, so the point furthest inside it in the plane the player runs in is
  the middle of it.
- **Vertical — where a player standing inside the volume would be.** A
  player-sized box is traced down through the volume, and the aim is the origin
  it comes to rest at, held clear of the surface by the same `SPAWN_CLEARANCE`
  the map compiler holds a spawn off the floor.

The vertical rule is where the prior art's constant went, and replacing it was
not cosmetic. On `coil`'s finish, `mins.z + 48` gives z 112 and the real
standing surface is z 80 — sixteen units, harmless. On `coil`'s **second
checkpoint** the same rule gives z −48 against a surface at z 64: it would aim
112 units underground. The probe never met that because it only applied the rule
to the finish.

### The two fallbacks, and why they are named

A fallback is reported as a `Note` in the printout, never substituted silently.

- **Largest piece**, when the bounds centre is not inside the volume. One
  trigger entity may own several brushes; the centre of an L-shaped start line's
  union is in the wall between its arms.
- **Volume centre**, when nothing standable is under the volume, or standing
  there would put the player outside it. A finish over a pit is a real thing to
  author, and the honest answer is the middle of the box plus a note.

Neither first-party map needs either — `the_general_rule_alone_suffices_on_every_map_we_ship`
in `tests/first_party.rs` is what says so, and it will fail the day one does.

### The two assumptions, stated rather than hidden

- **Checkpoint order is source order.** Defrag gives checkpoints no explicit
  index and `straf3-map` numbers them as it meets them. A map that declares them
  out of order gets a plan that visits them out of order, and nothing here can
  detect it. Every map with more than one checkpoint carries this note.

  There *is* a key that looks like an index and is not. Both first-party maps
  declare `"count"` on their checkpoints; `straf3-map` reads six keys out of a
  `.map` — `classname`, `origin`, `angle`, `angles`, `target`, `targetname` —
  and `count` is not one of them, so it orders nothing and reaches no compiled
  artefact. On both maps it happens to agree with source order, which is why it
  has never bitten anyone. The plan compares the two and reports a disagreement
  by name; it does **not** resolve one in `count`'s favour, because the compiled
  index is what every other reader in this tree sees and a crate that quietly
  preferred the other would be the one component disagreeing with the game.
- **Checkpoints do not gate the clock.** `RunState::finish` reads
  `TriggerSet::FINISH` alone, so a run that skips every checkpoint still
  produces a time. They are used as goals because they are the author's own
  statement of the route — which means a completed run has to report *which* of
  them it touched, or its time describes a route nobody chose.

## Two kinds of generality, which are not interchangeable

They are kept apart here on purpose, because conflating them is the easiest way
to overstate what has been shown.

- **Input generality** — the agent can produce an input a map needs that `coil`
  never needed. It bears on "is this agent reusable at all". It is real and it
  is narrow.
- **Route generality** — the agent can work out *where to go* on geometry whose
  goals are not in a line. It is the only thing that speaks to whether the
  search reasons about route, and the only thing that discharges a claim about
  generalising beyond `coil`.

A run that demonstrates the first is not evidence for the second, and this
document will not present it as such.

## What the search can and cannot reason about

There is no search in this crate yet. What can already be said, from measurement
rather than from expectation:

**Neither first-party map turns.** `coil` and `training-crouch-slide` are both
straight `+y` corridors; every leg of both courses reads a bearing of 90.0° and
a turn of 0.0°. The numbers are in
`tools/straf3-agent/results/first-party-geometry.md`. That is why a greedy
one-step search maximising `origin.y` completes `coil`: with no turn on the
course, maximising one world axis *is* following the route. The two maps also
share their corridor dimensions — both 448 units wide, both with triggers
spanning `x −224..224` — which is exactly the pair of numbers a two-map overfit
would key on. None of them appears in this crate, and the test fixtures
deliberately use different ones so that reusing them could not mask it.

The consequence is uncomfortable and is recorded before any search is written:
completing `training-crouch-slide` will demonstrate **input** generality — it
requires crouch, which `coil` never does — and will demonstrate that the goal
derivation reads an untuned map correctly. It will **not** demonstrate route
generality, because a monotone corridor does not ask for any. r11 says so in as
many words: completing `coil` or `training-crouch-slide` does not discharge it,
because the sort-triggers-by-axis heuristic completes those too.

Demonstrating route generality needs geometry where a one-step hill climber
measurably fails. No such geometry exists in this tree today. The one shape
that is already covered is the *derivation* half of it: a fixture whose second
checkpoint sits behind its first, so that the declared order and any sorted
order differ — `a_course_that_doubles_back_is_planned_in_the_declared_order_not_a_sorted_one`
in `src/course/tests.rs`. The plan follows the map. Whether a search can *run*
such a course is untested and unclaimed.

### Where the prior art's axis assumption actually lives

Worth stating precisely, because it changes what "goals derived from the map"
buys. In `probes/coil-course`, `+y` is not only in the scoring function — it is
in the **loop condition**: the search runs `while ... state.player.origin.y <
until_y ...`. A map with a turn does not make that bot score badly; it makes the
loop exit and the search stop.

So deriving goals from the trigger volumes, which is what this crate does, does
not by itself remove the axis. A termination test and a progress measure with no
world axis in them are a separate design problem, and this document will say
what replaced "y increases" when there is a search to say it about. Recording
the distinction now, before writing one, is cheaper than discovering it in a
run.

### Checkpoints are goals by contract, not by convenience

The run clock does not read them: `step.rs` advances `RunState` on
`TriggerSet::START` and `TriggerSet::FINISH` and on nothing else, and
`RunState::finish` requires only that the run is running. So "reached Finished"
is a weaker statement than it sounds — a run that skipped every checkpoint
satisfies it.

This crate therefore treats crossing every declared checkpoint, in declared
order, as part of what completing a course means, and any completion it reports
lists the crossings with their split times. A run that reached the finish
without them is reported as what it is: a shortcut, not a completion.

## Checksums and the build they belong to

`SimState::checksum` folds the whole simulation state, `RunState` included. That
is what makes it strong evidence: a replay agreeing on it did not merely follow
the same path, it started and stopped its clock on the same commands.

It is also what makes it **fragile in a specific, legitimate way**. Adding a
field to the state changes the digest of every run ever recorded, without any
run behaving differently. That has already happened here: commit `a604820` added
`Timers::slide_ms`, `dash_ms`, `wall_contact_ms` and `PlayerState::wall_normal`
for the candidate mechanics, and they are folded in `SimState::checksum`
alongside `double_jump_ms` — deliberately, because the mover branches on all
four and a replay that diverged with a *matching* digest would be worse than one
that diverged visibly. The mechanics were rejected and the profile field
reverted; the state fields were kept so a later wave can re-measure. So
`probes/coil-course`'s published `0x9a854d1a3653d8b7` no longer reproduces at
`HEAD`, while the trajectory and the clock reproduce exactly.

The rule this crate follows, and which anyone quoting one of its numbers should
follow:

- **A digest certifies** that two runs of *the same build of the state struct*
  visited the same states, on the same commands, with the same clock.
- **A digest does not certify** anything across builds. It is not a map
  identity, not a physics identity, and not a version-independent fingerprint.
- **What invalidates it:** any change to `SimState`'s folded fields, whether or
  not behaviour moved. A different map, a different profile and a different
  toolchain also change it, but those change behaviour too and are caught
  elsewhere.
- **Therefore every digest this crate publishes names the commit it was taken
  from**, next to the number. A bare hex value in prose is a claim with an
  expiry date nobody can see — `README.md` says so as a rule, and this tree has
  now been bitten by breaking it once.

A command stream additionally carries the map it was recorded against. A stream
replayed against the wrong map does not fail loudly; it runs, and the player
falls out of the world. So every stream this crate publishes is quoted together
with the map's `collision_digest`, and a verifier should check that before
comparing checksums at all.

## Times

**The agent has produced no times.** The only automated completion of a straf3
course is `probes/coil-course`'s, at 5 096 ms on `coil`.

When this crate does produce one, it will be a **bound, not a record**, and the
distinction is not a formality:

- an agent's time is an upper bound on what the course can be run in — it proves
  the course is completable in at most that, and says nothing about the best
  line;
- the speeds such a run reaches are lower bounds on what the movement affords;
- neither number is a course record, and neither should be quoted as one. A
  record is a claim about the best a *player* can do, and this milestone
  produces no human evidence by operator decision.

Any artefact this crate emits is machine-made and is labelled as such, so a
generated run is never mistaken for a played one.

## Which claims are unverified

- **That the derived plan is a route a player would take.** It is a sequence of
  volumes with a reachable-looking point in each. Nothing has walked it.
- **That the aim points are good targets to steer at.** They are points inside
  the volumes; whether steering at them completes a run is the next unit of work
  and is currently unknown.
- **That checkpoint source order is the intended order** on any map — see the
  assumption above. True for both first-party maps by inspection of their
  sources; unprovable in general.
- **Feel.** Nothing in this document is a judgement about how the movement
  feels. That is an open question in this milestone by operator decision.

## The profile the agent runs under, and a finding it inherits

The agent offers `cpm|vq3|experimental` and defaults to `cpm`. `straf3` is
deliberately absent: `PhysicsProfile::straf3()` landed in the simulation and its
client half did not, so `straf3-game` does not accept the name and a command
stream headed `profile straf3` would be refused by the very binary that has to
replay it. `PhysicsProfile::straf3()` is today bit-for-bit identical to
`PhysicsProfile::cpm()` — `the_canonical_profile_is_still_cpm_by_another_name`
in `src/profile.rs` asserts it — so running under `cpm` costs nothing but a
name. Closing that gap is requirement r1's work, not this crate's; when it is
closed, that test and `src/profile.rs`'s table are what have to change.

## Running it

```sh
cargo run --release -p straf3-agent -- plan <map.map> [--profile cpm|vq3|experimental] [--out <file>]
cargo test -p straf3-agent
```

The plan is a pure function of the map and the profile, so re-running the
commands in the committed printouts' headers regenerates those files rather than
merely resembling them.

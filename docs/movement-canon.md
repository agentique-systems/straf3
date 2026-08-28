# Canonical Straf3 movement

**Status of this document.** Part 1 is written. Part 3 is half written. Part 2
is not.

| Part | What it is | State |
|---|---|---|
| 1 | The criteria a mechanic must meet to enter canon | **written; amended three times; still frozen against candidate evidence** |
| 2 | The verdict on crouch slide, dash and wall jump | **not written** — waiting on the candidate sweep. §2.1 records the gate results that are decidable from the implementation alone |
| 3 | The frozen `PhysicsProfile::straf3()`, constant by constant | **§3.1–§3.7 written**: every inherited constant carries a grade or a stated choice, and the jump is measured. §3.8 and the constructor wait on Part 2 |

**No candidate has been judged against these criteria yet.** Part 1 therefore
still holds its pre-publication immunity, which is the most valuable thing this
document has: every threshold in it provably predates every number it will be
applied to. **No threshold has been edited this wave**, so §1.7 gains no fourth
amendment and nothing needs restating under the threshold-edit rule.

What *has* landed this wave is the half of the work that never depended on a
candidate number: Part 3's inherited constants, the measured jump that replaces
Part 3's arithmetic, and §2.1's gate results — which are properties of `step.rs`
rather than of the sweep, and two of which Part 1 pre-disclosed precisely so that
they could not later look like gates written after the numbers.

That order is the whole point and it is worth stating before anything else.

`docs/VISION.md` §22 lists **"the criteria through which a new mechanic becomes
canonical"** among the questions deliberately left open. §4.3 says only that "a
mechanic enters the canonical ruleset because testing demonstrates that it
improves Straf3", which names the evidence and not the standard. This document
answers that open question. Part 1 is that answer.

Part 1 was written and committed **before any measurement of any candidate
mechanic existed**, at a tree whose base is `965f02a`, at which
`tools/straf3-lab` measured the canonical vocabulary only and had no candidate
harness at all. Criteria written after the numbers are in are not criteria; they
are a justification for a decision already taken, and they are unfalsifiable
because the person writing them already knows which way each one has to point.

**The threshold-edit rule follows from that and binds the rest of this
document.** A number in Part 1 may be changed later. If it is, every verdict
scored under the old number must be restated under the new one in Part 2, in
public, naming the change — not silently re-scored. The criteria are revisable;
the record of what they used to say is not.

**Part 1 has been amended three times**, each after independent review and each
still before any candidate number existed. §1.7 records every change and what it moves. No
verdict has been scored under the superseded text, so nothing needed restating —
which is the entire benefit of writing the criteria first, and it expires the
moment the first candidate number is published.

---

## Part 1 — What a mechanic must be to enter canon

### 1.0 The shape of the decision

A candidate faces **eight gates** and then **seven weighed criteria**. Failing
one gate ends the case: no weighing afterwards, and no aggregate score in which
a gate failure can be outvoted.

**Where each gate's authority comes from, because it is not the same for all
eight.** The original draft of this document claimed the gates were all "things
the vision states Straf3 must not become, not things to be traded off". That
claim was checked against §4.4 and §21 directly and it does not hold for all of
them. It is replaced by this table, which is the honest version:

| Gate | Authority | Force |
|---|---|---|
| G2 incoherence | §4.4 "make the canonical movement language incoherent" | vision prohibition |
| G5 cooldowns | §4.4 "replace momentum mastery with cooldown rotations"; §21 "a cooldown-rotation game" | vision prohibition |
| G6 speed caps | §4.4 "impose arbitrary speed limits to compensate for poor design"; §21 | vision prohibition |
| G7 opacity | §4.4 "make important outcomes opaque or impossible to understand" | vision prohibition |
| G3 *second half* — no input may be ignored | §4.4 "automate execution that should belong to player skill" | vision prohibition |
| G1 determinism | §8 competitive integrity; §14's deterministic simulation boundary — **not** §4.2, which is the hedged list | vision prohibition |
| G3 *threshold of zero commands* | §4.4's anti-goal is conditional on "for visual spectacle"; §4.2's "responsive" is hedged | **canon's own choice** |
| G4 no new binding | §4.2's "the input vocabulary should remain *relatively* compact" — a hedge, in the hedged list | **canon's own choice** |
| G8 data, not a branch | `PhysicsProfile`'s own doc comment and `canon_frozen.rs` — the repository's doctrine, not the vision's | **repository doctrine** |

The two marked *canon's own choice* are revisable under the threshold-edit rule,
exactly as every weighed number is. **A candidate ended at G4, or at G3's
zero-command threshold, has been ended by a choice this document made and not by
a prohibition the vision states, and its verdict must say so in those words.** A
future candidate rejected by a preference that never got weighed, described as
though the vision had forbidden it, is the failure this paragraph exists to
prevent — and it is far harder to fix after the rejection than before it.

The weighed criteria come from §4.2's eight properties and §1's north star. They
are weighed because §4.2 says a strong mechanic "should generally be" these
things — the hedge is in the vision's own wording, and honouring it means
admitting that a mechanic can be excellent and imperfect. The arithmetic is in
§1.3.

Four questions are reserved for the operator's playtest, in §1.4, because
measurement genuinely cannot settle them. They are kept to four so that the
playtest has a short list rather than a survey.

### 1.1 Definitions the rest of Part 1 depends on

Three words are defined once here because the first draft used two of them in
two senses each, and under one of those readings a *gate* rejected every
candidate for a reason unrelated to what the gate measures.

---

**Outcome delta.** For one cell — a (context, entry speed) pair — and one choice
of invocation parameters, the **outcome delta** is

> the candidate run's horizontal speed at the horizon, **minus** the control
> run's horizontal speed at the same horizon, in ups.

The horizon is fixed at **1 second after the mechanic's availability window
closes**, the same for every mechanic so the numbers stay comparable. The
control is a run in the same context from the same entry speed that never
invoked the mechanic.

**"Outcome", unqualified, always means the outcome delta.** Where a criterion
needs the un-differenced speed it says **absolute exit speed** and means the
candidate run's horizontal speed at the horizon. Every ratio in this document
names its denominator; a ratio that does not is a defect in this document.

---

**Material.** A difference of at least **16 ups**.

That is 5% of the 320 ups ground cap, and it is the smallest speed change
legible on the overlay at a glance. `docs/movement-lab.md` §2's bunnyhop-window
table treats 1% / 5% / 10% of speed as the meaningful gradations, and this is
the middle one made absolute.

**Where a criterion states a percentage, the percentage and the 16 ups floor
both apply, and the larger governs.** This is not pedantry: several cells in the
sweep have a control speed near zero — the low-ceiling context in particular,
where the control may be stopped outright — and a percentage of a near-zero
quantity turns measurement noise into a finding.

---

**The naive neighbourhood.** Not a single perfectly-executed press. For each
cell, the naive neighbourhood is

> every invocation timing inside the availability window, crossed with every aim
> within ±30° of the player's current heading.

`docs/movement-lab.md` Limits §5 says the lab's numbers are ceilings because
"every technique here is held exactly. A player holds it imperfectly." W1 is the
one criterion that is *about* the imperfect player, so measuring a perfect one
would measure the wrong thing. **The naive outcome** is the mean outcome delta
over that neighbourhood. Counts must be published alongside any percentage: the
neighbourhood is finite and a percentage over 42 cells is false precision.

### 1.2 The sweep

W1, W2, W5 and W6 are scored from one measurement, so it is defined once.

**Contexts** — `straf3_collision::testbed`, the module itself and not the lab's
mirror of it (`docs/movement-lab.md` Limits §4):

| # | Context | Constructor | Kind |
|---|---|---|---|
| 1 | Flat ground | `floor()` | surface |
| 2 | Walkable ramp, 26° | `ramp()` | surface |
| 3 | Sliding ramp, 50° | `ramp()` | surface |
| 4 | Climbable riser, 18 units | `step()` | edge |
| 5 | Ledge and 256-unit drop | `ledge()`, `drop_from()` | edge |
| 6 | Inside corner, two walls | `corner()` | wall |
| 7 | Low ceiling | `ceiling_at()` | ceiling |

**Entry speeds:** 320, 400, 500, 640, 800, 1000 ups.

**Swept parameters, per cell:** invocation timing across the whole availability
window at 8 ms; wish direction relative to current velocity at 5°, refined per
G7's refinement rule where a step exceeds the materiality threshold.

**Also measured, on the same horizon and in the same cells:** the applicable
**canonical technique menu**, which W5 needs and which the candidate sweep alone
cannot produce. Each canonical technique is applicable only in some contexts,
and its domain is stated rather than assumed:

| Canonical technique | Applicable in contexts |
|---|---|
| `ground_turn` | 1, 2 |
| `air_forward`, `air_strafe` | all |
| `bunnyhop` | 1, 2 |
| drop launch | 5 |
| ramp traversal | 2, 3 |
| step-up | 4 |

**A context where the control cannot complete the traversal at all is a passage
result, not a speed result**, and is reported as such — "the control does not get
through; the candidate does" — rather than folded into a speed ratio where it
would appear as a near-infinite gain.

A candidate whose case needs geometry not on this list may have it built, but
the verdict must say so, and a mechanic that pays *only* in geometry built for
it is answered by W4.

### 1.3 The gates

Each gate names what decides it and what result fails it. Where an existing
piece of the repository already answers the question, it is named, because a
gate that needs new machinery to check is a gate that will be checked once.

---

**G1 — Deterministic.**

*Decides it:* `cargo xtask determinism` (`tools/det-runner`) across all four
build targets including wasm, plus
`crates/straf3-sim/src/state.rs`'s
`the_checksum_covers_the_state_a_technique_depends_on`.

*Fails if:* any target disagrees bit-for-bit, or the mechanic branches on state
that `SimState::checksum()` does not fold, or its outcome depends on anything
outside `(state, cmd, world, profile)` — wall-clock, `libm`, iteration order,
accumulated float state carried between commands by something other than
`PlayerState`.

*Authority:* §8's competitive integrity and §14's deterministic simulation
boundary, neither of which hedges. Not §4.2 — that list is the hedged one, and
citing it here would have undercut the gate with the document's own reasoning.
A run that cannot be re-simulated cannot be a record, so this is not a
movement-quality question at all: it is a question about whether the mode of
play exists.

---

**G2 — Inert when not invoked.**

*Decides it:* run the whole published measurement set — the 2137 named values in
`docs/movement-lab.md` and `tools/straf3-lab/measurements.pinned.tsv` — under
the candidate profile and diff it against the control profile.

*In scope:* **every measurement in which the mechanic's activation preconditions
are not all met on every command.** Only a measurement in which the mechanic
actually fires is exempt.

*Fails if:* any in-scope value moves. Not "moves a little": moves.

*Why scoped on preconditions and not on input:* G4 *requires* the mechanic to
overload an input canon already uses — crouch for the slide, jump for the dash
and the wall jump. So "measurements that do not involve the mechanic's own
input" is either near-total, which exempts every jump measurement from the
dash's G2 and guts the gate, or near-empty. Preconditions are checkable, they
are what the gate means, and they survive G4's overloading requirement.

*Authority:* §4.4's "makes the canonical movement language incoherent". A
mechanic that changes what a strafejump is worth when the player is not using it
has not been added to the language, it has replaced part of it, and every
existing measurement, every training map and every player's muscle memory is now
describing a game that no longer exists. This is also what keeps the evidence
readable: if enabling a mechanic moves unrelated numbers, no later comparison is
attributable to the mechanic.

---

**G3 — Immediate.**

*Decides it:* over presses **on which the mechanic's preconditions are met and
the mechanic fires** — two counts. First, the commands between the command
carrying the input and the first command on which velocity differs from a
control run that did not press it. Second, the commands on which the mechanic
causes any input to be ignored.

*Fails if:* either is not zero.

*Why restricted to presses that fire:* `step.rs` deliberately does **not** spend
a dash whose clamp yields nothing, so velocity never differs from the control on
those presses and the first count is undefined. A literal reading would fail a
mechanic at a gate for behaving exactly as designed.

*Authority, split:* the second count is squarely §4.4's "automate execution that
should belong to player skill" and needs no defence — an animation lock is the
game taking commands from the player and giving them back later. **The
zero-command threshold on the first count is canon's own choice.** §4.4's
responsiveness anti-goal is conditional on "for visual spectacle", so a wind-up
for gameplay reasons is not literally forbidden by the vision; §4.2's
"responsive" is hedged; §19's priority ordering explicitly says lower items are
not unimportant and is not an anti-goal. Canon chooses zero anyway, and a
candidate ended here must be told it was ended by that choice.

*What this does not forbid:* an availability window opened by an earlier event.
The player is not deprived of control while one runs. The forbidden thing is a
delay between deciding and moving.

---

**G4 — No new binding.**

*Decides it:* count the input bits and axes the mechanic reads that are not
already part of the movement input vocabulary (the two move axes, the view,
jump, crouch).

*Fails if:* the count is not zero.

*Authority: canon's own choice, and stated as one.* §4.2's opening — "the input
vocabulary should remain *relatively* compact" — is a hedge inside the hedged
list, and §21's "generic ability shooter with movement mechanics attached"
supports a constraint on ability-stacking without supporting a threshold of
exactly zero. The reasoning canon offers for choosing zero: depth is supposed to
come from timing, direction, geometry and sequencing, and a mechanic on its own
key has none of those to find — it is available whenever it is available and
costs nothing else to use. Overloading an existing input is what makes a
mechanic cost something, because the input is already doing a job.

`crates/straf3-sim/src/step.rs` already records that the replay codec would not
object to a new button bit, so this is a design decision kept as one rather than
a format limitation dressed up as a principle. A future mechanic that genuinely
needs a binding is not forbidden forever; it is forbidden until someone amends
this document ahead of measuring it.

---

**G5 — Earned, not refilled.**

Two halves. Both must pass.

***(a) Earned.*** *Decides it:* run a player who never exceeds `max_speed` —
accelerating on the ground only, jumping and landing freely, indefinitely — **on
flat open ground**, and count how many times the mechanic becomes available.
Publish the same count for all seven contexts of §1.2 as the evidence; **flat
ground is the cell that decides the gate.**

*Fails if:* the count on flat ground is not zero.

*Why this replaced the first draft's test.* The draft counted availability "for
a player who never performs the arming event", which is vacuous: the candidate
names its own arming event, so the answer is trivially zero for any mechanic
with any precondition at all. The gate could not fail anything. **The tree
already contains the right test**: `slide_entry_speed` is set to 400, above
`max_speed` 320, precisely so that "ground acceleration alone cannot reach it,
so a slide has to be entered out of a strafejump" (`profile.rs`). That is a
mechanic whose availability has to be *bought* with speed the player earned,
which is what distinguishes a technique from a rotation.

**Why *flat ground*, and this is a design ruling rather than a detail — it
decides one candidate on its own.** The replacement above substituted *speed*
for *earned*, and in doing so silently narrowed "earned" to one of its two
currencies. **Geometry is the other.** A mechanic conditioned on terrain the
player has to find, reach and be touching has been paid for, even if it was paid
for in route knowledge rather than in ups — and §7 says outright that mechanics
cannot be evaluated independently of the spaces they are used in. A cooldown's
defining property is not merely that it is cheap; it is that it is cheap
**everywhere**, on a schedule, with no condition the world can withhold.

Flat open ground is the context that imposes no geometric requirement at all. So
availability there, to a player who has also not earned it in speed, means the
mechanic was earned in neither currency — which is exactly what the gate is
asking. A mechanic armed by "touch the floor" fails, because the floor is
everywhere. A mechanic armed by "be against a wall" does not, because a wall is
somewhere.

*Disclosed with the same force as the dash disclosure below, because the honest
version of this ruling is that I chose between two readings that give opposite
verdicts.* `note_wall_contact` (`crates/straf3-sim/src/step.rs:1334`) gates wall
contact on `wall_contact_window_ms`, `wall_jump_velocity` and the plane's normal
— **and on nothing else. There is no speed precondition, so a player walking
into a wall at 100 ups arms a wall jump.** Under the flat-ground reading the
wall jump passes G5(a), because flat ground has no wall. Under a reading that
ran the same player across all seven contexts it would be **rejected at a gate,
with no weighing**, on `corner()`. Both readings were available, the document did
not say which, and this is knowable from `step.rs` before any candidate number
exists — so it is settled here rather than at verdict time, where a gate
consequence discovered after the numbers looks exactly like a gate written after
the numbers.

I have ruled for the flat-ground reading on the two-currencies argument above,
and against the seven-context reading because it would reject the wall jump for
being *geometric*, which is the thing that makes it a technique rather than a
rotation. **This is not a pass for the wall jump.** It still faces G5(b) — where
"a wall is available, therefore press it" is precisely the shape G5(b) tests —
and every weighed criterion. It is a ruling that the wall jump gets weighed
rather than ended.

*Disclosed here rather than in Part 2, so that nobody can say the gate was
written after seeing a number:* this test is known, from reading
`crates/straf3-sim/src/step.rs` alone and before any candidate measurement
exists, to bite one of the three candidates. The dash arms on any landing that
ended a jump, so a standing player can jump on the spot, land, and hold a dash
window at zero speed indefinitely — jump, land, dash, repeat, available to a
player who has never carried momentum, which is the rotation §4.4 names. That is
the gate working. Under §1.5's retune rule it is a pre-registrable retune of the
arming condition rather than an automatic rejection, and Part 2 will say which
happened.

***(b) Not already optimal.*** *Decides it:* the point-naive policy — invoke at
the first available command, aimed along the current heading — expressed as
`naive outcome delta / best outcome delta`, per cell, and taken as the **median
across contexts in which the best outcome delta is positive and material**.

*Fails if:* that median is ≥ 0.95.

*Why the positive-and-material restriction:* with both deltas negative the ratio
inverts. A naive −10 over a best −2 is 5.0, comfortably ≥ 0.95, so a mechanic
that only ever *harms* the player would fail G5(b) **as a cooldown** and be
ended at a gate with no weighing. It is not a cooldown; it is bad, and W1 should
reject it with its naive-harm number on the record where the next wave can read
it. Restricting the median to cells where there is a real benefit to be near-
optimal about is what makes the ratio mean what the gate says it means.

*Why on deltas and on a median:* on absolute speeds this is a test of entry
speed and nothing else — at 1000 ups entry, a naive 1020 against a best 1050 is
2.9% apart and the gate fails, while the same mechanic at 320 ups sits well
outside 5%. Since the sweep mandates entry speeds up to 1000 ups, that reading
would reject every candidate at a gate for a reason unrelated to cooldown-ness.
On deltas it is scale-free. The median rather than the worst cell, because a
single degenerate cell should not end a case with no weighing.

*Authority:* §4.4's "replace momentum mastery with cooldown rotations" and
§21's "a cooldown-rotation game". These are the two halves of what a cooldown
actually is: it arrives without being earned, and the correct play is to spend it
immediately. A window opened by a landing you had to reach *at speed* fails
neither half.

---

**G6 — No cap.**

*Decides it:* (a) read the implementation for any explicit clamp on the
magnitude of `velocity` or of its horizontal component; (b) compare every
terminal speed in `docs/movement-lab.md` §6 under the candidate profile against
the control, **in measurements where the mechanic is not invoked**.

*Fails if:* such a clamp exists, or an un-invoked terminal speed is lower.

*Why the "lower when used" clause was removed:* it converted a weighed question
into a gate. "Using this mechanic must never leave you slower than not using it,
in any context" is W1's naive-harm question and W6's decision-creating question —
and a mechanic that is *sometimes the wrong choice* is what W6 rewards. The
crouch slide was directly exposed: a slide is entered by crouching, and a
crouched player's wish speed is capped by the inherited, id-verified
`duck_scale` of 0.25, so a terminal speed measured mid-slide is lower for a
reason that has nothing to do with §4.4's arbitrary speed limits.

*What this gate is about:* the *ceiling*, not clamps in general.
`PM_Accelerate`'s clamp on the projection of velocity onto the wish direction is
the mechanism strafejumping is built out of and is emphatically permitted. The
forbidden shape is a limit on how fast the player may end up going.

---

**G7 — Attributable.**

*Decides it:* two parts, both measured.

**1. The rule predicts the outcome.** The verdict must state a rule — a formula
or a short algorithm — that computes the mechanic's effect from quantities the
player can perceive before invoking it: their speed, their direction relative to
the wish direction, whether they are grounded, what they are touching. The lab
measures the outcome across the sweep and publishes it beside the rule's
prediction, in the same measured-versus-closed-form shape
`docs/movement-lab.md` §1 and §4 already use. They must agree.

**2. No invisible cliff.** A **discontinuity** is a step in the outcome between
adjacent sweep points **that does not shrink when the grid is refined**.

> Halve the grid around every step that exceeds the materiality threshold. A
> continuous gradient's step halves with the grid; a genuine cliff does not.
> Report the largest step that survives refinement down to a stated floor,
> starting at 1° in aim, 1 ms in timing, 1/16 unit in geometry.

**The floor is a parameter of the rule, not a constant.** If a step is still
above threshold at the starting floor, refine further in that band and record
the floor that was needed; the finding is *whether it halves*, and a floor that
had to move is information rather than a failure. Canon's own vocabulary makes
this concrete: `docs/movement-lab.md` §1 shows vq3/forward at 500 ups entry
gaining 0.00 at 50° and 139.96 at 60°, because gain is pinned at zero until the
wish-speed clamp opens — so the steepest 1° step in that band may well exceed
16 ups. Theory says it should refine away, because that transition is a *kink*:
a discontinuity in the derivative, not in the value, and a kink's step halves
with the grid exactly as a smooth gradient's does. **If it does not halve, that
is a real finding about canon's own technique and belongs in this document**,
not a reason to weaken the test.

*Fails if:* no rule predicts the measurement, or a step that survives refinement
exceeds **16 ups** and does not coincide with a boundary the player can perceive
— a surface they are touching, a state the overlay shows, a threshold marked in
the world.

*Why refinement, and why this matters more than it looks.* "Largest single-step
change on a fixed grid" is a property of the grid, not of the mechanic. Applied
at §1.2's 5° aim resolution to the canonical technique, using canon's own
published numbers — `docs/movement-lab.md` §1, vq3/forward at 320 ups: 40° gains
97.71, 50° gains 177.82, best at 52° gains 197.45 — one 5° step is roughly
40 ups/s, about 20% of the best outcome, on no boundary in the world at all. The
un-refined test **rejects strafejumping**, the technique the game is named
after. It only falls under threshold near 1° spacing.

*The instrument must pass its own sanity check before it is used on a
candidate:* run it on strafejumping and it must report **no** surviving
discontinuity; run it on overbounce and it must report one, because a step from
160.00 ups returned to 0.17 ups returned across half a unit of drop height
survives any refinement whatever. A test that cannot tell those two apart is
measuring the grid. This is the same discipline
`crates/straf3-collision/tests/canon_frozen.rs` applies to itself when it
perturbs canon to prove the freeze bites.

*Authority:* §4.4's "make important outcomes opaque or impossible to
understand", and §20's Proof 2 — "players can learn primitives, combine them,
understand failures, and deliberately improve". Understanding a failure requires
that the failure had a reason visible at the time.

*The worked counterexample is already in the tree, and it is honest to say so.*
`docs/movement-lab.md` §4 measures overbounce: a 16.000-unit drop returns 100%
of the impact speed and a 16.500-unit drop returns 0.1%; 4.34% of the 8064 drops
sampled between 16 and 1024 units overbounce, scattered with nothing in the
world marking which, and the 1024.000-unit drop returns 100% too — so this is
not a low-drop artefact confined to a corner of the range. **Overbounce would
fail G7 if it were proposed today.** §1.6 states the principle that governs
that, what it does and does not license, and what it binds Part 2 to.

---

**G8 — Data, not a branch.**

*Decides it:* read `crates/straf3-sim/src/profile.rs` and
`crates/straf3-sim/src/step.rs`. The mechanic must be expressed as constants on
`PhysicsProfile` such that a stated value of those constants switches it off,
and `step.rs` must contain no test of *which profile* is in use.

*Fails if:* there is an `if canon { … }`, a profile-identity comparison, a
`bool` field that selects an algorithm, or a mechanic that cannot be switched
off by its own constants.

*Authority: the repository's own doctrine, not the vision's* —
`PhysicsProfile`'s doc comment ("a field here is a promise that the value is
genuinely a number the simulation reads, not a switch that selects a different
algorithm") and `canon_frozen.rs`, which already enforces the disabling half by
exhaustive destructure. It is a gate because it is the property that makes every
other gate checkable: a mechanic that is a branch cannot be A/B measured against
a control, cannot be recorded into a replay, and cannot be tuned without a
rebuild.

*One clarification, because the tree already contains the exception:* a
*threshold* constant does not have to be disabling. `wall_normal_max` and
`strafe_wish_speed_cap` are both read only when another constant is non-zero,
and zero is a meaningful value for each rather than an "off". A candidate may
have at most one such constant, and the verdict must name it and name the
constants that gate it.

### 1.4 The weighed criteria

Scored **pass / weak / fail** against stated thresholds, by the ordered tests
below — ordered so that no result falls in two bands and none falls in none.
Every threshold is a choice, not a derivation, and each says what it is
calibrated against.

---

**W1 — Learnable.** *(§4.2 "understandable at a basic level"; §1 "easy to
learn".)*

*Number:* the **naive-harm rate** — the fraction of the naive neighbourhood
(§1.1) whose outcome delta is negative by more than the materiality threshold.
Published as a count and a fraction.

1. \> 35% → **fail**
2. \> 20% → **weak**
3. otherwise → **pass**

*Calibration:* chosen, and stated as chosen. What the number has to be is well
below half, so that a beginner pressing the button at the obvious moment is
building a habit that helps them rather than one they will have to unlearn. 20%
is where "usually helps" stops being an honest description.

---

**W2 — Masterable.** *(§4.2 "difficult to perfect"; §1 "difficult to master".)*

*Numbers:* two, both published **per context** and never as a bare mean.

- The **naive-to-optimal gap**, per cell:
  `(best outcome delta − naive outcome delta) / best outcome delta`, with the
  naive outcome as defined in §1.1. Scored on the **median across the contexts
  in which the best outcome delta is positive and material** — the same
  restriction G5(b) carries, for the same reason.

  **The raw `best` and `naive` deltas in ups must be published beside every
  ratio**, and a cell that does not qualify is marked as not meaningful rather
  than printed as a number. A ratio whose denominator is two ups is not a
  measurement of anything.

  *Why this restriction, stated because it was missed once.* `best − naive` is
  never negative, since `best` is a maximum over the same set. So the sign of
  the gap is the sign of the denominator: in a context where the mechanic only
  ever harms, the gap is negative, and in one where it barely helps, the gap is
  an arbitrarily large number divided by an arbitrarily small one. Neither is a
  statement about how much there is to master — the first is a statement that
  the mechanic is bad, which is **W1's** job to report with its naive-harm
  number on the record, and the second is noise. This is the identical
  pathology amendment 2 fixed in G5(b), and W2 has the identical denominator; it
  should have been fixed in the same edit and was not.

  **The consequence for thin candidates, which is a real tightening and is
  disclosed rather than left to be discovered.** The qualifying test here is the
  same one W4 counts, so a mechanic material in three or more contexts has three
  or more cells in this median, and one material in two has two. A median over
  one or two cells reintroduces exactly what the median was added to prevent —
  a single cell carrying a *required* criterion. **So: where fewer than three
  contexts qualify, W2 cannot score better than *weak*.** Since admission
  requires W2 at pass, that makes "material in at least three contexts"
  effectively necessary for admission, which is what W4 already says in its own
  voice. A candidate ended this way must be told that W2 and W4 agreed rather
  than that two independent criteria condemned it.
- The **execution window**: the width, in milliseconds, of the set of invocation
  timings yielding ≥ 95% of the best outcome delta.

*Gap:* < 10% → fail; < 20% → weak; otherwise pass.
*Window:* ≥ 90% of the availability window → fail; ≤ 384 ms → pass; otherwise
weak.
*W2 is the worse of the two.*

*Why the median and not the mean.* The contexts are not commensurable for this
purpose. In the low-ceiling context the control may be stopped outright, so the
gap ratio there approaches 100%, and a single such context lifts a seven-context
mean by about 14 points on its own — enough for a candidate measuring 12% in six
contexts to reach a 24.6% mean and **pass a required criterion on one cell**.
The median cannot be carried by one context, and publishing all seven means the
spread is visible rather than averaged away.

*Calibration of the gap, from published canon numbers.* At 320 ups entry a VQ3
player holding 30° gains 49.51 ups/s and one holding the optimal 52° gains
197.45 (`docs/movement-lab.md` §1) — a 74.9% gap; at 20° it is 89.6%. A
candidate scraping 20% is already three to four times shallower than the
shallowest thing the game currently teaches, so a failure at 10% is not a close
call.

*Calibration of the window — and a disclosure about it.* 384 ms is **canon's own
chosen number**, deliberately decoupled from any constant Part 3 might change.
It coincides with today's measured usable double-jump delay, and the first draft
of this document cited that as its justification; that citation was wrong twice
over. It pinned a threshold to `double_jump_window_ms`, a `TODO(wave2)` constant
Part 3 must argue and may move — which would have forced every verdict scored
under 384 ms to be restated. And it was the wrong *kind* of quantity: the double
jump's 384 ms is an **availability** window in which every jump receives the
full boost ungraded by timing, so the double jump's own execution window equals
its availability window, which is W2's **fail** condition. The pass threshold was
anchored to an incumbent that scores at the fail edge.

The anchor of the right kind is elsewhere in the same section: the **bunnyhop
window**, where a landing player keeps ≥ 95% of their speed only by spending
**16 ms or less** on the ground. That is a graded execution window measured at
exactly W2's 95% level, and it is the tightest thing in the game. **384 ms is
therefore set twenty-four times looser than canon's only same-kind
measurement**, on purpose: the window half of W2 is a backstop against a
mechanic with no timing to hit at all, not a discriminator. The gap is the half
that discriminates, and it should be read as the one doing the work.

---

**W3 — Composable.** *(§4.2 "composable with other mechanics"; §4.2's thesis
that "advanced movement should come from combining understandable primitives".)*

*Numbers:*

1. **Chain gain**, per context: the best outcome delta when the mechanic is used
   *in addition to* the applicable canonical technique, against the best when it
   is used *instead of* it.
2. **Entry-speed sensitivity**: `d(absolute exit speed)/d(entry speed)` across
   the swept range — stated on the absolute quantity, by name, because on deltas
   it means something else entirely.
3. **Levelling**: whether the mechanic ever sets absolute exit speed to a value
   independent of the entry speed.

1. If (3) holds anywhere → **fail**.
2. Else if there is no context where using both beats using either alone by the
   materiality threshold → **fail**.
3. Else if (2) is ≤ 0 anywhere in the swept range → **weak**.
4. Else → **pass**.

*Why:* a mechanic that substitutes for strafejumping rather than chaining with
it does not add to the language, it replaces part of it. And a mechanic that
hands every player the same exit speed regardless of what they carried in has
erased the thing the previous ten seconds of play were about — a speed cap
wearing a mechanic's clothes, scored here rather than at G6 because it caps a
*technique* rather than the player.

---

**W4 — Useful in more than one situation.** *(§4.2 "useful in multiple
situations".)*

*Numbers:* the count of the seven contexts in which the mechanic's best outcome
delta is material, and the count of distinct *kinds* (surface / edge / wall /
ceiling) among them.

1. ≥ 3 contexts spanning ≥ 2 kinds → **pass**
2. ≥ 2 contexts → **weak**
3. otherwise (0 or 1 context) → **fail**

*Calibration:* materiality is defined in §1.1 and is the same everywhere in this
document. The "3 contexts, 2 kinds" shape exists because three ramp angles are
one situation measured three times, not three situations.

*Why:* a mechanic that pays in exactly one place needs maps built around it, and
"maps built to accommodate a mechanic" inverts §7's point that mechanics cannot
be evaluated independently of the spaces they are used in — it makes the space a
jig.

---

**W5 — Vocabulary-conserving.** *(§4.4 "make the canonical movement language
incoherent"; §3's balance of execution and discovery.)*

*Number:* for each canonical technique, within **its own domain** as tabulated
in §1.2, compare its best outcome against the candidate's best outcome in the
same cell. A technique **survives** if there is at least one cell in its domain
where the candidate does not beat it materially.

1. Two or more techniques dominated everywhere in their domain → **fail**
2. One → **weak**
3. None → **pass**

*Why it is stated this way.* The first draft asked whether each technique was
"still the best option available", scored from the candidate sweep — which
cannot produce it. That sweep varies the candidate's timing and aim against one
control and never scores the canonical techniques against each other, and "best
available" is undefined where the techniques are not alternatives: step-up is
not a choice on flat ground, the drop launch requires a drop. The domain table
in §1.2 and the technique menu measured alongside the sweep are what make this
producible.

*Why the criterion exists:* this is the arithmetic behind "the movement language
got bigger". A mechanic that makes two existing techniques pointless is a net
loss of one, however good the new one is. It is deliberately scored on
*dominated everywhere*, not on *weakened*: a new mechanic taking some of an old
one's territory is what composability looks like.

---

**W6 — Decision-creating.** *(§4.4 "introduce complexity without increasing
meaningful depth" and "reduce movement to memorizing ability sequences"; §21's
"excessive complexity substitutes for depth".)*

*Number:* for each context, the **≥ 95%-of-best set** of invocation parameters —
the same set W2 already computes — in timing and in aim. Compare across
contexts: the sets in at least two contexts must be **disjoint**, or their
centroids must differ by more than 10% of the swept range.

1. Both timing and aim satisfy it → **pass**
2. One does → **weak**
3. Neither → **fail**

*Why the set and not the argmax.* On a plateaued outcome surface the point
argmax is arbitrary and can jump the full width of the plateau for numerical
reasons — and a plateaued surface is precisely W2's failure mode. Scored on the
argmax, a mechanic that is *bad* by W2, with a wide execution window and nothing
to hit, would tend to show large spread and score **pass** on W6. The 95% set
costs nothing extra to measure and does not invert.

*Why the criterion exists:* a mechanic whose best use is identical in every
situation is not a decision, it is a step in a routine — §4.4's "reduce movement
to memorizing ability sequences". Depth is the player having to work out which
use is right *here*, and that only exists if the answer differs.

---

**W7 — Cheap.** *(§4.4 "introduce complexity without increasing meaningful
depth", cost side.)*

*Numbers:* three counts the verdict must publish — new `PhysicsProfile`
constants, new `PlayerState` fields, and new activation preconditions (distinct
state predicates gating the mechanic in `step.rs`) — and the one-sentence
statement of the rule G7 already requires.

1. ≥ 5 preconditions, or the honest statement needs more than one sentence →
   **fail**
2. ≤ 3 preconditions → **pass**
3. otherwise (4 preconditions) → **weak**

*Calibration:* three, because that is what the existing vocabulary costs. A
double jump is "land from a jump, jump again soon" — two. A strafejump is "be in
the air, hold a direction off your velocity" — two. A candidate needing five
conditions is not being learned by anyone from playing.

*Note:* the counts of constants and state fields are **published, not gated**.
They are information the operator and the next wave need; a mechanic with four
constants that is genuinely excellent should not be rejected by an accountant.

### 1.5 The arithmetic

Stated so a verdict can be argued with rather than asserted.

1. **Any gate fails → rejected.** The weighing does not happen. The verdict
   records which gate, the number, what would change it, and — for G4 or G3's
   threshold — that it was canon's choice rather than the vision's prohibition.
2. **All gates pass →** score W1–W7.
3. **Admitted** requires **W1, W2 and W3 all at pass**, and among W4–W7 **no
   fail and at most one weak**.
4. **Anything else → rejected for this wave**, naming the criterion, the number,
   and what would change the answer.

*Why W1–W3 are required rather than counted:* W1 and W2 are §1's north star
stated as two numbers — easy to learn is a low naive-harm rate, difficult to
master is a large naive-to-optimal gap — and a mechanic failing either fails the
sentence the whole game is built on. W3 is §4.2's thesis that depth comes from
combining primitives; a mechanic that does not combine is a primitive standing
alone, which is an ability, which is §21's first confirmed anti-goal.

---

**A third verdict: *unjudgeable on available evidence.***

Neither admitted nor rejected, and it exists because the alternative is worse. A
verdict that looks settled while meaning "we had nowhere to test it" is the
outcome this whole document is written to avoid, and forcing such a case into a
rejection would let a future wave read a clean-looking "no" where the honest
record is "not asked properly".

A candidate is **unjudgeable** when the evidence needed to score a required
criterion does not exist and cannot be produced this wave. The verdict must
name: which criterion, what evidence is missing, what would produce it, and
what the mechanic scored on everything that *could* be measured. An unjudgeable
candidate's constants stay at their disabling values in `straf3()` — the same
place a rejection puts them — but the record is different and the next wave is
told so.

**Geometry dependency is named, not implied.** A mechanic can be measurable in
`testbed` and unusable in every map that ships. The only map in the tree,
`assets/maps/coil.map`, has **no ceilings at all** — so a crouch slide, whose
stated value in `profile.rs` is that "carrying speed under a low ceiling is
worth doing", reduces on `coil` to a temporary friction change on open ground.
Its walls exist but its one dramatic non-walkable surface has a normal whose z
is 0.5547, inside the dead band between `wall_normal_max` and `min_walk_normal`
that `profile.rs` describes: too steep to walk, not steep enough to push off.
Every verdict must therefore state whether the mechanic's primary use has
geometry in a shipped map, and an admission that depends on geometry not yet
built is an admission **with a stated geometry dependency**, said in those
words. This is a different fact from anything the sweep measures, and it bears
on the playtest (§1.9) rather than on the score.

---

**The retune rule.** A candidate's constants are opening positions chosen to put
the mechanic in a measurable regime, not tuned values —
`PhysicsProfile::experimental()`'s own doc comment says so. One retune is
permitted per candidate, under a pre-registration rule identical in spirit to
this document's own: the verdict must name the constant, state the direction of
the change and predict which criterion it will move, **before** the
re-measurement is run. A retune discovered after the fact, or a second retune, is
not evidence — it is a search for a number that passes, and `experimental()`'s
doc comment already answers that: "a mechanic whose case depends on finding
exactly the right constant has already failed *simple to invoke*."

**Admitting more than one candidate requires a joint pass.** §1.2 measures one
candidate at a time against one control, and §1.6's one-variable rule requires
it. Nothing in that measures a *pair* — and the tree already contains an
interaction: `check_air_jump` handles the wall jump before the dash and returns,
so a press that could do either does the wall jump and the dash is not spent,
and `dash_window_ms` is set to the double-jump window deliberately so the two
"compete for the same input". So: if more than one candidate is admitted,
`PhysicsProfile::straf3()` must pass **G2 and G7 again under the joint profile**
before the freeze, and each verdict must state the precedence rule between the
admitted mechanics. Otherwise canon ships a combination no measurement covered.

**A weighed fail can be overturned only by the playtest**, never by another
measurement. If the operator plays a mechanic that measured badly and reports
that it is nonetheless movement worth mastering, that is evidence of the kind
§1.9 exists to collect and it outranks a threshold I chose. The reverse is also
true and matters more: a mechanic that passes everything here and plays badly is
rejected. §4.3's standard is "testing demonstrates that it improves Straf3", and
the operator's hands are part of the testing.

### 1.6 Inherited behaviour: what the criteria do and do not reach

G7 catches overbounce, which is in canon. That is deliberate — a gate shaped to
spare an incumbent is not a gate — and it needs a stated principle rather than a
convenient silence, because the exemption has now been granted twice and two
exemptions is a principle whether or not anyone writes it down.

**The principle: G7 governs admission and does not by itself govern removal.**
These are different decisions with different costs. Adding a new unpredictable
discontinuity imposes it on every future player and every future map. Removing
an existing one is a different argument, and this wave declines to open it.

**The ground is scope, and only scope.** Judging the inherited base means
re-deriving the entire Q3 vocabulary — the drop launch included, which
`docs/movement-lab.md` §6 calls "the largest number in the movement language" —
which is a different and much larger piece of work than judging three additions.
That is true today and it is the whole of the argument.

*The argument this document explicitly does not make*, recorded because it is
the one a reader will think of and it must not be mistaken for a load-bearing
premise here: that players have already learned overbounce and that removing it
would invalidate their investment. That is false of Straf3, and this repository
says so. `PLAYING.md`'s own "Not proven yet" section records that the
personal-best and ghost loop has been closed exactly once, that the `.s3d` was
deliberately not committed, and that no records service exists. There is no
player population and no route corpus. The removal cost that argument invokes is
Quake's, borrowed — and a document whose entire integrity claim is that it was
written before the numbers cannot afford a load-bearing premise its own
`PLAYING.md` falsifies.

**The exemption is not vision-neutral, and saying so is the point.**
`docs/VISION.md:105` lists among the foundations "overbounces and other valuable
emergent interactions **where they create understandable depth**". The
endorsement is conditional, and the condition is very close to what G7
formalises — so `docs/movement-lab.md` §4's measurement is evidence the
condition is *not met*. Keeping unjudged overbounce is therefore arguably against
§4.1 rather than blessed by it. This section is a scope decision, and it should
read as one.

**An exemption is available only to a behaviour whose provenance is citable.** A
behaviour whose provenance cannot be established has not been inherited; it is
*unjudged*, and it goes through Part 1 like any candidate, or its constants are
chosen deliberately under §1.8. Without this sentence an unsourced behaviour
this tree invented could shelter under the same words as one that genuinely
arrived with Quake.

*Worked application, because this is where it bites.* The double jump's
`double_jump_window_ms` 400 and `double_jump_boost` 100 carry no id-source
citation and cannot be verified against CPMA, whose source is not public. The
*behaviour* is citable at reconstruction grade and so reaches the exemption. The
two *magnitudes* do not escape §1.8 — and **they do not stand or fall together,
which four rounds of this argument got wrong before anyone opened the files.**

- **`double_jump_boost` 100.** GPP-1-1's `bg_promode.c` assigns
  `cpm_pm_jump_z = 100/*/230*/; // enable double-jump //100` inside the pro-mode
  branch of `CPM_UpdateSettings`. That is a port's own CPM configuration, not
  merely a comment about another game. Two caveats that must travel with it: the
  same file *declares* the same variable at file scope as `0.5`, with a comment
  reading "CPM: 100/270 (normal jumpvel is 270, doublejump default 100) =
  0.37037" — a declaration that disagrees with its own arithmetic and uses a
  different unit from the assignment; and the file is a Tremulous gameplay
  patch, so it is third-hand about CPMA.
- **`double_jump_window_ms` 400.** **Not corroborated there at all** — there is no
  millisecond window anywhere in that file's 372 lines — and its one attestation
  is much weaker than it looked. freepromode's README documents the cvar
  `g_doublejump` as "Give a boost if a jump is done within 400 ms of the last
  one", which is the number, verbatim. But the README also says, in the same
  file: that its author "used some code … `cpm1_dev_docs.zip`" and **"tried to
  purge it", which "has resulted in a *less accurate* imitation of CPM
  physics"**; that "I know that CPM is incorrect"; and that a better
  reconstruction exists elsewhere which the author recommends over their own.
  Xonotic's reconstruction sets `sv_doublejump 0`.

  So the 400 is attested by a source that (a) traces to the same single upstream
  document every other port traces to, and is therefore **not independent
  evidence**, and (b) describes itself as a degraded copy of that upstream. It
  is the weakest-supported constant in this tree, and **Part 3 should treat it
  as a value Straf3 must choose deliberately rather than one it can cite.**

*The structure is better attested than either number, and that is worth more to
Part 3.* The same file's vq3 branch reads `cpm_pm_jump_z = 0; // turn off
double-jump in vq3` — an independent port spelling "VQ3 is CPM with the
extensions switched off" as a zero on the very field, which is the relationship
`profile.rs` already encodes and defends.

*(Chain of custody, because a document that demands citations owes one for its
own claims. All three sources — GPP-1-1's `bg_promode.c`, Xonotic's
`physicsCPMA.cfg` and freepromode's `README.md` — were fetched raw and read in
full by the author of this document: `sha256 31bea076…`, `914892ff…` and
`087d9a43…`. Every one of them had previously been read through a summarising
fetch, and **two of the three were misread that way**. `bg_promode.c` declares
Tremulous-tuned values at file scope and overwrites them at runtime, so a
summary surfaces the declarations and misses the function; and freepromode's
README was cited for the 400 ms without the disclaimers three lines below it
that are the most important thing in the file. Neither error was visible in the
summary; both were obvious within a minute of `curl`.)*

**Part 3 must not argue that these numbers were never inherited** — that claim
is contradicted by the files. It must argue 400 and 100 separately, at the
grades above.

**What actually forces this exemption is a client limitation, and marking
dissolves it.** G7 part 2 does not require the *absence* of a discontinuity. It
requires the discontinuity to coincide with a boundary the player can perceive,
and it names "a state the overlay shows" as one such boundary. Overbounce fails
G7 on exactly one clause — "nothing in the world marking which" — which is a
gap in the client and in world authoring, not a property of the movement model.
A gate a behaviour fails for want of a *marker* is not telling you the behaviour
is unpredictable; it is telling you the game does not show its own state.

**So: mark it and overbounce passes G7, and this entire section becomes
unnecessary.** That is a better outcome than the exemption and it is cheap.
Building it is client work and outside this wave, so it is recorded rather than
done — but recording it converts a permanent-looking exception into a known fix
with a price attached. Two caveats, both real and both questions rather than
obstacles: `docs/movement-lab.md` §4 says eligibility turns on "which fraction of
a command the hull is inside when it meets the floor", which is a phase
relationship rather than a property of drop height, so a static world marking may
not be able to express it and only a live predicted-landing readout could; and §4
also says this is "the number most likely to move" under sub-stepping, so
markability cannot be settled until the re-measurement lands.

**The expiry, as a condition on the exemption itself.**

> *This exemption is grounded in scope, and it expires when removal stops being
> free: the inherited base must be judged before the first ranked record is
> accepted under canon.*

It is written as a condition the exemption carries, not as a promise about a
future wave — this document cannot bind one. Without it, "grandfathering by
silence" is what the exemption becomes, because the moment competitive records
exist the removal cost is real and the exemption sustains itself.

**What does and does not trip it.** The trigger is a record *accepted as
ranked* — which requires something that accepts it: a records service, a
leaderboard, a result that stands against other players' results. A run, a
personal best and a ghost captured under canon and committed to this repository
are **local artefacts and do not trip it**, however competitive the play that
produced them. They mark the moment the question becomes *live*, not the moment
the deadline fires. The distinction is load-bearing in both directions: worded
more loosely the exemption never expires, and worded more tightly it fires on an
artefact that carries no competitive weight and no player's standing depends on.

**Two things this section binds Part 2 to.**

1. A rejection under G7 must say that **Straf3 declines to *add* a new
   unpredictable discontinuity** — not that unpredictable discontinuities are
   foreign to the movement language. The second claim is false and overbounce
   proves it false; the first is true and defensible.
2. A rejection under G7 must publish the candidate's largest surviving
   discontinuity **beside overbounce's**: 160.00 ups returned against 0.17,
   across half a unit of drop height. If the candidate's cliff is smaller than
   the incumbent's and it is rejected anyway, that must be visible in the
   verdict with the admission-not-removal principle named as the reason. A
   rejection that hides the comparison is the one that will not survive being
   argued with.

### 1.7 Amendment record

Part 1 has been amended three times, each after independent review by
`movelead` and each still before any candidate number existed. No verdict
had been scored under any superseded text, so nothing required restating under
the threshold-edit rule.

**Amendment 3** — one change, ruled minutes before `lab` published and therefore
still under the pre-publication immunity:

| # | Change | Effect |
|---|---|---|
| **23** | **W2's gap takes G5(b)'s restriction**: median over contexts where the best delta is **positive and material**, raw deltas in ups published beside every ratio, non-qualifying cells marked rather than printed. And **where fewer than three contexts qualify, W2 cannot score better than weak** | `best − naive` is never negative, so the gap's sign is the denominator's: a context where the mechanic only harms yields a negative gap, and one where it barely helps yields noise. Amendment 2 fixed this in G5(b) and missed the identical denominator in W2 — which matters more here, because W2 is *required* and has been carried almost entirely by its gap half since amendment 1 re-anchored the window. The three-context floor exists because a median over one or two cells is the single-cell pathology the median was introduced to prevent |

**Amendment 2** — five changes, one of them a gate ruling that decides a
candidate:

| # | Change | Effect |
|---|---|---|
| **18** | **G5(a) now names its geometry: flat open ground decides the gate**, with all seven contexts published as evidence | The test did not say where its player ran, and the two readings gave the wall jump **opposite verdicts** — pass on flat ground, rejected at a gate on `corner()`. Ruled for flat ground on the two-currencies argument: geometry is the second currency in which availability can be earned, and a cooldown is cheap *everywhere*. The wall-jump consequence is disclosed in G5(a) itself, at the same strength as the dash's |
| **19** | G5(b) restricted to contexts where the best delta is **positive and material** | With both deltas negative the ratio inverts: naive −10 over best −2 is 5.0, so a mechanic that only ever harms would fail G5(b) *as a cooldown* and be ended at a gate. It is not a cooldown, it is bad, and W1 must reject it with its naive-harm number on the record |
| **20** | G7's refinement floor restated as a **parameter of the rule**, with the kink argument written out | Lab §1's own vq3/forward curve at 500 ups has a band where gain is pinned at zero until the wish-speed clamp opens; the steepest 1° step there may exceed 16 ups and trip G7's self-test, blocking every G7 verdict. The floor may move; whether the step *halves* is the finding |
| **21** | §1.6's double-jump citation rebuilt on bytes: "mutually consistent" removed, 400 and 100 separated, chain of custody updated | The two sources attest **disjoint** facts — one each, not two agreeing. Both had been misread through summarising fetches; `bg_promode.c` declares Tremulous values at file scope and overwrites them at runtime, so a summary surfaces the declarations and misses the function |
| **22** | The structural corroboration promoted over the magnitudes in §1.6 | GPP-1-1's vq3 branch spells "VQ3 is CPM with the extensions switched off" as a zero on the very field `profile.rs` uses. That is worth more to Part 3 than either number |

**Amendment 1** — seventeen changes. The six marked **blocking** would have
invalidated a measurement had they landed after `lab` published.

| # | Change | Effect |
|---|---|---|
| **1** | **Blocking.** "Outcome" defined once as the delta against control, in ups (§1.1); W3 and G7 restated on **absolute exit speed** by name | G5(b) on absolute speeds was a test of entry speed: at 1000 ups, naive 1020 vs best 1050 is 2.9% and the *gate* failed. Every candidate would have been rejected at a gate for a reason unrelated to cooldown-ness |
| **2** | **Blocking.** G7's discontinuity redefined as a step that survives grid refinement, with a stated refinement floor and a self-test on strafejumping and overbounce | The un-refined test measured the grid and would have **rejected strafejumping**: one 5° step of lab §1's own curve is ≈20% of the best outcome, four times the old threshold, on no boundary at all |
| **3** | **Blocking.** W2's 384 ms decoupled from `double_jump_window_ms` and restated as canon's own number, anchored instead to the bunnyhop window's 16 ms | The threshold was pinned to a `TODO(wave2)` constant Part 3 may move, and was the wrong *kind* of quantity — an ungraded availability window, from an incumbent that scores at W2's own fail edge |
| **4** | **Blocking.** W5 restated on a per-context technique domain table, measured alongside the sweep | W5 was not producible from the sweep §1.2 said produced it, and "best option available" was undefined where techniques are not alternatives |
| **5** | **Blocking.** G2's exemption re-phrased on **activation preconditions** rather than on the mechanic's input | G4 *requires* input overloading, so the input-phrased exemption was either near-total (gutting G2 for the dash) or near-empty |
| **6** | **Blocking.** G5 split: (a) the vacuous "never performs the arming event" test replaced by *available to a player who has never exceeded `max_speed`*; (b) restated on deltas and on a median | (a) could not fail anything, and specifically could not catch a dash armed by any jump-landing — jump on the spot, land, dash, repeat, at zero speed |
| 7 | G6's "lower when used" clause removed | It converted W1's and W6's weighed questions into a gate, and exposed the crouch slide through inherited `duck_scale` |
| 8 | §1.0's blanket "gates are not trade-offs" claim replaced by a per-gate authority table | False for G4, overstated for G3's threshold, and G1 cited the hedged §4.2 rather than the unhedged §8/§14 |
| 9 | Band holes and overlaps closed across W1–W4 and W7 by ordering the tests; W2's unreachable fail clause replaced by "≥ 90% of the availability window" | 20%, 35%, 384 ms each sat in two bands; a 600 ms–availability window and a 3% chain gain sat in none; the old fail clause could never trigger, and disagreed with the contract `lab` builds against |
| 10 | W2's gap scored on the **median** across contexts, published per context | A single near-zero-control context lifts a seven-context mean by ~14 points — enough to pass a *required* criterion on one cell |
| 11 | W6 scored on the ≥95%-of-best **set** rather than the point argmax | The argmax is unstable on a plateau, which is W2's failure mode — a mechanic bad by W2 would have scored pass on W6 |
| 12 | W1's naive policy widened from one exact press to a **neighbourhood**; counts published | Lab Limits §5: the lab measures perfect execution. W1 is the one criterion about the imperfect player |
| 13 | G3 restricted to presses on which the mechanic fires | A dash whose clamp yields nothing is deliberately not spent, so the old count was undefined and a literal reading failed it at a gate for working as designed |
| 14 | "Material" defined once as **16 ups**, applied as a floor under every percentage | Near-zero-control cells turned noise into findings; G7's "5% of the outcome" was a fraction of a near-zero delta |
| 15 | §1.5 gained the **joint-profile** clause for multiple admissions | `check_air_jump` resolves wall jump before dash; admitting both would ship a combination no measurement covered |
| 16 | §1.5 gained a third verdict, **unjudgeable on available evidence**, and a geometry-dependency disclosure | `coil` has no ceilings and its walls sit in the dead band, so a mechanic can be measurable in `testbed` and unusable in every shipped map |
| 17 | §1.6 rewritten: admission-not-removal principle; **scope as its only ground**, with the sunk-skill argument recorded as one this document explicitly does not make; §4.1:105's conditional endorsement cited; a citable-provenance requirement; **marking recorded as what dissolves the exemption**; **an expiry written as a condition the exemption carries**, with its trigger scoped to ranked records and explicitly not tripped by a local personal best or ghost; and two bindings on Part 2 | The exemption had been granted twice without being stated; the sunk-skill premise is falsified by `PLAYING.md`; an uncitable behaviour could have sheltered under "inherited"; and an exemption with no expiry sustains itself the moment records exist |

### 1.8 What this document is not deciding

Named, because an open question left visible is worth more than one silently
closed.

1. **Whether canon's inherited behaviours would pass these criteria.** §1.6
   states the principle, its ground, its limits and its recommended expiry.
   Overbounce fails G7 as written; the drop launch — the largest speed gain in
   the game, `docs/movement-lab.md` §4 and §6 — is an accident of `PM_WalkMove`'s
   length-preserving rescale rather than a designed mechanic. This document
   judges *candidates*, and deliberately does not grandfather anything by
   writing the criteria loosely enough to admit it.
2. **The tuned values of an admitted mechanic's constants**, beyond the single
   pre-registered retune of §1.5. Tuning is a different activity from judging and
   needs the operator's hands, not a sweep.
3. **Simulation frequency.** §22 leaves it open; every number in this document
   and in the lab is at 125 Hz, and the rate is part of the physics.
4. **Whether `coil` — or any map — has enough distinct routes to judge a mechanic
   against.** `docs/movement-lab.md` Limits §6 says plainly that the only
   perturbation harness that exists cannot answer this honestly. W4 and W6 are
   scored on testbed geometry; route-level evidence is P3's and P4's job, and
   §1.5's geometry-dependency disclosure is what keeps the gap visible in a
   verdict.
5. **What makes a *map*, a *profile*, or a *constant* canonical.** These are
   criteria for a mechanic. Inherited constants are governed by a different and
   simpler rule, which Part 3 applies: every constant canon carries must either
   carry a citation to a verifiable source, at the grade that source actually
   has, or be a value Straf3 chose deliberately with the reason recorded. "It
   was in CPM" is not a reason — §4.1 says the objective is not preservation.
6. **Whether a rejected mechanic is rejected forever.** It is not. A rejection
   must name the criterion, the number and what would change the answer,
   precisely so the next wave can satisfy it or argue with it rather than
   re-proposing the mechanic from scratch.
7. **Whether these criteria are right.** They are version 1.1 of an answer to a
   question §22 deliberately left open, and the honest status of such an answer
   is "the best available, written down where it can be attacked". §1.7 is the
   evidence that attacking it works.

### 1.9 What measurement cannot settle

Four questions. They are reserved rather than proxied because a proxy metric
that does not mean what it claims is worse than an honest gap — it converts a
question into a number, and then the number gets cited.

| | Question | Why measurement cannot settle it | Where it is asked |
|---|---|---|---|
| **P1** | Did *you* execute it, or did the game do it for you? | G3 measures that no command is taken from the player. It cannot measure whether the outcome felt authored. A mechanic can be fully player-controlled and still feel like it happened to you. | PLAYTEST.md §3 q13, q15 |
| **P2** | When it did not fire, could you tell why from the screen? | G7 measures that a *rule* exists and that surviving discontinuities sit on perceptible boundaries. Whether the boundary is perceptible **as rendered, at speed, on the day** is a fact about the client and the player, not about the simulation. | PLAYTEST.md §3 q16 |
| **P3** | Does it belong to the same game as strafejumping, or is it bolted on? | §4.4's coherence anti-goal. G2 measures that nothing else moved and W5 that nothing else died, and a mechanic can pass both and still feel imported from a different game. There is no measurement of idiom. | PLAYTEST.md §3 q14 |
| **P4** | Keep, cut, or revise? | §4.3's actual standard. Everything above is instrumentation for this question. | PLAYTEST.md §3 q17 |

PLAYTEST.md §3 already asks all four; its appendix maps every question to the
§4.2 property it tests. Nothing new needs inventing, and the playtest checklist
is not lengthened by this document — it is pointed at.

**A caution the geometry finding forces.** P1–P4 can only be answered about a
mechanic the operator can actually use in a map that exists. Where §1.5's
geometry disclosure says a mechanic's primary use has no geometry on `coil`, the
playtest cannot answer these four for it, and a verdict must not read as though
it had.

**What this means if the playtest does not happen.** The recommended profile
stands as *provisional* canon, recorded as provisional. A verdict resting only
on Part 1's measurements has answered every question except the four above, and
saying so is better than promoting a measured result to a decided one.

### 1.10 Conditions the evidence must meet

Four, so that a verdict cannot be built on measurements that were never
comparable.

1. **Measured under the integration canon will ship with.** The lab's published
   numbers were taken under single-step integration, and its own Limits §1 says
   so: §4's overbounce counts in particular are a per-command artefact that
   sub-stepping moves. A candidate judged on numbers taken under a
   known-superseded integration has been judged on a model the game will not
   run. Candidate measurement therefore follows sub-stepping, not the reverse.
2. **Measured against the real fixtures.** `docs/movement-lab.md` Limits §4
   records that the lab's §3 and §5 numbers were taken against a *mirror* of
   `straf3_collision::testbed` rather than the module itself, and "must be
   re-taken against the real module and compared before they are trusted". W4's
   context list touches the same geometry, so candidate measurement uses the
   module.
3. **One variable.** The candidate profile must differ from its control by the
   candidate's constants and nothing else, per `experimental()`'s doc comment —
   with §1.5's joint-profile clause covering the case where more than one is
   admitted.
4. **Against a stated control.** Every outcome in §1.2 is a difference from a
   control run in the same context at the same entry speed. An absolute number
   with no control is not evidence about a mechanic; it is evidence about a
   world.

---

## Part 2 — The verdicts

**No verdict is written.** The candidate sweep has not published, so no weighed
criterion has been scored, and a verdict requires them.

That restraint is deliberate and worth saying plainly: a document whose whole
discipline is that criteria precede numbers would be worth nothing if it ended by
producing verdicts in a hurry to look finished. **Part 1's pre-publication
immunity is intact** — every threshold was fixed before any candidate measurement
existed, and **no threshold has been edited this wave.**

Part 2 will record, for each of crouch slide, dash and wall jump: the gates, the
weighed scores with their measured numbers per context, the verdict — admitted,
rejected, or unjudgeable on available evidence — any geometry dependency, and
for a rejection the criterion, the number and what would change the answer. A
rejection under G7 carries the two obligations §1.6 sets.

### 2.1 The gates that do not need the sweep

Several gates are properties of `crates/straf3-sim/src/step.rs` and
`profile.rs` rather than of the candidate sweep, and they are settled here.
**These are gate results, not weighed scores**, and none of them is a verdict:
§1.5 makes a gate failure end a case, but a gate *pass* only earns the weighing.

They are published now, before the sweep, for the same reason Part 1 disclosed
two of them in advance — a gate consequence discovered after the numbers looks
exactly like a gate written after the numbers. Each is executed rather than read:
`crates/straf3-sim/tests/canon_gates.rs`.

**G5(a) — the disclosed one, run rather than predicted.** §1.3 G5(a) runs a
player who never exceeds `max_speed` on **flat open ground** and counts how many
times the mechanic becomes available; it fails if that count is not zero.

| Candidate | Count on flat ground | Result |
|---|---|---|
| Crouch slide | 0 | **pass** |
| Dash | **10 of 10 landings, at zero horizontal speed** | **fail** |
| Wall jump | 0 | **pass** |

The dash's failure is the one G5(a) disclosed in advance, in those words, from
`step.rs` alone: a standing player jumps on the spot, lands, and the landing arms
a dash window because arming is gated on `left_ground_by_jumping` and nothing
else. The test stands still and jumps ten times; it arms ten times. Under §1.5's
retune rule this is a **pre-registrable retune rather than an automatic
rejection**, and the retune is registered in §3.8 — before the re-measurement,
as §1.5 requires.

The two passes are published as the negative controls a gate needs. The slide
passes for the reason `profile.rs` designed in — `slide_entry_speed` 400 is above
`max_speed` 320, so ground acceleration alone cannot arm it. The wall jump passes
because `note_wall_contact` refuses any plane whose `|normal.z|` exceeds
`wall_normal_max` 0.3 and flat ground's normal is 1.0 — which is **amendment 2's
flat-ground ruling doing exactly what it was written to do**, and worth marking:
under the rejected seven-context reading the wall jump would have been ended here
with no weighing.

**G4 — no new binding. All three pass, and no candidate is ended by canon's own
choice.** The slide reads the crouch edge; the dash and the wall jump read the
jump edge; all are in the vocabulary §1.3 names. `Buttons` also carries `ATTACK`
and `WALK`, which are *not* movement inputs, so the test is behavioural rather
than a bitfield identity: holding both, at a speed where a slide is one crouch
edge away, arms nothing. Since G4 is marked **canon's own choice** in §1.0's
authority table, the fact that it ends no candidate is worth recording — nothing
in this wave rests on it.

**Still needing the sweep:** G2 (inertness across the published corpus), G5(b)
(the naive-to-best ratio), G7 part 2 (surviving discontinuities), and every
weighed criterion W1–W7. G1 is the determinism run; G6(b) and G7 part 1 need the
sweep's numbers beside the rule.

### 2.0 Known unmeasured questions, which a verdict must settle first

Recorded here so the next wave does not have to rediscover them. Neither is
measured; both are measurable on the harness that exists.

**Crouch slide: does tap-and-stand-up dominate?** `PM_Friction` reads
`slide_ms`, **not `p.crouched`**. So a player can tap crouch to arm the slide,
stand straight back up on the next command, and keep one-sixth friction for the
full 600 ms **at full wish speed** — because standing restores the wish speed
that `duck_scale` 0.25 was suppressing. If that is available it is strictly
better than sliding crouched, and it is the obvious thing a player will find.

Why this may decide the mechanic rather than merely tune it:
`PhysicsProfile::slide_duration_ms`'s own doc comment argues for a countdown
over hold-to-slide because "a slide the player can extend at will is a friction
toggle, and a toggle has nothing to master". That argument defends against
**extension**. It does not reach this: the duration is still bounded, but the
*posture cost* the mechanic was assumed to impose is not paid. If tap-and-stand
dominates, crouch slide is a 600 ms friction toggle with a speed price of
admission, and the anti-toggle case has to be made again on different ground.

The instruments already exist and no criterion needs changing. **W6** asks
whether the optimal play differs by context — if tap-and-stand is optimal
everywhere, that is a W6 fail and the mechanic is a routine rather than a
decision. **W1** asks whether the naive play harms. **G2**'s inertness is scoped
on activation preconditions, so a *standing* player carrying slide friction is
in scope for it. What is missing is only the measurement.

**Wall jump and crouch slide have no geometry in any shipped map.**
`assets/maps/coil.map` states in its own header that there are no ceilings
anywhere, and its one dramatic non-walkable surface is the gully's near wall at
normal z 0.5547 — above `wall_normal_max` 0.3 and below `min_walk_normal` 0.7,
inside the dead band where it is neither wall nor walkable. Training-map stubs
were brought into scope to close this. §1.5's geometry-dependency disclosure and
the unjudgeable verdict both exist because of it.

## Part 3 — The frozen ruleset

**Partly written.** The half of Part 3 that argues the *inherited* constants —
§3.1 to §3.5 below — is written, because none of it depends on a candidate
verdict. The candidate block (§3.7) and the constructor itself wait on Part 2,
and saying which is which is the point of this paragraph.

Two claims the previous wave's Part 3 stub made are now discharged. §3.0's
unexplored source has been explored and read from bytes; the result is in §3.6
and it is mostly a negative. Point 4's arithmetic has been replaced by a
measurement; the number is in §3.5 and it vindicates the map that was built to
it.

### 3.1 The rule, and the four grades

§1.8 point 5 states the rule this part applies: every constant canon carries
must either carry a citation to a verifiable source **at the grade that source
actually has**, or be a value Straf3 chose deliberately with the reason
recorded. "It was in CPM" is not a reason, because §4.1 says the objective is
not preservation.

Honouring "at the grade that source actually has" requires the grades to be
named, so that a constant cannot be quietly promoted by being described
warmly. Part 3 uses four:

| Grade | Means | What it is not |
|---|---|---|
| **A — id source** | Read from id Software's Quake 3 GPL release | — |
| **B — two reconstructions agree** | Two community reconstructions with no *stated* shared lineage carry the same value | **Not** verification against CPMA. CPMA's source is not public and no constant in this tree has been checked against it |
| **C — one reconstruction, or a shared lineage** | A single reconstruction, or two that trace to the same upstream | Not corroboration. Two copies of one document are one witness |
| **S — Straf3's choice** | No citable source at a usable grade; the value is chosen and the reason recorded | Not an admission of ignorance — §4.1 makes choosing the default, not the fallback |

Grade B is new to this wave and it is the most easily overstated thing in this
document, so its ceiling is stated twice: **B is agreement between two
reconstructions, and it is not verification.** A future wave that finds a CPMA
demo can raise these; nobody has.

### 3.2 The sixteen constants at Grade A

These are id's, read from the GPL release, and `profile.rs` marks each
*Verified*. `crates/straf3-sim/src/profile.rs`'s
`verified_constants_match_the_gpl_source` asserts them, and
`tests/movement.rs`'s `the_verified_constants_are_visible_in_the_movement_itself`
asserts they are *used* rather than merely stored — which is the difference
between a constant and a comment.

`accelerate` 10, `friction` 6, `stop_speed` 100, `max_speed` 320 (the `g_speed`
cvar default), `duck_scale` 0.25, `air_accelerate` 1, `gravity` 800,
`jump_velocity` 270, `step_height` 18, `overclip` 1.001, `max_clip_planes` 5,
`ground_trace_probe` 0.25, `min_walk_normal` 0.7, `hull_mins` (−15, −15, −24),
`hull_maxs` (15, 15, 32), `crouched_height` 16.

`straf3()` carries all sixteen unchanged. **This is a choice and not an
inheritance**, and §4.1 requires it to be argued rather than assumed: the
sixteen are the shape of the player and the shape of the world's response to
them, they are the substrate every measurement in `docs/movement-lab.md` was
taken against, and no candidate this wave proposes needs any of them to move.
Changing one would invalidate 2211 published measurements to buy nothing this
wave can name. That is the reason; it is not "they were in Quake".

### 3.3 `friction` 6, which is now justified rather than merely inherited

Grade A, and it needed the argument anyway. `cpm1_dev_docs` used **8**;
Xonotic's reconstruction states that "friction is 6 in all modern CPMA releases,
and in DeFRaG CPM", which is why GPP-1-1's CPM branch carries 8 and this tree
correctly does not.

This wave adds the observation behind that assertion. `ratoa_gamecode`'s
`bg_pmove.c` declares `const float pm_friction = 6.0f` as its **base**, and —
the load-bearing half — **there is no `pm_cpm_friction` anywhere in the file**.
A CPM-capable port carries 6 and never overrides it in its CPM branch. That is
Xonotic's claim *observed* rather than asserted, which is a better thing to
have. Read from bytes by `canon`; custody in §3.6.

### 3.4 The six CPM constants, and what changed for four of them

These are the `TODO(wave2)` block: the values `cpm()` adds on top of the VQ3
base. None is Grade A and none ever will be without a CPMA demo.

| Constant | Value | Grade | On what |
|---|---|---|---|
| `accelerate` (CPM) | 15 | **B** | GPP-1-1's `bg_promode.c`; `ratoa` `pm_cpm_accelerate 15.0f` |
| `air_stop_accelerate` | 2.5 | **B** | as above; `ratoa` `pm_cpm_airstopaccelerate 2.5f` |
| `strafe_accelerate` | 70 | **B** | as above; `ratoa` `pm_cpm_airstrafeaccelerate 70.0f` |
| `strafe_wish_speed_cap` | 30 | **B** | as above; `ratoa` `pm_cpm_airstrafewishspeed 30.0f` |
| `air_control` | 150 | **C** | `ratoa` carries 150 but **declares itself Xonotic-derived** — same lineage, not a second witness |
| `double_jump_window_ms` | 400 | **S** | §3.7 |
| `double_jump_boost` | 100 | **S** | §3.7 |

**Why `air_control` did not move with the other four, stated because the
temptation to move it was real.** `ratoa`'s `bg_pmove.c` carries
`pm_cpm_aircontrol = 150.0f`, which is this tree's value exactly. But the
function that consumes it is headed *"Copied with edits from `cl_input.c` from
Xonotic's Darkplaces engine"*. It is the lineage canon already cites, arriving a
second time — and §1.6's whole finding about the double jump was that repeated
copies of one upstream are one witness. Applying that finding only where it is
convenient would be worse than not having made it. `air_control` stays at
Grade C.

One structural difference in the same function, recorded so that a later reader
does not find it and think it was missed. `ratoa` computes
`k = 32 · clamp(0, 1, wishspeed / airstrafewishspeed) · dot² · dt · aircontrol`;
this tree (`step.rs:947`) computes `k = 32 · air_control · dot² · dt`, without
the clamp factor. Stated precisely rather than alarmingly: with the cap at 30
and an ordinary wishspeed near 320 that factor saturates at 1, so the two agree
in all normal play and diverge only below wishspeed 30 — a barely-touched stick.
The value is corroborated; the formulas are not identical, and Part 3 does not
claim they are.

**And the ceiling on all of Grade B, once more.** Four constants moved from one
reconstruction to two. Neither reconstruction is CPMA. Nothing in either
`bg_pmove.c` or `bg_public.h` states where `ratoa` got its values, and no README
claiming independent derivation was found — so **independence from
`cpm1_dev_docs` is unestablished even for the four that moved**, and four
matching numbers must not be read as implying a provenance none of them carries.
What Grade B says is: two people reconstructing CPM wrote down the same number.
That is worth having and it is not verification.

### 3.5 The jump, measured

Part 3's previous draft recorded that both Straf3 jump figures were
`270²/(2·800)` and `2·270/800` — **arithmetic agreeing with a map comment**, and
that nobody had measured this tree's actual jump. That is now done.

`crates/straf3-sim/tests/canon_jump.rs` drops a player onto `FlatGround`, lets
them settle, presses jump and watches the origin. Nothing in it computes an
expected value from `jump_velocity` and `gravity`; that is the point, because
the closed form is the thing being checked. At 125 Hz, the rate §1.8 point 3
fixes:

| Quantity | **Measured** | `coil.map` asserts | Closed form |
|---|---|---|---|
| Apex above rest | **45.561630 units** | 45.6 | 45.5625 |
| Airborne | **680 ms** (85 commands) | 0.675 s | 675 ms |

**`coil.map` is vindicated rather than contradicted, and that is worth saying
plainly because the course was built to those numbers.** The apex agrees to the
precision the map states. The airborne figure is quantised to the command grid —
the player is airborne at the end of command 84 (672 ms) and grounded at the end
of command 85 (680 ms) — so the continuous landing lies in **(672, 680] ms**,
and 675 falls inside that bracket. There is no finding against the map here.

**Straf3's jump belongs to neither clause alone, and the measurement shows why.**
`jump_velocity` 270 is id's at Grade A without qualification. The *integrator* is
Straf3's own: `step.rs` integrates gravity at the average of the start and end
vertical speeds within a sub-step. The resulting behaviour is a consequence of
both, so it is neither a cited constant nor a free choice. The integrator's own
claim — that this makes jump height "nearly independent of the command rate" —
turns out to understate itself:

| Rate | Apex | Airborne |
|---|---|---|
| 250 Hz (4 ms) | 45.561630 | 676 ms |
| 125 Hz (8 ms) | 45.561630 | 680 ms |
| 62.5 Hz (16 ms) | 45.561592 | 688 ms |

The apex at 250 and 125 Hz is identical to the last digit printed, and 62.5 Hz
differs by 4·10⁻⁵ units. Only the airborne duration moves, by one command's
width, which is quantisation rather than physics.

For contrast and not as a target: Xonotic's authors measured Quake 3 at 125 fps
at **48.528 units over 720 ms**. Straf3 is lower and shorter. Neither figure is
evidence about the other, because the integrators differ; the comparison is
recorded because a reader who knows Quake will otherwise wonder.

### 3.6 §3.0's unexplored source, now explored

The previous Part 3 named `rdntcntrl/ratoa_gamecode` (OpenArena Ratmod) as the
one reconstruction nobody had examined — recommended over freepromode by
freepromode's own author, and "the obvious next place to look before Part 3
argues `double_jump_window_ms` or `double_jump_boost`". It has now been looked
at, and the result is mostly a **negative**, which is why it is written up at
length rather than dropped.

*Chain of custody, per §1.6's closing rule.* `code/game/bg_pmove.c` and
`code/game/bg_public.h` from branch **`ratoa`** (`master` 404s), fetched raw with
`curl`, hashed, and read in full by **`canon`** — `sha256 d10e1335…` (2514 lines)
and `sha256 7a699cfe…`. Not through a summarising fetch.

**The `bg_promode.c` trap is absent here, and it was checked rather than
assumed.** Every movement constant is declared `const float` at file scope, so
it *cannot* be reassigned, and every other occurrence of each identifier is a
read inside the `PM_Get*` accessors. There is no `CPM_UpdateSettings` equivalent.

**What it gave: §3.4's Grade B for four constants, and §3.3's observation.**

**What it did not give, which is what §3.0 actually hoped for.** `ratoa` does
contain a 400 and a 100 together, in `PM_CheckJump`, which looks at first exactly
like the double-jump attestation §1.6 could not find. It is not one, for three
independent reasons, any one of which is sufficient:

1. `pm->ps->stats[STAT_JUMPTIME] = 400;` is assigned **unconditionally**, outside
   every `if`, on every successful jump — including the first jump from
   standing. It is not gated on the jump having ended a jump.
2. **It is set at the moment of the jump, not at the landing.** Straf3's window
   opens *on landing* (`step.rs:740-742`, gated on `left_ground_by_jumping`), so
   `ratoa`'s 400 ms runs from the previous jump and Straf3's runs from the
   landing. These are different quantities that share a number. And §3.5's
   measurement makes the difference concrete: a flat-ground jump→land→jump has
   ≥675 ms between the two jumps, so `ratoa`'s timer has **already expired at the
   landing** — its boost can never fire on a flat-ground double jump at all. It
   fires only when a second jump follows within 400 ms of the first, which
   requires a short air time: higher ground, or a ramp.
3. **The source's own author calls it a ramp jump.** `bg_public.h:293` reads
   `STAT_JUMPTIME,	// rampjump`; the boost is gated on `RAT_RAMPJUMP`; and the
   commented-out lines beside it add *horizontal* velocity along `pml.forward`.

This is the right number attached to a different mechanism — the `bg_promode.c`
failure in a new costume, caught this time because the file was read rather than
summarised. **§1.6's instruction therefore stands unchanged**, and §3.7 applies
it.

### 3.7 The two constants Straf3 chooses

Grade S. Neither is cited, and after §3.6 that is a demonstrated conclusion
rather than an absence of evidence — which is a materially stronger position
than the previous wave's.

**`double_jump_window_ms` = 400.** Chosen. Every attestation traces to
`cpm1_dev_docs`; freepromode's README gives the number verbatim but describes
itself as a *less accurate* imitation after its author "tried to purge" that
upstream; Xonotic sets `sv_doublejump 0`; and `ratoa`'s matching 400 measures a
different quantity (§3.6). *The reason Straf3 chooses 400:* it is the value the
tree's entire published measurement corpus was taken under, the value
`docs/movement-lab.md`'s bunnyhop and double-jump tables describe, and no
evidence recommends a different one. Moving it would invalidate those tables to
express a preference nobody has stated. **Straf3 keeps 400 because it is the
incumbent and nothing argues against it — not because CPM had it.**

**`double_jump_boost` = 100.** Chosen, at Grade C-going-on-S. GPP-1-1's
`bg_promode.c` assigns `cpm_pm_jump_z = 100/*/230*/; // enable double-jump //100`
inside the pro-mode branch of `CPM_UpdateSettings` — a port's own CPM
configuration, but third-hand about CPMA, and the same file declares the same
variable at file scope as `0.5` with a comment whose arithmetic disagrees with
itself. `ratoa`'s `+100` is a `RAT_RAMPJUMP` boost and does not corroborate it.
*The reason Straf3 chooses 100:* the same incumbency argument, plus the
structural one §1.6 rates above both magnitudes — GPP-1-1's vq3 branch sets the
same field to zero, spelling "VQ3 is CPM with the extensions switched off" as
data on the very field `profile.rs` uses. `ratoa` spells the same relationship a
second way, carrying `pm_accelerate 10.0f` beside `pm_cpm_accelerate 15.0f` and
switching between them on movement mode. **The relationship is better attested
than either magnitude, and it is the relationship `profile.rs` encodes.**

### 3.8 The candidate constants

**Awaiting Part 2.** The eight candidate constants — `slide_entry_speed`,
`slide_friction`, `slide_duration_ms`, `dash_speed`, `dash_window_ms`,
`wall_jump_velocity`, `wall_contact_window_ms`, `wall_normal_max` — all exist on
`PhysicsProfile` already, so an admission is a value change and a rejection or an
unjudgeable verdict leaves them at the disabling values `vq3()` and `cpm()`
carry. This section is written when the verdicts are.

One thing is settled ahead of them, under §1.5's pre-registration rule and
recorded here because a retune registered after its measurement is not evidence:
**the dash's single permitted retune is a new `dash_entry_speed` = 400**,
mirroring `slide_entry_speed`, tested against horizontal speed at the arming
landing. It is predicted to move G5(a) from fail to pass, possibly to lower W4's
context count, and to move no other gate. This is the one route to a new field
on `PhysicsProfile` that Part 3's previous draft identified, and it lands **only
if the dash is admitted** — a field that exists is folded into the physics digest
by `identity.rs` whether or not canon uses it, so a rejected dash must not leave
one behind.

---

## On citations, because this cost the wave two corrections

Every citation in this document was first gathered through a summarising web
fetch, and **two of the three were wrong in ways the summary could not show**.

- `bg_promode.c` declares Tremulous-tuned values at file scope and **overwrites
  them at runtime** in `CPM_UpdateSettings`. A summary reports the declarations
  and misses the function, producing a confident and false "`air_control` is 165
  modulated by 0.8".
- freepromode's README was cited for the one line matching this tree's 400 ms,
  without the three disclaimers immediately below it — which are the most
  important content in the file and change what the citation is worth.

Neither error was visible in the summary. Both took under a minute to find by
fetching the raw file and reading it. **The failure mode is not that summaries
are vague; it is that they are confidently specific about the wrong part of the
file**, which is indistinguishable from being right unless you open it.

So: a constant in Part 3 may be cited only against a source someone has read
from bytes, and the citation records who read it. All three sources behind §1.6
now meet that bar.

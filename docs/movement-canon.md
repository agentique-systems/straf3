# Canonical Straf3 movement

**Status of this document.** Part 1 is written. Parts 2 and 3 are not.

| Part | What it is | State |
|---|---|---|
| 1 | The criteria a mechanic must meet to enter canon | **written, and frozen against candidate evidence** |
| 2 | The verdict on crouch slide, dash and wall jump | not yet written |
| 3 | The frozen `PhysicsProfile::straf3()`, constant by constant | not yet written |

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

---

## Part 1 — What a mechanic must be to enter canon

### 1.0 The shape of the decision

A candidate faces **eight gates** and then **seven weighed criteria**.

The gates come from §4.4's movement anti-goals and §21's confirmed anti-goals.
They are gates because the vision states them as things Straf3 must not become,
not as things to be traded off: a mechanic that automates execution is not
redeemed by being fun, because a game where automation replaces execution is a
game the vision has already refused to make. **Failing one gate ends the case.**
There is no weighing afterwards and no aggregate score in which a gate failure
can be outvoted.

The weighed criteria come from §4.2's eight properties of a strong mechanic and
from §1's north star. They are weighed because §4.2 says a strong mechanic
"should generally be" these things — the hedge is in the vision's own wording,
and honouring it means admitting that a mechanic can be excellent and imperfect.
The arithmetic is in §1.3.

Four questions are reserved for the operator's playtest, in §1.4, because
measurement genuinely cannot settle them. They are kept to four so that the
playtest has a short list rather than a survey.

### 1.1 The gates

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

*Why a gate:* §4.2 names determinism outright and §8's competitive integrity
depends on it. A run that cannot be re-simulated cannot be a record, so this is
not a movement-quality question at all — it is a question about whether the mode
of play exists.

---

**G2 — Inert when not invoked.**

*Decides it:* run the whole published measurement set — the 2137 named values in
`docs/movement-lab.md` and `tools/straf3-lab/measurements.pinned.tsv` — under
the candidate profile and diff it against the control profile. Every measurement
that does not involve the mechanic's own input must be **bit-identical**.

*Fails if:* any canonical number moves. Not "moves a little": moves.

*Why a gate:* §4.4's "makes the canonical movement language incoherent". A
mechanic that changes what a strafejump is worth when the player is not using it
has not been added to the language, it has replaced part of it, and every
existing measurement, every training map and every player's muscle memory is now
describing a game that no longer exists. This is also the criterion that keeps
the evidence readable: if enabling a mechanic moves unrelated numbers, no later
comparison is attributable to the mechanic.

*Note:* this is the measurement-side statement of the rule
`PhysicsProfile::experimental()`'s doc comment already gives as the reason it is
`..Self::cpm()` — one variable at a time.

---

**G3 — Immediate.**

*Decides it:* the number of commands between the command carrying the input and
the first command on which the player's velocity differs from a control run that
did not press it. And, separately, the number of commands on which the mechanic
causes an input to be ignored.

*Fails if:* the first is not **zero** — the effect must land on the command of
the press — or the second is not **zero**.

*Why a gate:* §4.4's "weaken responsiveness for visual spectacle" and §19's
priority 2. A wind-up is the game taking commands away from the player and
giving them back later; an animation lock is the same thing with a nicer name.
Note what this gate does *not* forbid: an *availability window* opened by an
earlier event is fine, because the player is not deprived of control while it
runs. The forbidden thing is a delay between deciding and moving.

---

**G4 — No new binding.**

*Decides it:* count the input bits and axes the mechanic reads that are not
already part of the movement input vocabulary (the two move axes, the view,
jump, crouch).

*Fails if:* the count is not zero.

*Why a gate:* §4.2's opening sentence — "the input vocabulary should remain
relatively compact" — and §21's "a generic ability shooter with movement
mechanics attached". Depth is supposed to come from timing, direction, geometry
and sequencing, and a mechanic on its own key has none of those to find: it is
available whenever it is available and it costs nothing else to use. Overloading
an existing input is what makes a mechanic cost something, because the input is
already doing a job.

*This gate is falsifiable and I expect it to be argued with eventually.* A
future mechanic that genuinely needs a binding is not forbidden forever; it is
forbidden until someone amends this document ahead of measuring it, under the
threshold-edit rule above. `crates/straf3-sim/src/step.rs` already records that
the replay codec would not object to a new button bit — so the constraint is a
design decision and is being kept as one, not a format limitation dressed up as
a principle.

---

**G5 — Earned, not refilled.**

*Decides it:* two measurements. (a) Availability count over a fixed interval for
a player who never performs the arming event — standing still, walking, falling.
(b) Whether the optimal policy in the sweep of §1.2 is "use it at the first
moment it is available, aimed at the current heading".

*Fails if:* (a) is non-zero, or (b) is within 5% of the best outcome the sweep
found.

*Why a gate:* §4.4's "replace momentum mastery with cooldown rotations" and
§21's "a cooldown-rotation game". These are the two halves of what a cooldown
actually is: it arrives on a schedule you did not earn, and the correct play is
to spend it immediately. A window opened by a landing you had to reach at speed
fails neither half. A timer that refills fails the first; a button whose optimal
use is "press it now" fails the second even if it was earned, because the
decision has already been made for the player.

---

**G6 — No cap.**

*Decides it:* read the implementation for any clamp on the magnitude of
`velocity` or of its horizontal component; and compare every terminal speed in
`docs/movement-lab.md` §6 under the candidate profile against the control.

*Fails if:* such a clamp exists, or any terminal speed is *lower* under the
candidate.

*Why a gate:* §4.4's "impose arbitrary speed limits to compensate for poor
design" and §21's "a game where arbitrary speed caps solve map-design problems".
Note that this gate is about the *ceiling*, not about clamps in general:
`PM_Accelerate`'s clamp on the projection of velocity onto the wish direction is
the mechanism strafejumping is built out of and is emphatically permitted. The
forbidden shape is a limit on how fast the player may end up going.

---

**G7 — Attributable.**

*Decides it:* two parts, both measured.

1. *The rule predicts the outcome.* The verdict must state a rule — a formula
   or a short algorithm — that computes the mechanic's effect from quantities
   the player can perceive before invoking it: their speed, their direction
   relative to the wish direction, whether they are grounded, what they are
   touching. The lab measures the outcome across the sweep and publishes it
   beside the rule's prediction. They must agree, in the same
   measured-versus-closed-form form `docs/movement-lab.md` §1 and §4 already
   use.
2. *No invisible cliff.* Sweep each player-controlled input parameter at fine
   resolution. Find the largest single-step change in outcome. Any
   discontinuity larger than **5% of the outcome** must coincide with a boundary
   the player can see or hear — a surface they are touching, a state the overlay
   shows, a threshold marked in the world.

*Fails if:* no rule predicts the measurement, or a discontinuity above 5%
happens somewhere the player has no way to perceive.

*Why a gate:* §4.4's "make important outcomes opaque or impossible to
understand", and §20's Proof 2 — "players can learn primitives, combine them,
understand failures, and deliberately improve". Understanding a failure requires
that the failure had a reason visible at the time.

*The worked counterexample is already in the tree, and it is honest to say so.*
`docs/movement-lab.md` §4 measures overbounce: a 16.000-unit drop returns 100%
of the impact speed and a 16.500-unit drop returns 0.1%, and 4.34% of the 8064
drops sampled between 16 and 1024 units overbounce, scattered with nothing in
the world marking which. That is a 100-percentage-point discontinuity across a
half-unit of drop height that the player cannot see coming. **Overbounce would
fail G7 if it were proposed today.** It is in canon because it is inherited, and
what to do about inherited behaviour is explicitly not decided here — see §1.5.
The gate is stated in a form that catches it anyway, rather than in a form
carefully shaped to let it through, because a gate written to spare an incumbent
is not a gate.

---

**G8 — Data, not a branch.**

*Decides it:* read `crates/straf3-sim/src/profile.rs` and
`crates/straf3-sim/src/step.rs`. The mechanic must be expressed as constants on
`PhysicsProfile` such that a stated value of those constants switches it off,
and `step.rs` must contain no test of *which profile* is in use.

*Fails if:* there is an `if canon { … }`, a profile-identity comparison, a
`bool` field that selects an algorithm, or a mechanic that cannot be switched
off by its own constants.

*Why a gate:* `PhysicsProfile`'s own doc comment already sets this rule — "a
field here is a promise that the value is genuinely a number the simulation
reads, not a switch that selects a different algorithm" — and
`crates/straf3-collision/tests/canon_frozen.rs` already enforces the disabling
half by exhaustive destructure. It is restated as a gate because it is the
property that makes every other gate checkable: a mechanic that is a branch
cannot be A/B measured against a control, cannot be recorded into a replay, and
cannot be tuned without a rebuild.

*One clarification, because the tree already contains the exception:* a
*threshold* constant does not have to be disabling. `wall_normal_max` and
`strafe_wish_speed_cap` are both read only when another constant is non-zero,
and zero is a meaningful value for each of them rather than an "off". A
candidate may have at most one such constant, and the verdict must name it and
name the constants that gate it.

### 1.2 The weighed criteria

These are scored **pass / weak / fail** against stated thresholds. Every
threshold is a choice, not a derivation, and each one below says what it is
calibrated against — mostly against numbers `docs/movement-lab.md` has already
published about the *canonical* vocabulary, which is legitimate to use here
because they are not candidate measurements.

**The sweep.** W1, W2, W5 and W6 are all scored from one measurement, so it is
defined once. For each of the **seven contexts** listed below, and for each
entry speed in {320, 400, 500, 640, 800, 1000} ups, sweep the mechanic's
decisive player-controlled parameters — the command it is invoked on, across
the whole availability window at 8 ms resolution, and the wish direction
relative to the current velocity, at 5° — and record the outcome. **Outcome** is
horizontal speed at a fixed horizon of 1 second after the invocation window
closes, measured against a control run in the same context from the same entry
speed that never invoked the mechanic.

The contexts, which are `straf3_collision::testbed` and nothing new except where
noted:

| # | Context | Kind |
|---|---|---|
| 1 | Flat ground (`floor`) | surface |
| 2 | Walkable ramp, 26° (`ramp`) | surface |
| 3 | Sliding ramp, 50° (`ramp`) | surface |
| 4 | Climbable riser, 18 units (`step`) | edge |
| 5 | Ledge and 256-unit drop (`ledge`, `drop_from`) | edge |
| 6 | Inside corner, two walls (`corner`) | wall |
| 7 | Low ceiling (`ceiling_at`) | ceiling |

A candidate whose case needs geometry not in that list may have it built, but
the verdict must say so, and a mechanic that pays *only* in geometry that had to
be built for it is answered by W4.

---

**W1 — Learnable.** *(§4.2 "understandable at a basic level"; §1 "easy to
learn".)*

*Decides it:* the **naive-harm rate** — the fraction of opportunities in the
sweep where a naive policy (invoke at the first available command, aimed along
the current heading) leaves the player *worse off* than the control that did not
invoke it at all.

*Pass:* ≤ 20%. *Weak:* 20–35%. *Fail:* > 35%.

*Calibration:* chosen, not derived. What the number has to be is well below
half, so that a beginner who presses the button at the obvious moment is
building a habit that helps them rather than one they will have to unlearn. 20%
is where "usually helps" stops being an honest description.

---

**W2 — Masterable.** *(§4.2 "difficult to perfect"; §1 "difficult to master".)*

*Decides it:* two numbers from the same sweep.

- The **naive-to-optimal gap**: `(best outcome − naive outcome) / best outcome`,
  averaged over contexts.
- The **execution window**: the width, in milliseconds, of the set of invocation
  timings that yield ≥ 95% of the best outcome.

*Pass:* gap ≥ 20% **and** execution window ≤ 384 ms. *Weak:* gap 10–20%, or
window 384–600 ms. *Fail:* gap < 10%, or window wider than the availability
window itself — which would mean every legal invocation is near-optimal and
there is nothing to hit.

*Calibration:* both from published canon numbers. The gap: at 320 ups entry, a
VQ3 player holding 30° gains 49.51 ups/s and one holding the optimal 52° gains
197.45 (`docs/movement-lab.md` §1) — a 75% gap, and at 20° it is 90%. A
candidate scraping 20% is already three to four times shallower than the
shallowest thing the game currently teaches, so a failure at 10% is not a close
call. The window: 384 ms is the *loosest* timing canon has, the measured usable
double-jump delay (§2); the tightest is the jump re-arm at one command, 8 ms,
which has no tolerance at all. A candidate looser than 384 ms is looser than
anything a player currently has to hit.

---

**W3 — Composable.** *(§4.2 "composable with other mechanics"; §4.2's thesis
that "advanced movement should come from combining understandable primitives".)*

*Decides it:* three numbers.

1. **Chain gain.** Over a fixed course in each context, compare the best outcome
   when the mechanic is used *instead of* the existing technique against the
   best outcome when it is used *in addition to* it. Composable requires
   `both > either alone` by at least 5%.
2. **Entry-speed sensitivity.** Outcome as a function of entry speed. A
   composable mechanic hands back speed that grows with the speed brought in.
   Requires `d(outcome)/d(entry) > 0` across the swept range, and
   `outcome ≥ entry` wherever the mechanic is correctly used.
3. **Momentum conservation.** The mechanic must never *set* speed to a value
   independent of what the player arrived with.

*Pass:* all three. *Weak:* (1) holds but sensitivity is flat over part of the
range. *Fail:* the best line never uses both, or the mechanic levels entry speed
to a constant.

*Calibration:* the 5% on chain gain is the same materiality threshold used
throughout this document; see W4.

*Why it is scored this way:* a mechanic that substitutes for strafejumping
rather than chaining with it does not add to the language, it replaces part of
it — and a mechanic that hands every player the same exit speed regardless of
what they carried in has erased the thing the previous ten seconds of play were
about. That second failure is a speed cap wearing a mechanic's clothes; it is
scored here rather than at G6 because it caps a *technique* rather than the
player, which is a difference of degree.

---

**W4 — Useful in more than one situation.** *(§4.2 "useful in multiple
situations".)*

*Decides it:* the number of the seven contexts in which the mechanic's best
outcome beats the control by ≥ **5%**, and the number of distinct *kinds*
(surface / edge / wall / ceiling) among them.

*Pass:* ≥ 3 contexts spanning ≥ 2 kinds. *Weak:* 2 contexts, or 3 within one
kind. *Fail:* 1 context.

*Calibration:* the 5% materiality threshold is taken from
`docs/movement-lab.md` §2, whose bunnyhop-window table treats 1%, 5% and 10% of
speed as the meaningful gradations of a speed change; 5% of a 320 ups run is 16
ups, which is legible on the speed overlay while the player is moving. The
"3 contexts, 2 kinds" shape exists because three ramp angles are one situation
measured three times, not three situations.

*Why:* a mechanic that pays in exactly one place needs maps built around it, and
"maps built to accommodate a mechanic" is the inverse of §7's "movement
mechanics cannot be evaluated independently from the spaces in which those
mechanics are used" — it makes the space a jig.

---

**W5 — Vocabulary-conserving.** *(§4.4 "make the canonical movement language
incoherent"; §3's balance of execution and discovery.)*

*Decides it:* for each existing technique the lab measures — `ground_turn`,
`air_forward`, `air_strafe`, `bunnyhop`, the drop launch, ramp traversal,
step-up — is there still at least one (context, entry speed) cell in the sweep
where it is the best option available?

*Pass:* every existing technique survives somewhere. *Weak:* one becomes
never-optimal. *Fail:* two or more do.

*Why:* this is the arithmetic behind "the movement language got bigger". Adding
a mechanic that makes two existing techniques pointless is a net loss of one,
however good the new one is, and the players who learned them have had their
practice deleted. It is deliberately scored on *never-optimal*, not on
*weakened*: a new mechanic taking some of an old one's territory is what
composability looks like.

---

**W6 — Decision-creating.** *(§4.4 "introduce complexity without increasing
meaningful depth" and "reduce movement to memorizing ability sequences"; §21's
"excessive complexity substitutes for depth".)*

*Decides it:* across the seven contexts, does the *argmax* — the optimal
invocation timing and the optimal aim relative to velocity — actually change?
Requires the optimum to differ between at least two contexts by more than 10% of
the swept range of that parameter.

*Pass:* the optimum varies in both timing and aim. *Weak:* varies in one.
*Fail:* the optimum is the same everywhere.

*Why:* a mechanic whose best use is identical in every situation is not a
decision, it is a step in a routine — which is precisely §4.4's "reduce movement
to memorizing ability sequences". Depth is the player having to work out *which*
use is right here, and that only exists if the answer differs.

---

**W7 — Cheap.** *(§4.4 "introduce complexity without increasing meaningful
depth", cost side.)*

*Decides it:* three counts the verdict must publish — new `PhysicsProfile`
constants, new `PlayerState` fields, and new activation preconditions (distinct
state predicates gating the mechanic in `step.rs`) — and the one-sentence
statement of the rule required by G7.

*Pass:* ≤ 3 activation preconditions, and the rule fits one sentence naming only
things a player can perceive. *Weak:* 4 preconditions. *Fail:* 5 or more, or the
honest statement needs more than one sentence.

*Calibration:* three, because that is what the existing vocabulary costs. A
double jump is "land from a jump, jump again soon" — two. A strafejump is "be in
the air, hold a direction off your velocity" — two. A candidate needing five
conditions is not being learned by anyone from playing.

*Note:* the count of constants and state fields is *published, not gated*. It is
information the operator and the next wave need; a mechanic with four constants
that is genuinely excellent should not be rejected by an accountant.

### 1.3 The arithmetic

Stated so a verdict can be argued with rather than asserted.

1. **Any gate fails → rejected.** The weighing does not happen. The verdict
   records which gate, the number, and what would change it.
2. **All gates pass →** score W1–W7.
3. **Admitted** requires **W1, W2 and W3 all at pass**, and among W4–W7 **no
   fail and at most one weak**.
4. **Anything else → rejected for this wave**, naming the criterion, the number,
   and what would change the answer.

*Why W1–W3 are required rather than counted:* W1 and W2 are §1's north star
stated as two numbers — easy to learn is a low naive-harm rate, difficult to
master is a large naive-to-optimal gap — and a mechanic failing either fails the
sentence the whole game is built on. W3 is §4.2's thesis that depth comes from
combining primitives; a mechanic that does not combine is a primitive that
stands alone, which is an ability, which is §21's first confirmed anti-goal.

**The retune rule.** A candidate's constants are opening positions chosen to put
the mechanic in a measurable regime, not tuned values —
`PhysicsProfile::experimental()`'s own doc comment says so. So one retune is
permitted per candidate, under a pre-registration rule identical in spirit to
this document's own: the verdict must name the constant, state the direction of
the change and predict which criterion it will move, **before** the re-measurement
is run. A retune discovered after the fact, or a second retune, is not evidence —
it is a search for a number that passes, and `experimental()`'s doc comment
already gives the answer to that: "a mechanic whose case depends on finding
exactly the right constant has already failed *simple to invoke*."

**A weighed fail can be overturned only by the playtest**, never by another
measurement. If the operator plays a mechanic that measured badly and reports
that it is nonetheless movement worth mastering, that is evidence of the kind
§1.4 exists to collect and it outranks a threshold I chose. The reverse is also
true and matters more: a mechanic that passes everything here and plays badly is
rejected. §4.3's standard is "testing demonstrates that it improves Straf3", and
the operator's hands are part of the testing.

### 1.4 What measurement cannot settle

Four questions. They are reserved rather than proxied because a proxy metric
that does not mean what it claims is worse than an honest gap — it converts a
question into a number and then the number gets cited.

| | Question | Why measurement cannot settle it | Where it is asked |
|---|---|---|---|
| **P1** | Did *you* execute it, or did the game do it for you? | G3 measures that no command is taken from the player. It cannot measure whether the outcome felt authored. A mechanic can be fully player-controlled and still feel like it happened to you. | PLAYTEST.md §3 q13, q15 |
| **P2** | When it did not fire, could you tell why from the screen? | G7 measures that a *rule* exists and that discontinuities sit on perceptible boundaries. Whether the boundary is perceptible **as rendered, at speed, on the day** is a fact about the client and the player, not about the simulation. | PLAYTEST.md §3 q16 |
| **P3** | Does it belong to the same game as strafejumping, or is it bolted on? | §4.4's coherence anti-goal. G2 measures that nothing else moved and W5 that nothing else died, and a mechanic can pass both and still feel imported from a different game. There is no measurement of idiom. | PLAYTEST.md §3 q14 |
| **P4** | Keep, cut, or revise? | §4.3's actual standard. Everything above is instrumentation for this question. | PLAYTEST.md §3 q17 |

PLAYTEST.md §3 already asks all four; its appendix maps every question to the
§4.2 property it tests. Nothing new needs inventing, and the playtest checklist
is not lengthened by this document — it is pointed at.

**What this means if the playtest does not happen.** The recommended profile
stands as *provisional* canon, recorded as provisional. A verdict resting only
on Part 1's measurements has answered every question except the four above, and
saying so is better than promoting a measured result to a decided one.

### 1.5 What this document is not deciding

Named, because an open question left visible is worth more than one silently
closed.

1. **Whether canon's inherited behaviours would pass these criteria.** They
   were not judged by them; they arrived with Quake. Overbounce fails G7 as
   shown above, and the drop launch — the largest speed gain in the game,
   `docs/movement-lab.md` §4 and §6 — is an accident of `PM_WalkMove`'s
   length-preserving rescale rather than a designed mechanic. Whether either
   belongs in Straf3 is a real question and a different argument, with a
   different cost: these are behaviours players have already built routes
   around. This document judges *candidates*. It deliberately does not
   grandfather anything by writing the criteria loosely enough to admit it.
2. **The tuned values of an admitted mechanic's constants**, beyond the single
   pre-registered retune of §1.3. Tuning is a different activity from judging
   and needs the operator's hands, not a sweep.
3. **Simulation frequency.** §22 leaves it open; every number in this document
   and in the lab is at 125 Hz, and the rate is part of the physics.
4. **Whether `coil` — or any map — has enough distinct routes to judge a
   mechanic against.** `docs/movement-lab.md` Limits §6 says plainly that the
   only perturbation harness that exists cannot answer this honestly. W4 and W6
   are therefore scored on testbed geometry, and route-level evidence is P3's
   and P4's job.
5. **What makes a *map*, a *profile*, or a *constant* canonical.** These are
   criteria for a mechanic. Inherited constants are governed by a different and
   simpler rule, which Part 3 applies: every constant canon carries must either
   carry a citation to a verifiable source or be a value Straf3 chose
   deliberately, with the reason recorded. "It was in CPM" is not a reason —
   §4.1 says the objective is not preservation.
6. **Whether a rejected mechanic is rejected forever.** It is not. A rejection
   under §1.3 must name the criterion, the number and what would change the
   answer, precisely so that the next wave can either satisfy it or argue with
   it rather than re-proposing the mechanic from scratch.
7. **Whether these criteria are right.** They are version 1 of an answer to a
   question §22 deliberately left open, and the honest status of an answer to
   such a question is "the best available, written down where it can be
   attacked". The threshold-edit rule at the top of this document is what makes
   attacking it productive.

### 1.6 Conditions the evidence must meet

Three, so that a verdict cannot be built on measurements that were never
comparable.

1. **Measured under the integration canon will ship with.** Every number in
   `docs/movement-lab.md` today is taken under single-step integration, and its
   own Limits §1 says so: `pmove_msec` sub-stepping is not implemented, and
   §4's overbounce counts in particular are a per-command artefact that
   sub-stepping is expected to move. A candidate judged on numbers taken under a
   known-superseded integration has been judged on a model the game will not
   run. Candidate measurement therefore follows sub-stepping, not the reverse.
2. **One variable.** The candidate profile must differ from its control by the
   candidate's constants and nothing else, per `experimental()`'s doc comment.
3. **Against a stated control.** Every outcome in §1.2 is a difference from a
   control run in the same context at the same entry speed. An absolute number
   with no control is not evidence about a mechanic; it is evidence about a
   world.

---

## Part 2 — The verdicts

Not yet written. Will record, for each of crouch slide, dash and wall jump: the
gates, the weighed scores with their measured numbers, the verdict, and — for a
rejection — the criterion, the number and what would change the answer.

## Part 3 — The frozen ruleset

Not yet written. Will record `PhysicsProfile::straf3()` constant by constant,
each with either a citation or the reason Straf3 chose the value.

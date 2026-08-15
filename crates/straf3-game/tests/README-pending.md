# `input_and_pacing_seam.rs.pending` — staged, not yet compiled

18 tests for **criteria 3 and 5 at the library seam**, written against the API
platform published (`command_from_input`, `InputState`, `MouseLook`,
`advance_one`, `FixedStep`, `plan_ticks`).

It carries a `.pending` extension so Cargo ignores it. Rename to
`input_and_pacing_seam.rs` once `straf3-game`'s lib target is merged into this
worktree, then compile, fix any signature drift, and mutation-prove it the same
way `../../straf3-platform/tests/mutation-proof.sh` does the oracle.

It is staged rather than shipped for one reason: it has never been compiled,
because `straf3_game`'s lib does not exist in this worktree yet. Shipping
uncompilable code into a shared tree would present a typo of mine as platform's
breakage.

What it pins that nothing else does:

- `right_move == +127` for `MoveRight`. Flip that sign and the game strafe-jumps
  mirrored, with no compile error and no other test failing.
- The view is an **absolute** angle, not the last mouse delta — a recorded delta
  only reproduces on the machine and sensitivity that recorded it.
- Yaw stays wrapped to (-180, 180], which preserves angular resolution.
- Jump and crouch travel as **both** the button bit and the `up_move` axis.
- `advance_one` is `step_in_place` and nothing else, proven per tick over a real
  321-command run rather than by reading the source.
- `FixedStep`'s books balance every frame:
  `ticks * tick_ms + carried_ms + dropped_ms == every millisecond fed in`.
- An uncapped accumulator never drops a millisecond, which separates "drops
  under stress" from "drops as a matter of course".

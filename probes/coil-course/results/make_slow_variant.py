#!/usr/bin/env python3
"""Make a deliberately slower variant of a recorded coil run, and prove it.

Why this exists
---------------
Criterion 6 wants a second session that *races a ghost and displays a split*.
Playing the same file twice races a ghost of itself and the split is exactly
zero — a true number, and weak evidence, because zero is also what a broken
comparison would print.

So the second session needs a run that finishes but is slower. The only honest
way to make one is to perturb the recorded command stream and then check what
the perturbation actually produced: a strafe-jumping bot's commands are absolute
view angles timed for a moving player, so inserting a stall changes everything
downstream and may well end the run in a wall. This script therefore does not
assume — it sweeps insertion points, replays each candidate through the shipped
binary, and reports which ones still cross the finish line.

Usage
-----
    python3 make_slow_variant.py <straf3-binary> <map> [--write <out.txt>]

With --write it emits the slowest candidate that still finishes.
"""

import argparse
import pathlib
import re
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE / "coil-run.txt"

# `run           5096 ms  (5.096 s, start 1234 ms, finish 6330 ms)`
FINISHED = re.compile(r"^\s*run\s+(\d+) ms\b")
UNFINISHED = re.compile(r"^\s*run\s+(not started|started at)")


def load(path):
    """Split the fixture into its header lines and its expanded commands."""
    header, cmds = [], []
    for raw in path.read_text().splitlines():
        line = raw.split("#")[0].strip()
        if line.startswith("cmd "):
            f = line.split()
            repeat = int(f[1])
            cmds.extend([" ".join(f[2:])] * repeat)
        elif line:
            header.append(line)
    return header, cmds


def render(header, cmds, note):
    out = [note, ""]
    out.extend(header)
    out.append("")
    out.append("# cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> <roll>")
    # Fold identical consecutive commands back into repeat counts, exactly as
    # the recorder writes them.
    i = 0
    while i < len(cmds):
        n = 1
        while i + n < len(cmds) and cmds[i + n] == cmds[i]:
            n += 1
        out.append(f"cmd {n} {cmds[i]}")
        i += n
    return "\n".join(out) + "\n"


def stall_like(cmd, count):
    """`count` commands of standing still, looking where `cmd` looked.

    The view angles are carried over so the player's camera does not jump: only
    the movement axes and the buttons are zeroed. A yaw discontinuity would be a
    second perturbation on top of the one being measured.
    """
    # `cmd` and the repeat count are already stripped by `load`, so the fields
    # here are: fwd right up buttons pitch yaw roll.
    f = cmd.split()
    pitch, yaw, roll = f[4], f[5], f[6]
    return [f"0 0 0 - {pitch} {yaw} {roll}"] * count


def nudge_yaw(window, degrees):
    """Turn the view `degrees` further over `window`, changing nothing else.

    Much gentler than a stall: the player keeps moving and keeps strafing, and
    only how much speed the turn gains changes. Written back with `repr`-style
    shortest round-tripping, because the parser quantises to a 16-bit angle and
    a truncated decimal would be a second, unintended perturbation.
    """
    out = []
    for cmd in window:
        f = cmd.split()
        f[5] = repr(float(f[5]) + degrees)
        out.append(" ".join(f))
    return out


def replay(binary, map_path, text):
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as fh:
        fh.write(text)
        path = fh.name
    try:
        proc = subprocess.run(
            [binary, "--replay", path, "--map", map_path],
            capture_output=True,
            text=True,
            timeout=120,
        )
    finally:
        pathlib.Path(path).unlink(missing_ok=True)
    for line in proc.stdout.splitlines():
        if m := FINISHED.match(line):
            return int(m.group(1))
        if UNFINISHED.match(line):
            return None
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("binary")
    ap.add_argument("map")
    ap.add_argument("--write")
    args = ap.parse_args()

    header, cmds = load(SOURCE)
    baseline = replay(args.binary, args.map, render(header, cmds, "# unmodified"))
    if baseline is None:
        sys.exit("the unmodified run does not finish — nothing here is meaningful")
    print(f"baseline: {baseline} ms over {len(cmds)} commands")

    candidates = []
    for at in range(0, len(cmds), 5):
        for count in (1, 2, 4, 8):
            candidates.append(
                (
                    f"stall {count} at {at}",
                    cmds[:at] + stall_like(cmds[at], count) + cmds[at:],
                )
            )
    # A far gentler perturbation than a stall: nudge the view angle over a short
    # window, which changes how much speed the strafe gains without ever taking
    # the player's hands off the controls. If a stall is too violent for this
    # course to survive, this is the next thing to try before concluding the
    # stream cannot be slowed at all.
    for at in range(0, len(cmds), 5):
        for degrees in (-1.0, -0.25, 0.25, 1.0):
            for width in (4, 16):
                candidates.append(
                    (
                        f"yaw {degrees:+} over {width} at {at}",
                        cmds[:at] + nudge_yaw(cmds[at : at + width], degrees) + cmds[at + width :],
                    )
                )

    finished, best = 0, None
    for label, doctored in candidates:
        ms = replay(args.binary, args.map, render(header, doctored, "# probe"))
        if ms is None:
            continue
        finished += 1
        delta = ms - baseline
        if delta != 0:
            print(f"  {label}: {ms} ms ({delta:+} ms)")
        if delta > 0 and (best is None or ms > best[0]):
            best = (ms, label, doctored)

    print(f"\n{finished} of {len(candidates)} perturbations still finished the course")
    if best is None:
        sys.exit(
            "no perturbation produced a slower run that still finishes.\n"
            "This bot stream is not robust to being slowed down: the commands are\n"
            "absolute view angles timed for a moving player, so a change anywhere\n"
            "after the start line ends the run rather than delaying it. A slower\n"
            "finishing run has to be *driven*, not doctored."
        )

    ms, label, doctored = best
    print(f"\nslowest finishing variant: {ms} ms (+{ms - baseline}) — {label}")

    if args.write:
        note = (
            f"# A DOCTORED copy of coil-run.txt, for criterion 6's ghost race.\n"
            f"#\n"
            f"# Made by probes/coil-course/results/make_slow_variant.py: {label}.\n"
            f"# Nothing else differs from the original.\n"
            f"#\n"
            f"# It exists because a session that plays coil-run.txt against a personal\n"
            f"# best set by coil-run.txt races a ghost of itself and the split is zero.\n"
            f"# Zero is a true number and weak evidence — it is also what a comparison\n"
            f"# that was not happening at all would print. This run finishes in\n"
            f"# {ms} ms against the original's {baseline} ms, so the split is\n"
            f"# +{ms - baseline} ms: visibly non-zero, and the right sign.\n"
            f"#\n"
            f"# The perturbation was not chosen by taste. The script sweeps candidates\n"
            f"# and replays every one, because these commands are absolute view angles\n"
            f"# timed for a moving player: most changes end the run in a wall rather\n"
            f"# than merely slowing it down.\n"
        )
        pathlib.Path(args.write).write_text(render(header, doctored, note))
        print(f"written to {args.write}")


if __name__ == "__main__":
    main()

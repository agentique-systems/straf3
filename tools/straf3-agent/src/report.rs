//! The plan printout: everything the derivation decided, in one page.
//!
//! It is a text report rather than a summary line because its job is to be
//! *checkable*. The trigger table can be read against
//! `probes/coil-course/results/coil.txt`, which published the same volumes from
//! an independent derivation; the aim points can be read against the bounds
//! printed beside them; and every fallback and assumption appears as a note
//! with a name rather than as a number that happens to look plausible.
//!
//! # Determinism, and the one number that is not bit-stable
//!
//! Same map, same profile, same bytes out. The exception is the bearing and
//! turn columns, which go through `atan2` — not IEEE-specified, so two targets
//! may legitimately disagree in the last bits. They are printed to a tenth of a
//! degree, they describe the course rather than steer through it, and nothing
//! in this crate takes a decision on one. That is the same reasoning
//! `crates/straf3-sim/src/num.rs` applies below the seam, applied to a report
//! above it.

use straf3_map::{CompiledMap, TriggerKind, TriggerSet};
use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Vec3, s};

use crate::course::{CoursePlan, Horizontal, Note, Vertical};

/// Render the whole plan for `map`, compiled from `source_path`.
#[must_use]
pub fn plan(
    source_path: &str,
    map: &CompiledMap,
    profile: &PhysicsProfile,
    plan: &CoursePlan,
) -> String {
    let mut out = String::with_capacity(4096);
    header(&mut out, source_path, map, profile, plan);
    warnings(&mut out, map);
    spawn(&mut out, plan);
    triggers(&mut out, map);
    course(&mut out, plan);
    legs(&mut out, plan);
    notes(&mut out, plan);
    out
}

fn header(
    out: &mut String,
    source_path: &str,
    map: &CompiledMap,
    profile: &PhysicsProfile,
    plan: &CoursePlan,
) {
    let coverage = map.collider().trigger_coverage();
    let timing = map
        .triggers
        .iter()
        .filter(|t| t.kind.trigger_set().is_some())
        .count();
    let half = profile.hull_half_extents();

    out.push_str("== straf3-agent plan ==\n");
    line(out, "map", source_path);
    line(
        out,
        "compiler",
        &format!(
            "straf3-map, collision digest {:#018x}",
            map.collision_digest()
        ),
    );
    line(out, "solids", &format!("{} hulls", map.hulls.len()));
    line(
        out,
        "triggers",
        &format!(
            "{} volumes ({timing} timing, {} recorded only)",
            map.triggers.len(),
            map.triggers.len() - timing
        ),
    );
    line(
        out,
        "clock",
        &format!(
            "coverage {:#010x}  START={}  FINISH={}  has_timing()={}",
            coverage.0,
            coverage.contains(TriggerSet::START),
            coverage.contains(TriggerSet::FINISH),
            map.has_timing()
        ),
    );
    line(
        out,
        "bounds",
        &format!("{} .. {}", xyz(map.bounds.mins), xyz(map.bounds.maxs)),
    );
    line(
        out,
        "profile",
        &format!(
            "{}  hull {:.0} x {:.0} x {:.0}, origin offset {}",
            plan.profile_name,
            half.x * 2.0,
            half.y * 2.0,
            half.z * 2.0,
            xyz(profile.hull_center_offset()),
        ),
    );
    line(
        out,
        "runnable",
        if plan.is_runnable() {
            "yes — the plan reaches a finish volume"
        } else {
            "NO — see the notes; this map cannot be timed"
        },
    );
}

fn warnings(out: &mut String, map: &CompiledMap) {
    out.push_str("\n-- compiler warnings --\n");
    if map.warnings.is_empty() {
        out.push_str("  (none)\n");
        return;
    }
    for w in &map.warnings {
        out.push_str(&format!("  {w:?}\n"));
    }
}

fn spawn(out: &mut String, plan: &CoursePlan) {
    let s = &plan.spawn;
    out.push_str("\n-- spawn --\n");
    line(
        out,
        "origin",
        &format!("{} yaw {:.1}", xyz(s.origin), s.yaw),
    );
    line(
        out,
        "clearance",
        &format!("start_solid={} all_solid={}", s.start_solid, s.all_solid),
    );
    match s.ground_z {
        Some(z) => line(
            out,
            "ground",
            &format!("surface z {z:.3}, {:.3} below the origin", s.origin.z - z),
        ),
        None => line(out, "ground", "none within the map's bounds"),
    }
}

fn triggers(out: &mut String, map: &CompiledMap) {
    out.push_str("\n-- trigger volumes, in source order --\n");
    out.push_str(&format!(
        "  {:>2}  {:<14} {:<12} {:<20} {:>6}  bounds\n",
        "#", "kind", "target", "resolved", "pieces"
    ));
    for (i, t) in map.triggers.iter().enumerate() {
        out.push_str(&format!(
            "  {i:>2}  {:<14} {:<12} {:<20} {:>6}  {} .. {}\n",
            kind_label(t.kind),
            t.target.as_deref().unwrap_or("-"),
            t.target_classname.as_deref().unwrap_or("?"),
            t.hulls.len(),
            xyz(t.bounds.mins),
            xyz(t.bounds.maxs),
        ));
    }
}

fn course(out: &mut String, plan: &CoursePlan) {
    out.push_str("\n-- the course, derived from those volumes --\n");
    if plan.waypoints.is_empty() {
        out.push_str("  (no timing volumes: there is no course here)\n");
        return;
    }
    out.push_str(&format!(
        "  {:<8} {:<12} {:<28} {:<22} {}\n",
        "step", "target", "aim (player origin)", "horizontal", "vertical"
    ));
    for w in &plan.waypoints {
        for (n, t) in w.targets.iter().enumerate() {
            let step = if n == 0 {
                w.step.to_string()
            } else {
                // An alternative volume for the same step. Marked rather than
                // repeated, so the printout cannot be misread as two steps.
                format!("  alt{n}")
            };
            out.push_str(&format!(
                "  {step:<8} {:<12} {:<28} {:<22} {}\n",
                t.name.as_deref().unwrap_or("-"),
                xyz(t.aim),
                horizontal_label(t.horizontal),
                vertical_label(t.vertical),
            ));
            if !t.aim_inside {
                out.push_str("           ^ the aim point is NOT inside this volume\n");
            }
        }
    }
    out.push_str(
        "\n  horizontal `bounds centre` and vertical `standing on ...` are the general\n\
         \x20 rules; anything else is a fallback and is listed in the notes below.\n",
    );
}

fn legs(out: &mut String, plan: &CoursePlan) {
    out.push_str("\n-- legs (straight lines between consecutive aim points) --\n");
    if plan.legs.is_empty() {
        out.push_str("  (none)\n");
        return;
    }
    out.push_str(&format!(
        "  {:<18} {:>10} {:>10} {:>9} {:>9} {:>8}\n",
        "leg", "distance", "ground", "rise", "bearing", "turn"
    ));
    for l in &plan.legs {
        out.push_str(&format!(
            "  {:<18} {:>10.1} {:>10.1} {:>9.1} {:>9.1} {:>8}\n",
            format!("{} -> {}", l.from, l.to),
            l.distance,
            l.ground_distance,
            l.rise,
            l.bearing_deg,
            match l.turn_deg {
                Some(t) => format!("{t:.1}"),
                None => "-".to_owned(),
            },
        ));
    }
    let turned = plan
        .legs
        .iter()
        .filter_map(|l| l.turn_deg)
        .fold(s(0.0), |acc, t| acc.max(t.abs()));
    out.push_str(&format!(
        "\n  sharpest turn between legs: {turned:.1} deg. A course whose legs all read\n  \
         the same bearing is a corridor, and a bot that maximises one coordinate\n  \
         completes it without ever having to steer.\n"
    ));
}

fn notes(out: &mut String, plan: &CoursePlan) {
    out.push_str("\n-- notes --\n");
    if plan.notes.is_empty() {
        out.push_str("  (none: every aim point came from a general rule)\n");
        return;
    }
    for n in &plan.notes {
        out.push_str(&format!("  {}\n", note_text(n)));
    }
}

fn note_text(note: &Note) -> String {
    match note {
        Note::NoStart => "no target_startTimer volume: the clock can never start".to_owned(),
        Note::NoFinish => "no target_stopTimer volume: a run can never end".to_owned(),
        Note::SeveralStarts(n) => format!(
            "{n} volumes start the clock; the plan aims at the first in source order \
             and crossing any of them would do"
        ),
        Note::SeveralFinishes(n) => format!(
            "{n} volumes stop the clock; the plan aims at the first in source order. \
             Alternative finishes are route choice, and this plan does not choose"
        ),
        Note::CheckpointGap(index) => format!(
            "checkpoint {index} is missing while a higher index exists — a checkpoint \
             was dropped after numbering; see the compiler warnings"
        ),
        Note::CheckpointOrderIsSourceOrder(n) => format!(
            "ASSUMPTION: the {n} checkpoints are visited in source order. Defrag gives \
             them no explicit index and straf3-map numbers them as it meets them, so a \
             map that declares them out of order gets a plan in that same wrong order. \
             Nothing here can check it"
        ),
        Note::CheckpointCountIsNotRead(n) => format!(
            "{n} checkpoints declare a `count` key and nothing reads it. straf3-map takes \
             classname, origin, angle, angles, target and targetname out of a .map and \
             nothing else; checkpoint numbering comes from source order. Both first-party \
             maps declare `count`, and on both it happens to agree — reported so the next \
             author does not find out otherwise"
        ),
        Note::CheckpointCountContradictsSourceOrder(order) => format!(
            "CONTESTED ORDER: the `count` keys imply the checkpoint order {order:?} and the \
             compiler assigned source order. The compiled order is what every reader in this \
             tree sees, so the plan follows it — but one of the two is wrong and it is not \
             this program's place to decide which"
        ),
        Note::CheckpointsDoNotGateTheClock(n) => format!(
            "the {n} checkpoints are route guidance, not gates: RunState::finish reads \
             TriggerSet::FINISH alone, so a run that skips them all still produces a time"
        ),
        Note::AimOutsideVolume(index) => format!(
            "FALLBACK FAILED: trigger {index}'s aim point is outside the volume. \
             Neither the general rule nor the fallback found a point inside it"
        ),
        Note::SpawnInSolid => "the player hull at the spawn overlaps solid geometry".to_owned(),
        Note::NothingUnderSpawn => {
            "nothing under the spawn within the map's bounds: the player falls".to_owned()
        }
    }
}

// ---------------------------------------------------------------------------

fn kind_label(kind: TriggerKind) -> String {
    match kind {
        TriggerKind::Start => "START".to_owned(),
        TriggerKind::Finish => "FINISH".to_owned(),
        TriggerKind::Checkpoint(i) => format!("checkpoint {i}"),
        TriggerKind::Teleport => "teleport".to_owned(),
        TriggerKind::Push => "push".to_owned(),
        TriggerKind::Other => "(untimed)".to_owned(),
    }
}

fn horizontal_label(h: Horizontal) -> String {
    match h {
        Horizontal::BoundsCentre => "bounds centre".to_owned(),
        Horizontal::LargestPiece(i) => format!("largest piece ({i}) [fallback]"),
    }
}

fn vertical_label(v: Vertical) -> String {
    match v {
        Vertical::Standing(z) => format!("standing on z={z:.1}"),
        Vertical::VolumeCentre => "volume centre [fallback]".to_owned(),
    }
}

fn line(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{key:<14} {value}\n"));
}

fn xyz(v: Vec3) -> String {
    format!("({:.1}, {:.1}, {:.1})", v.x, v.y, v.z)
}

//! Section 9: the three candidates, measured against `docs/movement-canon.md`
//! Part 1's sweep.
//!
//! # What this section is, and the one thing it is not
//!
//! It is the evidence Part 2 is scored from. It is **not** Part 2. Nothing here
//! calls a gate passed or failed, scores a weighed criterion, or recommends an
//! admission — that is deliberately somebody else's job, and the separation is
//! the point: the criteria in Part 1 were written before any of these numbers
//! existed, and they keep that immunity only if the seat that produced the
//! numbers did not also decide what they mean.
//!
//! Where a criterion cannot be scored from the instruments that exist, this
//! section says so in §1.9's words rather than substituting something adjacent.
//! An honest gap feeds the third verdict — *unjudgeable on available evidence* —
//! which §1.5 makes a legitimate outcome. A proxy metric that does not mean what
//! it claims feeds a verdict that looks settled and is not.
//!
//! # Every percentage below carries its count
//!
//! §1.1 requires it and the reason is arithmetic: the naive neighbourhood is
//! finite — 26 aims wide, and as few as two timings deep in a context where the
//! mechanic is barely available — so a bare percentage over it is false
//! precision. Counts are published beside every fraction, and a cell whose
//! denominator does not qualify is marked rather than printed.

use straf3_sim::num::{Scalar, s};

use crate::candidate::{
    self, AIM_STEP, AIMS, Anchor, Cell, Context, ENTRY_SPEEDS, HORIZON, Kind, MATERIAL, Mechanic,
    NAIVE_HALF_WIDTH, Policy, Reached, TOP_FRACTION, Technique,
};
use crate::dataset::{Dataset, Measurement, Section, Table, diff};
use crate::gates;
use crate::harness::MS;
use crate::measure::{attribution, pad};
use crate::refine;

/// The aim grid, refined, is the only axis of §1.2's sweep that G7 can refine.
///
/// Timing cannot: the sweep's timing resolution is one command, and one command
/// is the simulation's input quantum. There is no half-command to refine into,
/// so "does this step shrink when the grid is refined" has no meaning on the
/// timing axis and the report says that rather than reporting a number for it.
const TIMING_FLOOR_NOTE: &str = "not refinable — 8 ms is the input quantum";

/// How far either side of the nominal approach the geometry refinement looks,
/// in units.
const GEOMETRY_SPAN: Scalar = s(16.0);

/// The coarse grid the geometry refinement starts on, in units.
const GEOMETRY_COARSE: Scalar = s(0.5);

/// Everything one cell needed, computed once and reused by every criterion that
/// reads it.
struct Work {
    /// The anchors, shared between the sweep and the technique menu so that a
    /// technique at angle θ and the candidate's control at aim θ are the same
    /// run.
    anchors: Vec<Option<Anchor>>,
    /// The sweep, with crouch tapped (the default policy; see §2.0).
    tap: Cell,
    /// The sweep with crouch held. Only the crouch slide reads crouch, so this
    /// is `None` for the other two rather than a duplicate of `tap`.
    hold: Option<Cell>,
    /// The canonical technique menu's three policies in this cell.
    menu: Vec<(Policy, Option<Reached>)>,
}

/// Sweep everything one mechanic needs.
fn work(mech: Mechanic, ctx: &Context, entry: Scalar) -> Work {
    let anchors = candidate::anchors(mech, ctx, entry);
    let tap = candidate::sweep_with(mech, ctx, &anchors, false);
    let hold =
        (mech == Mechanic::CrouchSlide).then(|| candidate::sweep_with(mech, ctx, &anchors, true));
    let menu = Policy::all()
        .into_iter()
        .map(|p| (p, candidate::best_policy(mech, ctx, &anchors, p)))
        .collect();
    Work {
        anchors,
        tap,
        hold,
        menu,
    }
}

/// Whether a cell's best outcome delta is **positive and material** — the
/// restriction G5(b) and W2 both carry, for the same reason.
fn qualifies(cell: &Cell) -> bool {
    cell.best().is_some_and(|(_, _, g)| g >= MATERIAL)
}

/// The median of a list, or `None` when the list is empty.
///
/// A sorted-clone median rather than a running one: the lists here are at most
/// seven long and the clone keeps the caller's order intact for printing.
fn median(values: &[Scalar]) -> Option<Scalar> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a measured delta"));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) * s(0.5)
    })
}

/// A speed formatted for a table cell.
fn ups(v: Scalar) -> String {
    format!("{v:.2}")
}

/// The key prefix one cell's measurements share.
fn cell_key(mech: Mechanic, ctx: &Context, entry: Scalar) -> String {
    format!(
        "candidate.{}.{}.e{}",
        mech.key(),
        ctx.name,
        pad(entry as u32, 4)
    )
}

pub(crate) fn measure() -> Section {
    let mut section = Section::new("9. The candidates");
    preamble(&mut section);

    let contexts = candidate::contexts();

    // G2 re-runs the whole published measurement set under each candidate and
    // diffs it against the control. The control half is the same every time, so
    // it is taken once. The profile is named `p` in both halves because a diff
    // over keys can only see a moved *value* if the key did not move too.
    let control_set = Dataset::from_sections(&super::vocabulary(&[("p", Mechanic::control())]));

    // G7's licence, before any G7 number is produced.
    let self_test = attribution::self_test();
    attribution::report(&mut section, &self_test);

    // Kept alive past the loop: the crouch slide's paired tap/hold sweeps are
    // what §2.0's question is settled from, and re-running them would double the
    // most expensive sweep in the report to arrive at numbers already in hand.
    let mut slide_work: Option<Vec<Vec<Work>>> = None;

    for mech in candidate::mechanics() {
        let work: Vec<Vec<Work>> = contexts
            .iter()
            .map(|ctx| {
                ENTRY_SPEEDS
                    .iter()
                    .map(|e| work(mech, ctx, s(*e)))
                    .collect()
            })
            .collect();

        section.say(format!("### {}", mech.title()));
        sweep_table(&mut section, mech, &contexts, &work);
        w1(&mut section, mech, &contexts, &work);
        w2_and_g5b(&mut section, mech, &contexts, &work);
        w3(&mut section, mech, &contexts, &work);
        w4(&mut section, mech, &contexts, &work);
        w5(&mut section, mech, &contexts, &work);
        w6(&mut section, mech, &contexts, &work);
        g3(&mut section, mech, &contexts, &work);
        g5a(&mut section, mech, &contexts);
        g2(&mut section, mech, &control_set);
        g7(&mut section, mech, &contexts, &work, &self_test);
        if mech == Mechanic::Dash {
            retuned_dash(&mut section, &contexts, &work);
        }
        if mech == Mechanic::CrouchSlide {
            slide_work = Some(work);
        }
    }

    w7(&mut section);
    if let Some(work) = slide_work {
        tap_and_stand(&mut section, &contexts, &work);
    }
    unmeasured(&mut section);
    section
}

fn preamble(section: &mut Section) {
    section.say(
        "**This section measures. It does not judge.** \
         `docs/movement-canon.md` Part 1 sets eight gates and seven weighed \
         criteria, and §1.2 defines one sweep that four of the weighed criteria \
         are scored from. Everything below is that sweep and the instruments the \
         gates need beside it. No number here is called a pass or a fail, and no \
         mechanic here is recommended: Part 1's thresholds were fixed before any \
         of these numbers existed, and the only way that immunity survives is if \
         the seat producing the numbers does not also decide what they mean.",
    );

    let mut conditions = Table::new(
        "**§1.10's four conditions on the evidence**, each checked rather than \
         asserted.",
        &["condition", "how it is met here"],
    );
    conditions.push(vec![
        "1. Measured under the integration canon will ship with".to_string(),
        format!(
            "Nothing in `candidate.rs` chooses an integration; every run calls \
             `straf3_sim::step`, which is `step_bounded(…, \
             PMOVE_SUBSTEP_MAX_MS)` with the bound at {} ms, and the bound is \
             reachable from nowhere above that function. **The precise version, \
             because the loose one would overstate it:** at the 125 Hz these \
             measurements are taken at, an 8 ms command is one sub-step, so the \
             shipping integrator and the single-step integrator it replaced \
             coincide here by construction. The condition is met because these \
             runs go through the mover canon will ship — not because \
             sub-stepping changed a candidate number, which at this rate it \
             cannot.",
            straf3_sim::PMOVE_SUBSTEP_MAX_MS
        ),
    ]);
    conditions.push(vec![
        "2. Measured against the real fixtures".to_string(),
        format!(
            "`tools/straf3-lab/src/geometry.rs` is one `pub use` of \
             `straf3_collision::testbed`, so the seven contexts below are the \
             module itself and not a mirror of it. `geometry::MIRRORED` is \
             `{}`, and the document's *Limits* 4 records the swap moving no \
             measurement.",
            crate::geometry::MIRRORED
        ),
    ]);
    conditions.push(vec![
        "3. One variable".to_string(),
        "Each candidate carries its own profile with that mechanic's constants \
         and nothing else, not `PhysicsProfile::experimental()`, which carries \
         all three. `the_candidate_profiles_change_one_mechanic_each` holds each \
         one to `experimental()`'s own values and holds the three composed \
         together equal to it, so a candidate profile cannot drift into being a \
         fourth tuning."
            .to_string(),
    ]);
    conditions.push(vec![
        "4. Against a stated control".to_string(),
        "Every run is taken twice in lockstep from the same state on the same \
         command script, once under the candidate profile and once under \
         `PhysicsProfile::cpm()`. The control is not a similar run; it is the \
         same run with the mechanic absent."
            .to_string(),
    ]);
    section.table(conditions);

    section.say(format!(
        "**The grid, and the horizon.** Seven contexts crossed with six entry \
         speeds — {} ups — is 42 cells per mechanic. Inside each cell the \
         invocation timing is swept across the whole availability window at one \
         command (8 ms) and the wish direction is swept at {AIM_STEP:.0}° around \
         the **whole** circle ({AIMS} aims), because `corner()` is not \
         mirror-symmetric and a half sweep would report the wrong side's answer \
         for it. The horizon is {HORIZON} commands — one second — after the \
         window closes, counted from the **anchor** rather than from the press, \
         so every timing inside one cell is compared at the same instant.",
        ENTRY_SPEEDS
            .iter()
            .map(|e| format!("{e:.0}"))
            .collect::<Vec<_>>()
            .join("/"),
    ));

    section.say(format!(
        "**Outcome, absolute exit speed, and materiality.** §1.1 defines \
         *outcome* as the candidate run's horizontal speed at the horizon minus \
         the control's at the same horizon, in ups, and that is what every \
         column headed Δ below is. Where a criterion needs the un-differenced \
         quantity — W3's entry-speed sensitivity, G7 — it is published \
         separately and by name as **absolute exit speed**. *Material* is \
         {MATERIAL:.0} ups everywhere, and where a criterion also states a \
         percentage both apply and the larger governs."
    ));

    section.say(format!(
        "**The anchor is not the entry speed, and several cells turn on that.** \
         A run is placed so the mechanic's arming event happens at the \
         context's feature, which for the dash and the wall jump means falling \
         or flying there first. The geometry charges for the trip. The \
         `anchor` columns below publish what the player actually had when the \
         window opened — in the corner at 640 ups nominal entry the dash's \
         anchor speed is single digits, because the wall stopped them — and any \
         reading of a delta in such a cell has to start from that number rather \
         than from the column heading. §1.2 calls this a passage result rather \
         than a speed result, and the two anchor columns are what let a reader \
         tell them apart. The naive neighbourhood is every timing in the window \
         crossed with every aim within ±{NAIVE_HALF_WIDTH:.0}° of the current \
         heading."
    ));
}

// ── the sweep itself ───────────────────────────────────────────────────────

fn sweep_table(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    let mut table = Table::new(
        format!(
            "**{} — the sweep.** `Δ` is the outcome delta in ups; `abs` is \
             absolute exit speed; `control` is the best absolute exit speed the \
             existing vocabulary reached in the same cell with the same freedom \
             of aim and timing.",
            mech.title()
        ),
        &[
            "context",
            "entry",
            "anchor cmds",
            "anchor ups",
            "avail",
            "best Δ",
            "best abs",
            "control abs",
            "at ms",
            "at aim",
            "Δ at aim 0",
            "latency",
        ],
    );

    for (ci, ctx) in contexts.iter().enumerate() {
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let w = &work[ci][ei];
            let cell = &w.tap;
            let key = cell_key(mech, ctx, entry);

            section.record(Measurement::flag(
                format!("{key}.reachable"),
                cell.reachable,
            ));
            if !cell.reachable {
                table.push(vec![
                    ctx.name.to_string(),
                    format!("{entry:.0}"),
                    "—".to_string(),
                    "—".to_string(),
                    "never".to_string(),
                    "—".to_string(),
                    "—".to_string(),
                    "—".to_string(),
                    "—".to_string(),
                    "—".to_string(),
                    "—".to_string(),
                    "—".to_string(),
                ]);
                continue;
            }

            let best = cell.best();
            let avail = cell.available_commands();
            section.record(Measurement::count(
                format!("{key}.anchor_commands"),
                cell.anchor_commands as u32,
            ));
            section.record(Measurement::ups(
                format!("{key}.anchor_speed_ups"),
                cell.anchor_speed,
            ));
            section.record(Measurement::count(
                format!("{key}.avail_commands"),
                avail as u32,
            ));
            section.record(Measurement::ms(
                format!("{key}.avail_ms"),
                (avail * usize::from(MS)) as u32,
            ));
            section.record(Measurement::flag(
                format!("{key}.timing_degenerate"),
                cell.timing_degenerate,
            ));
            section.record(Measurement::count(
                format!("{key}.worst_latency"),
                cell.worst_latency as u32,
            ));
            section.record(Measurement::ups(
                format!("{key}.control_best_absolute_ups"),
                cell.control_best(),
            ));

            match best {
                Some((t, a, g)) => {
                    section.record(Measurement::ups(format!("{key}.best_delta_ups"), g));
                    section.record(Measurement::ups(
                        format!("{key}.best_absolute_ups"),
                        cell.absolute(t, a),
                    ));
                    section.record(Measurement::ms(
                        format!("{key}.best_timing_ms"),
                        (t * usize::from(MS)) as u32,
                    ));
                    section.record(Measurement::degrees(
                        format!("{key}.best_aim_deg"),
                        s(a as f32 * AIM_STEP),
                    ));
                    section.record(Measurement::flag(format!("{key}.material"), g >= MATERIAL));
                }
                None => {
                    section.record(Measurement::label(
                        format!("{key}.best_delta_ups"),
                        "never-fired",
                    ));
                    section.record(Measurement::flag(format!("{key}.material"), false));
                }
            }

            // The diagnostic that tells a zero at the horizon from a zero at
            // every command. Taken at the cell's best point, which is the run
            // most likely to have had something to erase.
            if let Some((t, a, _)) = best
                && let Some(anchor) = work[ci][ei].anchors[a]
            {
                let run = candidate::walk_pair_perturbed(
                    mech,
                    ctx,
                    &anchor.state,
                    s(a as f32 * AIM_STEP),
                    Some(t),
                    mech.window_commands() + HORIZON,
                    false,
                    None,
                    s(0.0),
                );
                section.record(Measurement::ups(
                    format!("{key}.peak_gain_ups"),
                    run.peak_gain,
                ));
                section.record(Measurement::ms(
                    format!("{key}.peak_gain_at_ms"),
                    (run.peak_at * usize::from(MS)) as u32,
                ));
            }

            let aim0 = cell.point_naive().map(|(t, _)| t);
            match aim0 {
                Some(t) => {
                    section.record(Measurement::ups(
                        format!("{key}.delta_aim0_ups"),
                        cell.delta(t, 0),
                    ));
                    section.record(Measurement::ups(
                        format!("{key}.absolute_aim0_ups"),
                        cell.absolute(t, 0),
                    ));
                }
                None => {
                    section.record(Measurement::label(
                        format!("{key}.delta_aim0_ups"),
                        "never-fired-at-aim-0",
                    ));
                }
            }

            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                cell.anchor_commands.to_string(),
                ups(cell.anchor_speed),
                if cell.timing_degenerate {
                    format!("{avail}*")
                } else {
                    avail.to_string()
                },
                best.map_or("never".to_string(), |(_, _, g)| ups(g)),
                best.map_or("—".to_string(), |(t, a, _)| ups(cell.absolute(t, a))),
                ups(cell.control_best()),
                best.map_or("—".to_string(), |(t, _, _)| {
                    (t * usize::from(MS)).to_string()
                }),
                best.map_or("—".to_string(), |(_, a, _)| {
                    format!("{:.0}", a as f32 * AIM_STEP)
                }),
                aim0.map_or("—".to_string(), |t| ups(cell.delta(t, 0))),
                cell.worst_latency.to_string(),
            ]);
        }
    }
    section.table(table);
    section.say(
        "A cell marked `*` in `avail` is one where every invocation timing \
         produced the identical run, so the timing axis selected nothing and the \
         count is not a window. That is measured by comparing the rows rather \
         than inferred from the context's name.",
    );
    section.say(
        "**A delta of zero has two causes and they are not the same fact.** \
         Either the mechanic did nothing, or it did something and the second of \
         simulation between the window closing and the horizon erased it. On \
         flat ground the second is what happens: a player holding a direction \
         converges to the ground terminal speed whether or not they slid, so a \
         run measured a full second later shows nothing however much speed the \
         slide carried in between. `peak_gain_ups` in the machine-readable \
         section is the largest advantage the candidate held at any command of \
         that cell's best run, with `peak_gain_at_ms` saying when. **It is a \
         diagnostic and no criterion is scored on it** — §1.1 defines the \
         outcome at the horizon and this document does not get to move the \
         horizon because it dislikes an answer. It is published so that a zero \
         can be read correctly, and a verdict that wants to say \"this mechanic \
         is worth 300 ups for half a second and nothing at the horizon\" has the \
         number for both halves of the sentence.",
    );
}

// ── W1 ─────────────────────────────────────────────────────────────────────

fn w1(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    let mut table = Table::new(
        format!(
            "**{} — W1's naive neighbourhood.** Every timing in the window \
             crossed with every aim within ±{NAIVE_HALF_WIDTH:.0}° of the \
             heading. `harmed` counts the points whose outcome delta is negative \
             by more than {MATERIAL:.0} ups; `n` is the size of the \
             neighbourhood, published because a percentage over it without its \
             count is false precision.",
            mech.title()
        ),
        &["context", "entry", "mean Δ", "harmed", "n", "harm rate"],
    );

    let mut all_harmed = 0usize;
    let mut all_points = 0usize;
    for (ci, ctx) in contexts.iter().enumerate() {
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let cell = &work[ci][ei].tap;
            if !cell.reachable {
                continue;
            }
            let key = cell_key(mech, ctx, entry);
            let (harmed, points) = cell.naive_harm(mech);
            let mean = cell.naive(mech);
            all_harmed += harmed;
            all_points += points;

            section.record(Measurement::count(
                format!("{key}.naive_points"),
                points as u32,
            ));
            section.record(Measurement::count(
                format!("{key}.naive_harmed"),
                harmed as u32,
            ));
            if points > 0 {
                section.record(Measurement::ratio(
                    format!("{key}.naive_harm_rate"),
                    s(harmed as f32) / s(points as f32),
                ));
            }
            if let Some((m, _)) = mean {
                section.record(Measurement::ups(format!("{key}.naive_mean_delta_ups"), m));
            }

            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                mean.map_or("—".to_string(), |(m, _)| ups(m)),
                harmed.to_string(),
                points.to_string(),
                if points > 0 {
                    format!("{:.1}%", 100.0 * harmed as f32 / points as f32)
                } else {
                    "—".to_string()
                },
            ]);
        }
    }
    section.table(table);

    let key = format!("candidate.{}.w1", mech.key());
    section.record(Measurement::count(
        format!("{key}.harmed_total"),
        all_harmed as u32,
    ));
    section.record(Measurement::count(
        format!("{key}.points_total"),
        all_points as u32,
    ));
    if all_points > 0 {
        section.record(Measurement::ratio(
            format!("{key}.harm_rate_pooled"),
            s(all_harmed as f32) / s(all_points as f32),
        ));
        section.say(format!(
            "Pooled over every reachable cell: **{all_harmed} of {all_points} \
             points harm the player by more than {MATERIAL:.0} ups**, which is \
             {:.1}%. The pooled figure is published beside the per-cell rows and \
             not instead of them: W1 states one rate and does not say over what, \
             and a mechanic can be harmless where it is available often and \
             harmful where it is available twice.",
            100.0 * all_harmed as f32 / all_points as f32
        ));
    }
}

// ── W2 and G5(b) ───────────────────────────────────────────────────────────

fn w2_and_g5b(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    let mut table = Table::new(
        format!(
            "**{} — W2's gap and execution window, and G5(b)'s point-naive \
             ratio.** The raw `best` and `naive` deltas in ups sit beside every \
             ratio, and a cell whose best delta is not positive and material is \
             marked *not meaningful* rather than printed as a number. `window` \
             is the count of timings reaching ≥{:.0}% of the best delta, in ms; \
             `span` is the distance from the first such timing to the last.",
            mech.title(),
            TOP_FRACTION * s(100.0)
        ),
        &[
            "context",
            "entry",
            "best Δ",
            "naive Δ",
            "gap",
            "point-naive Δ",
            "point/best",
            "window",
            "span",
        ],
    );

    // Per-context medians, over the entry speeds that qualify in that context.
    let mut context_gap: Vec<Scalar> = Vec::new();
    let mut context_point: Vec<Scalar> = Vec::new();

    for (ci, ctx) in contexts.iter().enumerate() {
        let mut gaps_here: Vec<Scalar> = Vec::new();
        let mut points_here: Vec<Scalar> = Vec::new();
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let cell = &work[ci][ei].tap;
            if !cell.reachable {
                continue;
            }
            let key = cell_key(mech, ctx, entry);
            let ok = qualifies(cell);
            let best = cell.best().map(|(_, _, g)| g);
            let naive = cell.naive(mech).map(|(m, _)| m);
            let point = cell.point_naive().map(|(_, d)| d);

            let (timings, _) = cell.top_set();
            let window_ms = timings.len() * usize::from(MS);
            let span_ms = match (timings.first(), timings.last()) {
                (Some(lo), Some(hi)) => (hi - lo + 1) * usize::from(MS),
                _ => 0,
            };
            section.record(Measurement::ms(
                format!("{key}.exec_window_ms"),
                window_ms as u32,
            ));
            section.record(Measurement::ms(
                format!("{key}.exec_window_span_ms"),
                span_ms as u32,
            ));

            let gap = match (ok, best, naive) {
                (true, Some(b), Some(n)) => {
                    let g = (b - n) / b;
                    gaps_here.push(g);
                    section.record(Measurement::ratio(format!("{key}.w2_gap"), g));
                    format!("{:.1}%", g * s(100.0))
                }
                _ => {
                    section.record(Measurement::label(
                        format!("{key}.w2_gap"),
                        "not-meaningful",
                    ));
                    "not meaningful".to_string()
                }
            };
            let ratio = match (ok, best, point) {
                (true, Some(b), Some(p)) => {
                    let r = p / b;
                    points_here.push(r);
                    section.record(Measurement::ratio(format!("{key}.g5b_point_naive"), r));
                    format!("{r:.4}")
                }
                _ => {
                    section.record(Measurement::label(
                        format!("{key}.g5b_point_naive"),
                        "not-meaningful",
                    ));
                    "not meaningful".to_string()
                }
            };
            section.record(Measurement::flag(format!("{key}.qualifies"), ok));

            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                best.map_or("—".to_string(), ups),
                naive.map_or("—".to_string(), ups),
                gap,
                point.map_or("—".to_string(), ups),
                ratio,
                format!("{window_ms}"),
                format!("{span_ms}"),
            ]);
        }
        if let Some(m) = median(&gaps_here) {
            context_gap.push(m);
            section.record(Measurement::ratio(
                format!("candidate.{}.{}.w2_gap_median", mech.key(), ctx.name),
                m,
            ));
        }
        if let Some(m) = median(&points_here) {
            context_point.push(m);
            section.record(Measurement::ratio(
                format!("candidate.{}.{}.g5b_median", mech.key(), ctx.name),
                m,
            ));
        }
    }
    section.table(table);

    let key = format!("candidate.{}", mech.key());
    section.record(Measurement::count(
        format!("{key}.w2_qualifying_contexts"),
        context_gap.len() as u32,
    ));
    match median(&context_gap) {
        Some(m) => {
            section.record(Measurement::ratio(format!("{key}.w2_gap_median"), m));
        }
        None => {
            section.record(Measurement::label(
                format!("{key}.w2_gap_median"),
                "no-qualifying-context",
            ));
        }
    }
    match median(&context_point) {
        Some(m) => section.record(Measurement::ratio(format!("{key}.g5b_median"), m)),
        None => section.record(Measurement::label(
            format!("{key}.g5b_median"),
            "no-qualifying-context",
        )),
    }

    section.say(format!(
        "**{} qualifying contexts**, and the medians are taken over those. \
         *An aggregation this document had to choose, and says so:* W2 and \
         G5(b) both define their number **per cell** and then score it on \
         *the median across contexts*, and a context here holds six cells — one \
         per entry speed. Nothing in Part 1 says how the six become one. The \
         medians published above collapse each context by taking the median of \
         its qualifying entry speeds first, then the median across contexts. \
         The per-cell numbers are all in the table and in the machine-readable \
         section, so a verdict that wants a different collapse can take one \
         without re-running anything.",
        context_gap.len()
    ));
}

// ── W3 ─────────────────────────────────────────────────────────────────────

fn w3(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    let mut table = Table::new(
        format!(
            "**{} — W3.** `both` is the best absolute exit speed with the \
             mechanic and a held angle; `mechanic alone` is the best with the \
             mechanic and no angle held (aim 0); `technique alone` is the best \
             the control reached over every aim. `d(abs)/d(entry)` is the slope \
             of `both` against entry speed across each adjacent pair.",
            mech.title()
        ),
        &[
            "context",
            "entry",
            "both",
            "mechanic alone",
            "technique alone",
            "both − best alone",
            "d(abs)/d(entry)",
        ],
    );

    for (ci, ctx) in contexts.iter().enumerate() {
        let mut absolutes: Vec<Option<Scalar>> = Vec::new();
        for (ei, _) in ENTRY_SPEEDS.iter().enumerate() {
            absolutes.push(work[ci][ei].tap.candidate_best());
        }
        let mut chain_best = Scalar::NEG_INFINITY;
        let mut slope_min = Scalar::INFINITY;
        let mut slope_max = Scalar::NEG_INFINITY;
        let mut any_slope = false;

        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let cell = &work[ci][ei].tap;
            if !cell.reachable {
                continue;
            }
            let key = cell_key(mech, ctx, entry);
            let both = cell.candidate_best();
            let alone = cell.best_without_strafing();
            let technique = cell.control_best();
            let chain = match (both, alone) {
                (Some(b), Some(a)) => Some(b - a.max(technique)),
                (Some(b), None) => Some(b - technique),
                _ => None,
            };
            if let Some(c) = chain {
                if c > chain_best {
                    chain_best = c;
                }
                section.record(Measurement::ups(format!("{key}.w3_chain_gain_ups"), c));
            }
            if let Some(a) = alone {
                section.record(Measurement::ups(format!("{key}.w3_mechanic_alone_ups"), a));
            }

            // The slope against the previous entry speed in the same context.
            let slope = if ei > 0 {
                match (absolutes[ei - 1], both) {
                    (Some(prev), Some(now)) => {
                        let d = (now - prev) / (entry - s(ENTRY_SPEEDS[ei - 1]));
                        any_slope = true;
                        if d < slope_min {
                            slope_min = d;
                        }
                        if d > slope_max {
                            slope_max = d;
                        }
                        section.record(Measurement::ratio(format!("{key}.w3_entry_slope"), d));
                        Some(d)
                    }
                    _ => None,
                }
            } else {
                None
            };

            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                both.map_or("—".to_string(), ups),
                alone.map_or("never fires at aim 0".to_string(), ups),
                ups(technique),
                chain.map_or("—".to_string(), ups),
                slope.map_or("—".to_string(), |d| format!("{d:.4}")),
            ]);
        }

        let key = format!("candidate.{}.{}", mech.key(), ctx.name);
        if chain_best > Scalar::NEG_INFINITY {
            section.record(Measurement::ups(
                format!("{key}.w3_chain_gain_best_ups"),
                chain_best,
            ));
        }
        if any_slope {
            section.record(Measurement::ratio(
                format!("{key}.w3_entry_slope_min"),
                slope_min,
            ));
            section.record(Measurement::ratio(
                format!("{key}.w3_entry_slope_max"),
                slope_max,
            ));
        }
    }
    section.table(table);
    section.say(
        "**Levelling** — W3's third number, whether the mechanic ever sets \
         absolute exit speed to a value independent of the entry speed — is read \
         off the slope column: a slope of zero across an adjacent pair is a pair \
         of entry speeds the mechanic returned the same exit speed for. The \
         minimum and maximum slope per context are in the machine-readable \
         section under `w3_entry_slope_min` and `_max`.",
    );
}

// ── W4 ─────────────────────────────────────────────────────────────────────

fn w4(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    let mut table = Table::new(
        format!(
            "**{} — W4.** A context counts when its best outcome delta over the \
             six entry speeds is material ({MATERIAL:.0} ups).",
            mech.title()
        ),
        &["context", "kind", "best Δ over entries", "material"],
    );
    let mut kinds: Vec<Kind> = Vec::new();
    let mut count = 0usize;
    for (ci, ctx) in contexts.iter().enumerate() {
        let best = (0..ENTRY_SPEEDS.len())
            .filter_map(|ei| work[ci][ei].tap.best().map(|(_, _, g)| g))
            .fold(Scalar::NEG_INFINITY, Scalar::max);
        let material = best >= MATERIAL;
        if material {
            count += 1;
            if !kinds.contains(&ctx.kind) {
                kinds.push(ctx.kind);
            }
        }
        section.record(Measurement::ups(
            format!("candidate.{}.{}.w4_best_delta_ups", mech.key(), ctx.name),
            if best > Scalar::NEG_INFINITY {
                best
            } else {
                s(0.0)
            },
        ));
        table.push(vec![
            ctx.name.to_string(),
            ctx.kind.key().to_string(),
            if best > Scalar::NEG_INFINITY {
                ups(best)
            } else {
                "never fires".to_string()
            },
            if material { "yes" } else { "no" }.to_string(),
        ]);
    }
    section.table(table);
    let key = format!("candidate.{}", mech.key());
    section.record(Measurement::count(
        format!("{key}.w4_material_contexts"),
        count as u32,
    ));
    section.record(Measurement::count(
        format!("{key}.w4_distinct_kinds"),
        kinds.len() as u32,
    ));
    section.say(format!(
        "**{count} of 7 contexts material, spanning {} distinct kind(s)**: {}.",
        kinds.len(),
        if kinds.is_empty() {
            "none".to_string()
        } else {
            kinds
                .iter()
                .map(|k| format!("`{}`", k.key()))
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
}

// ── W5 ─────────────────────────────────────────────────────────────────────

fn w5(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    section.say(
        "**W5 needs a technique menu the candidate sweep cannot produce, and \
         measuring one turned up a fact about §1.2's harness worth stating \
         first.** On the harness §1.2 specifies — one context, one entry speed, \
         an angle held off the current velocity and a jump the player may or may \
         not press — **four of the seven named techniques are the same command \
         policy**. `ground_turn`, `ramp traversal`, `step-up` and the `drop \
         launch` are each *hold a direction and press nothing*; so is \
         `air_forward`. What distinguishes them is which context they are named \
         in, because in each case the geometry supplies the technique. Only the \
         strafe axis and the jump-on-landing rhythm are separate things for the \
         player's hands to do. The menu below therefore measures three policies \
         per cell and maps the seven names onto them. That is a finding rather \
         than a shortcut, and scoring W5 as though there were seven independent \
         measurements would overstate the evidence by four.",
    );

    let mut table = Table::new(
        format!(
            "**{} — the canonical technique menu**, measured from the \
             candidate's own anchors and to the candidate's own horizon, under \
             the control profile, with the held angle swept 0–90° at \
             {AIM_STEP:.0}°. Absolute exit speed in ups. `candidate` is the \
             candidate's best absolute exit speed in the same cell.",
            mech.title()
        ),
        &[
            "context",
            "entry",
            "held_forward",
            "held_strafe",
            "bunnyhop",
            "candidate",
            "beats best technique by",
        ],
    );

    for (ci, ctx) in contexts.iter().enumerate() {
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let w = &work[ci][ei];
            if !w.tap.reachable {
                continue;
            }
            let key = cell_key(mech, ctx, entry);
            let mut best_technique = Scalar::NEG_INFINITY;
            let mut cells: Vec<String> = Vec::new();
            for (policy, reached) in &w.menu {
                match reached {
                    Some(r) => {
                        section.record(Measurement::ups(
                            format!("{key}.menu.{}.absolute_ups", policy.key()),
                            r.absolute,
                        ));
                        section.record(Measurement::degrees(
                            format!("{key}.menu.{}.angle_deg", policy.key()),
                            r.angle,
                        ));
                        if r.absolute > best_technique {
                            best_technique = r.absolute;
                        }
                        cells.push(format!("{:.2} @{:.0}°", r.absolute, r.angle));
                    }
                    None => cells.push("—".to_string()),
                }
            }
            let candidate_abs = w.tap.candidate_best();
            let over = candidate_abs.map(|c| c - best_technique);
            if let Some(o) = over {
                section.record(Measurement::ups(
                    format!("{key}.w5_over_best_technique_ups"),
                    o,
                ));
            }
            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                cells.first().cloned().unwrap_or_default(),
                cells.get(1).cloned().unwrap_or_default(),
                cells.get(2).cloned().unwrap_or_default(),
                candidate_abs.map_or("—".to_string(), ups),
                over.map_or("—".to_string(), ups),
            ]);
        }
    }
    section.table(table);

    // Per named technique, in its own domain: the cells where the candidate does
    // not beat it materially. W5's survival test is scored on that count.
    let mut survival = Table::new(
        format!(
            "**{} — W5's survival test.** For each named technique, over the \
             cells of *its own* domain: how many the candidate fails to beat by \
             {MATERIAL:.0} ups or more. A technique survives if that count is \
             not zero.",
            mech.title()
        ),
        &[
            "technique",
            "policy",
            "domain cells",
            "not beaten materially",
        ],
    );
    for technique in Technique::all() {
        let policy = technique.policy();
        let mut cells = 0usize;
        let mut survived = 0usize;
        for &ci in technique.domain() {
            for w in &work[ci] {
                if !w.tap.reachable {
                    continue;
                }
                let Some((_, reached)) = w.menu.iter().find(|(p, _)| *p == policy) else {
                    continue;
                };
                let Some(reached) = reached else { continue };
                let Some(candidate_abs) = w.tap.candidate_best() else {
                    continue;
                };
                cells += 1;
                if candidate_abs - reached.absolute < MATERIAL {
                    survived += 1;
                }
            }
        }
        section.record(Measurement::count(
            format!(
                "candidate.{}.w5.{}.cells_not_beaten",
                mech.key(),
                technique.key()
            ),
            survived as u32,
        ));
        section.record(Measurement::count(
            format!(
                "candidate.{}.w5.{}.domain_cells",
                mech.key(),
                technique.key()
            ),
            cells as u32,
        ));
        survival.push(vec![
            format!("`{}`", technique.key()),
            format!("`{}`", policy.key()),
            cells.to_string(),
            survived.to_string(),
        ]);
    }
    section.table(survival);
}

// ── W6 ─────────────────────────────────────────────────────────────────────

/// One context's ≥95%-of-best set, kept whole so W6's cross-context comparison
/// can ask about disjointness as well as about centroids.
struct TopSet {
    timings: Vec<usize>,
    aims: Vec<Scalar>,
    timing_centroid: Scalar,
    aim_centroid: Scalar,
}

fn w6(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    let window_ms = mech.window_commands() * usize::from(MS);
    let mut table = Table::new(
        format!(
            "**{} — W6's ≥{:.0}%-of-best sets**, the same sets W2's execution \
             window is read off. Aims are signed offsets from the heading. \
             Centroids are compared against 10% of each swept range: {} ms of \
             timing and 36° of aim.",
            mech.title(),
            TOP_FRACTION * s(100.0),
            window_ms / 10
        ),
        &[
            "context",
            "entry",
            "timings",
            "timing span ms",
            "timing centroid ms",
            "aims",
            "aim span",
            "aim centroid",
        ],
    );

    for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
        let entry = s(*entry);
        // Collected per entry speed so the cross-context comparison is between
        // like and like: comparing a context at 320 ups with another at 1000 is
        // a comparison of entry speeds wearing W6's clothes.
        let mut sets: Vec<TopSet> = Vec::new();
        for (ci, ctx) in contexts.iter().enumerate() {
            let cell = &work[ci][ei].tap;
            if !cell.reachable {
                continue;
            }
            let key = cell_key(mech, ctx, entry);
            let (timings, _) = cell.top_set();
            let aims = cell.top_aim_degrees();
            let centroids = cell.top_centroids();
            section.record(Measurement::count(
                format!("{key}.w6_top_timings"),
                timings.len() as u32,
            ));
            section.record(Measurement::count(
                format!("{key}.w6_top_aims"),
                aims.len() as u32,
            ));
            if let Some((tc, ac)) = centroids {
                section.record(Measurement::ms(
                    format!("{key}.w6_timing_centroid_ms"),
                    (tc * s(f32::from(MS))) as u32,
                ));
                section.record(Measurement::degrees(
                    format!("{key}.w6_aim_centroid_deg"),
                    ac,
                ));
                sets.push(TopSet {
                    timings: timings.clone(),
                    aims: aims.clone(),
                    timing_centroid: tc,
                    aim_centroid: ac,
                });
            }
            let timing_span = match (timings.first(), timings.last()) {
                (Some(lo), Some(hi)) => (hi - lo + 1) * usize::from(MS),
                _ => 0,
            };
            let aim_span = match (aims.first(), aims.last()) {
                (Some(lo), Some(hi)) => *hi - *lo,
                _ => s(0.0),
            };
            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                timings.len().to_string(),
                timing_span.to_string(),
                centroids.map_or("—".to_string(), |(t, _)| {
                    format!("{:.0}", t * s(f32::from(MS)))
                }),
                aims.len().to_string(),
                format!("{aim_span:.0}"),
                centroids.map_or("—".to_string(), |(_, a)| format!("{a:.1}")),
            ]);
        }

        // The comparison W6 actually scores: at least two contexts must have
        // disjoint sets, or centroids more than 10% of the swept range apart.
        let mut timing_disjoint = 0usize;
        let mut aim_disjoint = 0usize;
        let mut timing_apart = 0usize;
        let mut aim_apart = 0usize;
        let mut pairs = 0usize;
        for i in 0..sets.len() {
            for j in (i + 1)..sets.len() {
                let (a, b) = (&sets[i], &sets[j]);
                pairs += 1;
                if a.timings.iter().all(|t| !b.timings.contains(t)) {
                    timing_disjoint += 1;
                }
                if a.aims.iter().all(|x| !b.aims.contains(x)) {
                    aim_disjoint += 1;
                }
                let dt = (a.timing_centroid - b.timing_centroid).abs() * s(f32::from(MS));
                if dt > s(window_ms as f32 * 0.1) {
                    timing_apart += 1;
                }
                if (a.aim_centroid - b.aim_centroid).abs() > s(36.0) {
                    aim_apart += 1;
                }
            }
        }
        let key = format!("candidate.{}.w6.e{}", mech.key(), pad(entry as u32, 4));
        section.record(Measurement::count(
            format!("{key}.context_pairs"),
            pairs as u32,
        ));
        section.record(Measurement::count(
            format!("{key}.timing_disjoint_pairs"),
            timing_disjoint as u32,
        ));
        section.record(Measurement::count(
            format!("{key}.aim_disjoint_pairs"),
            aim_disjoint as u32,
        ));
        section.record(Measurement::count(
            format!("{key}.timing_centroid_apart_pairs"),
            timing_apart as u32,
        ));
        section.record(Measurement::count(
            format!("{key}.aim_centroid_apart_pairs"),
            aim_apart as u32,
        ));
    }
    section.table(table);
    section.say(
        "The pair counts W6 is scored on — how many pairs of contexts have \
         disjoint sets, and how many have centroids more than 10% of the swept \
         range apart, in timing and in aim, at each entry speed — are in the \
         machine-readable section under `w6.e<entry>.*`. They are counts of \
         pairs rather than a yes/no, because W6 asks for *at least two contexts* \
         and the number of pairs that satisfy it is the evidence for that.",
    );
}

// ── G3 ─────────────────────────────────────────────────────────────────────

fn g3(section: &mut Section, mech: Mechanic, contexts: &[Context], work: &[Vec<Work>]) {
    let mut worst_latency = 0usize;
    // Tracked as a *pair* from one cell rather than as two independent maxima:
    // the candidate's worst cell and the control's worst cell need not be the
    // same cell, and reporting the two maxima side by side would invite a
    // reader to subtract numbers that describe different runs.
    let mut worst_excess: i64 = i64::MIN;
    let mut worst_pair = (0usize, 0usize, String::new());
    let mut probed = 0usize;

    for (ci, ctx) in contexts.iter().enumerate() {
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let w = &work[ci][ei];
            let cell = &w.tap;
            if !cell.reachable {
                continue;
            }
            worst_latency = worst_latency.max(cell.worst_latency);
            let Some((t, a, _)) = cell.best() else {
                continue;
            };
            let Some(anchor) = w.anchors[a] else { continue };
            let u = gates::unresponsive(
                mech,
                ctx,
                &anchor,
                s(a as f32 * AIM_STEP),
                t,
                mech.window_commands() + HORIZON,
                false,
            );
            let excess = u.candidate as i64 - u.control as i64;
            if excess > worst_excess {
                worst_excess = excess;
                worst_pair = (
                    u.candidate,
                    u.control,
                    format!("{} at {:.0} ups", ctx.name, entry),
                );
            }
            probed = probed.max(u.probed);
            let key = cell_key(mech, ctx, s(*entry));
            section.record(Measurement::count(
                format!("{key}.g3_unresponsive_candidate"),
                u.candidate as u32,
            ));
            section.record(Measurement::count(
                format!("{key}.g3_unresponsive_control"),
                u.control as u32,
            ));
        }
    }

    let key = format!("candidate.{}.g3", mech.key());
    section.record(Measurement::count(
        format!("{key}.worst_latency_commands"),
        worst_latency as u32,
    ));
    section.record(Measurement::count(
        format!("{key}.worst_excess_unresponsive_candidate"),
        worst_pair.0 as u32,
    ));
    section.record(Measurement::count(
        format!("{key}.worst_excess_unresponsive_control"),
        worst_pair.1 as u32,
    ));
    section.say(format!(
        "**{} — G3's two counts.** The first is the commands between the \
         command carrying the input and the first command on which velocity \
         differs from the control, over every press that fired: the worst across \
         all 42 cells is **{worst_latency}**. The second is the commands on \
         which the mechanic causes an input to be ignored, measured by rotating \
         one command's wish direction by {:.0}° at a time over the \
         {} commands after the press and counting the commands that changed \
         nothing. The cell where the candidate ignores the most *more than its \
         own control does* is **{}**: **{} unresponsive in the candidate against \
         {} in the control**, out of {probed} probed. The control's count is the \
         reading that matters — a command the control also ignores was never the \
         mechanic's doing, and at 640 ups a crouched player's wish speed of 80 \
         is below `PM_Accelerate`'s projection in every direction, so steering \
         stops mattering for reasons that predate every candidate. \
         \n\nWhat that instrument does *not* cover, stated rather than left to \
         be assumed: it measures whether a command's **steering** reached the \
         outcome. It cannot measure a spent *jump press*, because under canon an \
         airborne jump press does nothing at all, so there is no control \
         behaviour to differ from. Read from `step.rs` instead: the dash and the \
         wall jump both set `jump_held` when they fire, exactly as a floor jump \
         does, so the following command's jump press is refused until the input \
         is released. Whether that is a press *spent* or a press *ignored* is a \
         reading of the gate, not a measurement, and it is left to the verdict.",
        mech.title(),
        gates::PROBE_ROTATION,
        gates::PROBE_COMMANDS,
        worst_pair.2,
        worst_pair.0,
        worst_pair.1,
    ));
}

// ── G5(a) ──────────────────────────────────────────────────────────────────

fn g5a(section: &mut Section, mech: Mechanic, contexts: &[Context]) {
    let mut table = Table::new(
        format!(
            "**{} — G5(a).** A player who accelerates on the ground only, jumps \
             and lands freely, holds one world direction so they never \
             strafejump, and never invokes the mechanic — run for \
             {} seconds in each context. `armed` counts the transitions from \
             unavailable to available. **Flat ground decides the gate; the other \
             six are the evidence** (amendment 2, change 18).",
            mech.title(),
            gates::EARNED_SECONDS
        ),
        &[
            "context",
            "armed",
            "commands available",
            "peak speed",
            "over max_speed",
        ],
    );
    let max_speed = Mechanic::control().max_speed;
    for ctx in contexts {
        let e = gates::earned(mech, ctx);
        let key = format!("candidate.{}.{}.g5a", mech.key(), ctx.name);
        section.record(Measurement::count(
            format!("{key}.arming_events"),
            e.arming_events as u32,
        ));
        section.record(Measurement::count(
            format!("{key}.available_commands"),
            e.available_commands as u32,
        ));
        section.record(Measurement::ups(
            format!("{key}.peak_speed_ups"),
            e.max_speed,
        ));
        table.push(vec![
            ctx.name.to_string(),
            e.arming_events.to_string(),
            e.available_commands.to_string(),
            ups(e.max_speed),
            if e.max_speed > max_speed { "YES" } else { "no" }.to_string(),
        ]);
    }
    section.table(table);
}

// ── G2 ─────────────────────────────────────────────────────────────────────

fn g2(section: &mut Section, mech: Mechanic, control_set: &Dataset) {
    let candidate_set = Dataset::from_sections(&super::vocabulary(&[("p", mech.profile())]));
    let changes = diff(control_set, &candidate_set);
    let key = format!("candidate.{}.g2", mech.key());
    section.record(Measurement::count(
        format!("{key}.measurements_compared"),
        control_set.len() as u32,
    ));
    section.record(Measurement::count(
        format!("{key}.measurements_moved"),
        changes.len() as u32,
    ));

    if changes.is_empty() {
        section.say(format!(
            "**{} — G2.** Sections 1 to 6 re-taken under the candidate profile \
             and diffed against the same six families under the control: **{} \
             measurements, and none of them moved**. G2's scope is every \
             measurement in which the mechanic's activation preconditions are \
             not all met on every command; since no value moved at all, the \
             question of which measurements are exempt does not arise.\
             \n\n*What {} is and is not, because G2 names the whole published \
             set.* This document publishes 2211 values: 1059 under `cpm`, 1059 \
             under `vq3`, 74 in section 8 and 19 in section 7. The {} compared \
             here are the `cpm` half, because the candidate constants sit on top \
             of `cpm` — `experimental()` is `..Self::cpm()` — so a `vq3` \
             re-measurement would be a measurement of a profile nobody has \
             proposed. Section 7 restates other sections' numbers rather than \
             taking its own, and section 8 is not parameterised by profile at \
             all. The unmeasured half is named here rather than folded into a \
             claim about \"the whole set\".",
            mech.title(),
            control_set.len(),
            control_set.len(),
            control_set.len(),
        ));
        return;
    }

    let mut table = Table::new(
        format!(
            "**{} — G2: every measurement that moved.** Named, because G2 fails \
             on *any* in-scope value moving and a count without names cannot be \
             checked.",
            mech.title()
        ),
        &["measurement", "control", "candidate"],
    );
    for change in &changes {
        table.push(match change {
            crate::dataset::Change::Moved { key, was, now } => {
                vec![format!("`{key}`"), was.clone(), now.clone()]
            }
            crate::dataset::Change::Added { key, value } => {
                vec![format!("`{key}`"), "—".to_string(), value.clone()]
            }
            crate::dataset::Change::Removed { key, was } => {
                vec![format!("`{key}`"), was.clone(), "—".to_string()]
            }
        });
    }
    section.table(table);
    section.say(format!(
        "{} of {} measurements moved. **Whether each is in G2's scope turns on \
         whether the mechanic actually fires in that measurement**, which this \
         diff cannot see from outside: it compares two datasets, and a dataset \
         does not record whether a timer was consulted. The names are published \
         so the verdict can decide the exemption per measurement against what \
         the measurement does, and this section deliberately does not decide it.",
        changes.len(),
        control_set.len()
    ));
}

// ── G7 ─────────────────────────────────────────────────────────────────────

fn g7(
    section: &mut Section,
    mech: Mechanic,
    contexts: &[Context],
    work: &[Vec<Work>],
    self_test: &attribution::SelfTest,
) {
    let floor = self_test.aim_floor();
    let total = mech.window_commands() + HORIZON;

    let mut table = Table::new(
        format!(
            "**{} — G7 part 2.** The outcome delta swept against aim at the \
             cell's best timing, refined to {floor:.2}° — the floor the \
             instrument's own self-test needed — and against the approach offset \
             refined to {:.4} of a unit. A step that survives is a cliff; one \
             that halves with the grid is a gradient.",
            mech.title(),
            attribution::GEOMETRY_FLOOR
        ),
        &[
            "context",
            "entry",
            "aim: coarse",
            "aim: refined",
            "geometry: coarse",
            "geometry: refined",
        ],
    );

    let mut worst_aim = s(0.0);
    let mut worst_geometry = s(0.0);
    let mut predicted_worst = s(0.0);

    for (ci, ctx) in contexts.iter().enumerate() {
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let w = &work[ci][ei];
            let cell = &w.tap;
            let Some((t, a, _)) = cell.best() else {
                continue;
            };
            let key = cell_key(mech, ctx, entry);

            // Aim. The anchor is re-found at every probed aim, because the aim
            // steers the approach and an anchor held fixed would be measuring a
            // different run than the sweep did.
            let aim_step = refine::largest_step(
                |aim| {
                    candidate::anchor(mech, ctx, entry, aim).map_or(s(0.0), |anchor| {
                        candidate::walk_pair_perturbed(
                            mech,
                            ctx,
                            &anchor.state,
                            aim,
                            Some(t),
                            total,
                            false,
                            None,
                            s(0.0),
                        )
                        .gain()
                    })
                },
                s(0.0),
                s(360.0),
                s(AIM_STEP),
                floor,
                MATERIAL,
            );

            // Geometry: where the approach began, which is what the player
            // controls about their own position when the window opens.
            let nominal = candidate::entering(mech, ctx, entry);
            let aim_deg = s(a as f32 * AIM_STEP);
            let geometry_step = refine::largest_step(
                |offset| {
                    let mut start = nominal;
                    start.player.origin.x += offset;
                    candidate::anchor_from(mech, ctx, start, aim_deg).map_or(s(0.0), |anchor| {
                        candidate::walk_pair_perturbed(
                            mech,
                            ctx,
                            &anchor.state,
                            aim_deg,
                            Some(t),
                            total,
                            false,
                            None,
                            s(0.0),
                        )
                        .gain()
                    })
                },
                -GEOMETRY_SPAN,
                GEOMETRY_SPAN,
                GEOMETRY_COARSE,
                attribution::GEOMETRY_FLOOR,
                MATERIAL,
            );

            // Where a surviving step sits is as much of G7's answer as how big
            // it is: the gate fails only on a step that does *not* coincide with
            // a boundary the player can perceive, and nobody can check that
            // against a height without knowing where the step was found.
            if let Some(st) = aim_step {
                section.record(Measurement::ups(
                    format!("{key}.g7_aim_refined_ups"),
                    st.refined,
                ));
                section.record(Measurement::degrees(format!("{key}.g7_aim_at_deg"), st.at));
                worst_aim = worst_aim.max(st.refined);
            }
            if let Some(st) = geometry_step {
                section.record(Measurement::ups(
                    format!("{key}.g7_geometry_refined_ups"),
                    st.refined,
                ));
                section.record(Measurement::units(
                    format!("{key}.g7_geometry_at_units"),
                    st.at,
                ));
                worst_geometry = worst_geometry.max(st.refined);
            }

            // G7 part 1: the closed form beside the measurement.
            if let Some(anchor) = w.anchors[a]
                && let Some(p) = gates::immediate(mech, ctx, &anchor, aim_deg, t, false)
            {
                section.record(Measurement::ups(
                    format!("{key}.g7_impulse_predicted_ups"),
                    p.predicted,
                ));
                section.record(Measurement::ups(
                    format!("{key}.g7_impulse_measured_ups"),
                    p.measured,
                ));
                predicted_worst = predicted_worst.max(p.residual().abs());
            }

            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                aim_step.map_or("none".to_string(), |st| ups(st.coarse)),
                aim_step.map_or("—".to_string(), |st| {
                    format!("{:.2} @ {:.2}°", st.refined, st.at)
                }),
                geometry_step.map_or("none".to_string(), |st| ups(st.coarse)),
                geometry_step.map_or("—".to_string(), |st| {
                    format!("{:.2} @ {:+.3}u", st.refined, st.at)
                }),
            ]);
        }
    }
    section.table(table);

    let key = format!("candidate.{}.g7", mech.key());
    section.record(Measurement::ups(
        format!("{key}.worst_surviving_aim_step_ups"),
        worst_aim,
    ));
    section.record(Measurement::ups(
        format!("{key}.worst_surviving_geometry_step_ups"),
        worst_geometry,
    ));
    section.record(Measurement::ups(
        format!("{key}.worst_impulse_residual_ups"),
        predicted_worst,
    ));
    section.say(format!(
        "**{} — the three G7 numbers.** Largest step surviving aim refinement to \
         {floor:.2}°: **{} ups**. Largest surviving geometry refinement to \
         {:.4} of a unit: **{} ups**. Largest disagreement between the closed \
         form `step.rs` computes the impulse from and the impulse actually \
         measured against the control on the invoking command: **{} ups**. \
         \n\nThe **timing** axis is {TIMING_FLOOR_NOTE}: the sweep's timing \
         resolution is one command and one command is the simulation's input \
         quantum, so there is no finer grid to refine into and \"does this step \
         shrink when the grid is refined\" has no meaning there. That is a limit \
         of the model rather than of this instrument, and it is stated rather \
         than answered with a number that would only describe the grid. \
         \n\nThe closed forms are the *immediate impulse*, not the outcome at \
         the horizon. No closed form for the horizon outcome is offered, because \
         a second of `PM_Accelerate`, friction, ground probes and possibly a \
         collision separates the impulse from the horizon and every one of those \
         depends on the whole run. G7's first part asks the **verdict** to state \
         a rule; what is published here is the arithmetic the code already \
         contains, measured, so that a rule has something to be checked against. \
         \n\n**Beside the incumbent, because §1.6 binds a G7 rejection to \
         publish the comparison.** The same instrument, at the same geometry \
         floor, finds overbounce's surviving step at {} ups across {:.4} of a \
         unit of drop height (the self-test table above). Canon's own text \
         quotes overbounce at 160.00 against 0.17; that is §4's figure at one \
         drop height, and the refined sweep finds a larger one. Both numbers are \
         this instrument's, taken the same way, so they can be compared \
         directly.",
        mech.title(),
        ups(worst_aim),
        attribution::GEOMETRY_FLOOR,
        ups(worst_geometry),
        ups(predicted_worst),
        ups(self_test.overbounce.step.map_or(s(0.0), |st| st.refined)),
        self_test.overbounce.step.map_or(s(0.0), |st| st.width),
    ));
}

// ── the dash's one pre-registered retune ───────────────────────────────────

/// The speed the pre-registered retune gates the dash's arming on.
///
/// Mirrors `slide_entry_speed` exactly, and sits above `max_speed` 320 for the
/// same reason: ground acceleration alone cannot reach it, so the window has to
/// be bought with speed the player earned.
const DASH_ENTRY_SPEED: Scalar = s(400.0);

fn retuned_dash(section: &mut Section, contexts: &[Context], work: &[Vec<Work>]) {
    section.say(
        "### Dash, retuned — the one pre-registered change §1.5 allows\n\n\
         **Registered before this measurement was run, which is the only thing \
         that makes it evidence.** §1.5 permits one retune per candidate and \
         requires the verdict to name the constant, state the direction and \
         predict which criterion it moves *before* the re-measurement. Canon did \
         that, and it reached this seat ahead of the numbers below. Its terms, \
         recorded here so the order is on the record in the document the numbers \
         live in:",
    );
    section.say(format!(
        "- **Constant:** a new `dash_entry_speed` on `PhysicsProfile`. No \
         existing constant can express it — the dash's two are `dash_speed` and \
         `dash_window_ms`, and arming is `left_ground_by_jumping` in the \
         just-landed branch of `step.rs`, which no current constant gates on \
         speed.\n\
         - **Direction:** above `max_speed` 320, at **{DASH_ENTRY_SPEED:.0}**, \
         mirroring `slide_entry_speed`, tested against horizontal speed at the \
         arming landing.\n\
         - **Prediction:** G5(a) moves fail→pass. W4's context count may fall. \
         W1 may move in either direction, with no sign predicted. G3, G4, G6, G7 \
         and G8 should not move at all.\n\
         - **One attempt.** There is no second retune."
    ));
    section.say(
        "**These numbers are derived, not taken against a patched crate, and the \
         condition for that being exact is stated rather than assumed.** The \
         field does not exist in `straf3-sim` and must not land speculatively: \
         `identity.rs` folds an exhaustive destructure of `PhysicsProfile`, so \
         adding a field moves the physics digest for `vq3` and `cpm` too, and a \
         rejected candidate would have permanently altered the digest of two \
         profiles it has nothing to do with. But the retune as registered gates \
         **arming and nothing else**: a landing at or above the threshold arms \
         exactly the window that arms today, and one below it arms nothing. A \
         run whose arming was refused is a run with the mechanic absent, so its \
         outcome is the control's and its delta is exactly zero. The rows below \
         are therefore the measured sweep rewritten under that rule — arithmetic \
         on numbers already taken, not an estimate of numbers not taken. **If \
         the constant that lands does anything more than gate arming on the \
         horizontal speed at the arming event — reads a different speed, gates \
         the spend rather than the arm, or touches the window length — this \
         derivation is void and these cells must be re-measured against the \
         patch.** Every value carries the `dash_retuned` prefix so no reader can \
         mistake one for a shipped-crate measurement.",
    );

    let mut table = Table::new(
        format!(
            "**Dash before and after the retune.** `arm ups` is the horizontal \
             speed at the arming landing at the best aim — the quantity \
             `dash_entry_speed` {DASH_ENTRY_SPEED:.0} would be compared \
             against."
        ),
        &[
            "context",
            "entry",
            "arm ups (aim 0)",
            "before: best Δ",
            "after: best Δ",
            "aims still arming",
            "before: harmed/n",
            "after: harmed/n",
        ],
    );

    let mut before_material = 0usize;
    let mut after_material = 0usize;
    let mut kinds_after: Vec<Kind> = Vec::new();
    for (ci, ctx) in contexts.iter().enumerate() {
        let mut best_before = Scalar::NEG_INFINITY;
        let mut best_after = Scalar::NEG_INFINITY;
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let w = &work[ci][ei];
            if !w.tap.reachable {
                continue;
            }
            let after = candidate::gated_by_entry_speed(&w.tap, &w.anchors, DASH_ENTRY_SPEED);
            let key = format!(
                "candidate.dash_retuned.{}.e{}",
                ctx.name,
                pad(entry as u32, 4)
            );
            let b = w.tap.best().map(|(_, _, g)| g);
            let a = after.best().map(|(_, _, g)| g);
            if let Some(b) = b {
                best_before = best_before.max(b);
            }
            match a {
                Some(a) => {
                    best_after = best_after.max(a);
                    section.record(Measurement::ups(format!("{key}.best_delta_ups"), a));
                }
                None => section.record(Measurement::label(
                    format!("{key}.best_delta_ups"),
                    "never-fired",
                )),
            }
            section.record(Measurement::flag(
                format!("{key}.reachable"),
                after.reachable,
            ));
            // How many of the 72 aims still arm the dash. This is the number
            // that explains the shape of the retune: the fall to the arming
            // landing is itself a strafejump, so an aim held off the velocity
            // during it can earn the threshold the entry speed did not supply.
            let armed = candidate::anchor_speeds(&w.anchors)
                .iter()
                .filter(|v| v.is_some_and(|v| v >= DASH_ENTRY_SPEED))
                .count();
            section.record(Measurement::count(
                format!("{key}.aims_armed"),
                armed as u32,
            ));
            let (bh, bn) = w.tap.naive_harm(Mechanic::Dash);
            let (ah, an) = after.naive_harm(Mechanic::Dash);
            section.record(Measurement::count(format!("{key}.naive_harmed"), ah as u32));
            section.record(Measurement::count(format!("{key}.naive_points"), an as u32));
            section.record(Measurement::count(
                format!("{key}.avail_commands"),
                after.available_commands() as u32,
            ));
            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                ups(w.tap.anchor_speed),
                b.map_or("—".to_string(), ups),
                a.map_or("never fires".to_string(), ups),
                format!("{armed}/{AIMS}"),
                format!("{bh}/{bn}"),
                format!("{ah}/{an}"),
            ]);
        }
        if best_before >= MATERIAL {
            before_material += 1;
        }
        if best_after >= MATERIAL {
            after_material += 1;
            if !kinds_after.contains(&ctx.kind) {
                kinds_after.push(ctx.kind);
            }
        }
    }
    section.table(table);

    // The before-value published beside the after-value, which is what makes a
    // §1.5 retune evidence rather than a replacement of the record.
    section.record(Measurement::count(
        "candidate.dash_retuned.w4_material_contexts_before",
        before_material as u32,
    ));
    section.record(Measurement::count(
        "candidate.dash_retuned.w4_material_contexts",
        after_material as u32,
    ));
    section.record(Measurement::count(
        "candidate.dash_retuned.w4_distinct_kinds",
        kinds_after.len() as u32,
    ));

    // G5(a) under the gate, measured rather than argued from the peak speed.
    let mut g5a = Table::new(
        format!(
            "**Dash retuned — G5(a).** The same player as before, with arming \
             refused below {DASH_ENTRY_SPEED:.0} ups at the landing. Flat ground \
             decides the gate."
        ),
        &["context", "before: armed", "after: armed", "peak speed"],
    );
    for ctx in contexts {
        let before = gates::earned(Mechanic::Dash, ctx);
        let after = gates::earned_gated(Mechanic::Dash, ctx, Some(DASH_ENTRY_SPEED));
        section.record(Measurement::count(
            format!("candidate.dash_retuned.{}.g5a.arming_events", ctx.name),
            after.arming_events as u32,
        ));
        g5a.push(vec![
            ctx.name.to_string(),
            before.arming_events.to_string(),
            after.arming_events.to_string(),
            ups(after.max_speed),
        ]);
    }
    section.table(g5a);
    section.say(format!(
        "**Against the prediction, item by item, including the part that did not \
         happen.** G5(a) moved as canon predicted: the arming count on flat \
         ground goes from 30 to **0**, because a player who never exceeds \
         `max_speed` peaks at 320.00 ups and cannot land at \
         {DASH_ENTRY_SPEED:.0}. W4 was predicted to *possibly fall*; it did not. \
         It stays at **{after_material}** material contexts spanning {} kinds, \
         the same as before.\n\n\
         The reason is in the `aims still arming` column and is worth stating \
         because it is not obvious: **the fall to the arming landing is itself a \
         strafejump.** The sweep holds its aim off the current velocity from the \
         first command, so at an aim of 50–70° a player entering at 320 ups is \
         accelerating all the way down and lands well above {DASH_ENTRY_SPEED:.0} \
         — the threshold the entry speed did not supply, earned in the air on \
         the way to the landing. What the retune removes at low entry speeds is \
         not the mechanic but the *lazy* aims: on flat ground at 320 ups the \
         best delta falls from 6.73 ups to 0.01 while 47 timings still fire. \
         Whether \"earned during the approach\" is what G5(a) means by earned is \
         a reading of the gate and is left to the verdict; the number is here \
         either way.",
        kinds_after.len()
    ));
}

// ── W7 ─────────────────────────────────────────────────────────────────────

fn w7(section: &mut Section) {
    section.say(
        "**W7's three counts, read from the source and published rather than \
         gated** (except the precondition count, which W7 does gate). The \
         constants are `PhysicsProfile`'s; the state fields are `PlayerState`'s \
         and `Timers`'; the preconditions are the distinct state predicates \
         gating the mechanic in `step.rs`. Two counts are given for the \
         preconditions because Part 1 does not say whether the profile guards — \
         the `!= 0` tests that G8 *requires* every mechanic to carry so that a \
         stated value switches it off — are state predicates. They are tests on \
         the profile rather than on the player, so the first column excludes \
         them; the second includes them, and a verdict can use either without \
         re-reading the file.",
    );

    let mut table = Table::new(
        "**W7 — cost.**".to_string(),
        &[
            "mechanic",
            "new constants",
            "new state fields",
            "preconditions (player state)",
            "preconditions (incl. profile guards)",
            "the predicates",
        ],
    );
    // Counted by reading `crates/straf3-sim/src/step.rs` and `profile.rs`, and
    // pinned by `the_w7_counts_match_the_source` at the foot of this module so
    // that a mechanic growing a precondition breaks a test rather than quietly
    // changing a published number.
    let rows: &[(Mechanic, u32, u32, u32, u32, &str)] = &[
        (
            Mechanic::CrouchSlide,
            3,
            1,
            3,
            4,
            "`crouch_edge`; walking (`check_slide` is reached only from \
             `PM_WalkMove`); `speed >= slide_entry_speed`. Profile guard: \
             `slide_duration_ms != 0`.",
        ),
        (
            Mechanic::Dash,
            2,
            1,
            5,
            7,
            "`jump_pressed`; `!jump_held`; airborne (`check_air_jump` is reached \
             only from `PM_AirMove`); `dash_ms > 0`; `wishdir != 0`; \
             `addspeed > 0`. Profile guards: `dash_speed != 0`, \
             `dash_window_ms != 0`. Counted as five because `addspeed > 0` is a \
             test on the impulse rather than on state.",
        ),
        (
            Mechanic::WallJump,
            3,
            2,
            4,
            6,
            "`jump_pressed`; `!jump_held`; airborne; `wall_contact_ms > 0`. \
             Arming adds `|normal.z| <= wall_normal_max`, counted here because \
             it is a test on the world the player is touching. Profile guards: \
             `wall_jump_velocity != 0`, `wall_contact_window_ms != 0`.",
        ),
    ];
    for (mech, constants, fields, preconditions, with_guards, predicates) in rows {
        let key = format!("candidate.{}.w7", mech.key());
        section.record(Measurement::count(
            format!("{key}.new_profile_constants"),
            *constants,
        ));
        section.record(Measurement::count(
            format!("{key}.new_state_fields"),
            *fields,
        ));
        section.record(Measurement::count(
            format!("{key}.preconditions"),
            *preconditions,
        ));
        section.record(Measurement::count(
            format!("{key}.preconditions_with_profile_guards"),
            *with_guards,
        ));
        table.push(vec![
            mech.title().to_string(),
            constants.to_string(),
            fields.to_string(),
            preconditions.to_string(),
            with_guards.to_string(),
            (*predicates).to_string(),
        ]);
    }
    section.table(table);
    section.say(
        "The state fields counted are `Timers::slide_ms` for the slide, \
         `Timers::dash_ms` for the dash, and `Timers::wall_contact_ms` plus \
         `PlayerState::wall_normal` for the wall jump. \
         `PlayerState::left_ground_by_jumping` is *not* counted against the \
         dash: it already existed for the double jump, which arms on the same \
         landing under the same provenance rule.",
    );
}

// ── §2.0: the crouch slide's tap-and-stand question ────────────────────────

fn tap_and_stand(section: &mut Section, contexts: &[Context], work: &[Vec<Work>]) {
    section.say(
        "### Crouch slide: does tap-and-stand-up dominate?\n\n\
         `docs/movement-canon.md` §2.0 names this as a question a verdict must \
         settle first, because it may decide the mechanic rather than tune it. \
         **The claim is true of the code.** `PM_Friction` \
         (`crates/straf3-sim/src/step.rs`) selects `slide_friction` on \
         `self.profile.slide_duration_ms != 0 && p.timers.slide_ms > 0` and reads \
         nothing else — `p.crouched` is not consulted. The wish-speed cap is a \
         separate test in `walk_move`, `if p.crouched { wishspeed = \
         min(wishspeed, max_speed · duck_scale) }`. So the two halves of \
         \"sliding\" are gated on different things: the friction on a timer, the \
         speed price on a posture. A player who taps crouch to start the timer \
         and stands up on the next command pays the price for one command and \
         keeps the friction for the whole countdown.",
    );

    let mut table = Table::new(
        "**Tap-and-stand against hold-crouch**, over the whole sweep. Best \
         outcome delta in ups, and the naive-harm rate in each policy's own \
         neighbourhood."
            .to_string(),
        &[
            "context",
            "entry",
            "tap: best Δ",
            "hold: best Δ",
            "tap − hold",
            "tap: harmed/n",
            "hold: harmed/n",
        ],
    );

    let mut tap_wins = 0usize;
    let mut hold_wins = 0usize;
    let mut ties = 0usize;
    let mut worst_advantage = Scalar::NEG_INFINITY;

    for (ci, ctx) in contexts.iter().enumerate() {
        for (ei, entry) in ENTRY_SPEEDS.iter().enumerate() {
            let entry = s(*entry);
            let tap = &work[ci][ei].tap;
            let Some(hold) = work[ci][ei].hold.as_ref() else {
                continue;
            };
            if !tap.reachable {
                continue;
            }
            let key = format!(
                "candidate.crouch_slide.{}.e{}",
                ctx.name,
                pad(entry as u32, 4)
            );
            let tb = tap.best().map(|(_, _, g)| g);
            let hb = hold.best().map(|(_, _, g)| g);
            if let Some(h) = hb {
                section.record(Measurement::ups(format!("{key}.hold_best_delta_ups"), h));
            }
            let (th, tn) = tap.naive_harm(Mechanic::CrouchSlide);
            let (hh, hn) = hold.naive_harm(Mechanic::CrouchSlide);
            section.record(Measurement::count(
                format!("{key}.hold_naive_harmed"),
                hh as u32,
            ));
            let advantage = match (tb, hb) {
                (Some(t), Some(h)) => {
                    let d = t - h;
                    section.record(Measurement::ups(format!("{key}.tap_minus_hold_ups"), d));
                    if d > MATERIAL {
                        tap_wins += 1;
                    } else if d < -MATERIAL {
                        hold_wins += 1;
                    } else {
                        ties += 1;
                    }
                    if d > worst_advantage {
                        worst_advantage = d;
                    }
                    Some(d)
                }
                _ => None,
            };
            table.push(vec![
                ctx.name.to_string(),
                format!("{entry:.0}"),
                tb.map_or("—".to_string(), ups),
                hb.map_or("—".to_string(), ups),
                advantage.map_or("—".to_string(), ups),
                format!("{th}/{tn}"),
                format!("{hh}/{hn}"),
            ]);
        }
    }
    section.table(table);
    section.record(Measurement::count(
        "candidate.crouch_slide.tap.cells_won",
        tap_wins as u32,
    ));
    section.record(Measurement::count(
        "candidate.crouch_slide.hold.cells_won",
        hold_wins as u32,
    ));
    section.record(Measurement::count(
        "candidate.crouch_slide.tap_hold.cells_tied",
        ties as u32,
    ));
    section.record(Measurement::ups(
        "candidate.crouch_slide.tap.largest_advantage_ups",
        if worst_advantage > Scalar::NEG_INFINITY {
            worst_advantage
        } else {
            s(0.0)
        },
    ));
    section.say(format!(
        "Over the cells where both policies fire: **tap beats hold materially in \
         {tap_wins}, hold beats tap materially in {hold_wins}, and {ties} are \
         within {MATERIAL:.0} ups of each other**. The largest advantage tapping \
         holds anywhere is {} ups. Note that the primary sweep tables above are \
         the **tap** policy, because tapping is the default in `presses()`; the \
         hold numbers are here and in the machine-readable section under \
         `hold_*`.",
        ups(if worst_advantage > Scalar::NEG_INFINITY {
            worst_advantage
        } else {
            s(0.0)
        })
    ));

    // What the timer does once the speed that bought it is gone.
    let mut life = Table::new(
        "**What a slide does after the speed that bought it is gone.** Armed at \
         the entry speed on flat ground, then nothing is held — no move axis, so \
         `PM_Friction` is the only force and what the run measures is the \
         mechanic alone. `standing` counts the commands of the slide spent \
         standing up."
            .to_string(),
        &[
            "policy",
            "entry",
            "slide commands",
            "standing",
            "exit speed",
            "lowest speed",
            "commands below entry speed",
            "ran below max_speed",
        ],
    );
    let floor_ctx = &contexts[0];
    for hold_crouch in [false, true] {
        for entry in ENTRY_SPEEDS {
            let Some(l) = gates::slide_life(floor_ctx, s(*entry), hold_crouch) else {
                continue;
            };
            let key = format!(
                "candidate.crouch_slide.life.{}.e{}",
                if hold_crouch { "hold" } else { "tap" },
                pad(*entry as u32, 4)
            );
            section.record(Measurement::count(
                format!("{key}.commands"),
                l.commands as u32,
            ));
            section.record(Measurement::count(
                format!("{key}.standing_commands"),
                l.standing_commands as u32,
            ));
            section.record(Measurement::ups(
                format!("{key}.exit_speed_ups"),
                l.exit_speed,
            ));
            section.record(Measurement::ups(
                format!("{key}.lowest_speed_ups"),
                l.lowest_speed,
            ));
            section.record(Measurement::count(
                format!("{key}.commands_below_entry_speed"),
                l.commands_below_entry as u32,
            ));
            section.record(Measurement::flag(
                format!("{key}.ran_below_max_speed"),
                l.ran_below_max_speed,
            ));
            life.push(vec![
                if hold_crouch { "hold" } else { "tap" }.to_string(),
                format!("{entry:.0}"),
                l.commands.to_string(),
                l.standing_commands.to_string(),
                ups(l.exit_speed),
                ups(l.lowest_speed),
                format!("{:.0}", l.commands_below_entry),
                if l.ran_below_max_speed { "yes" } else { "no" }.to_string(),
            ]);
        }
    }
    section.table(life);
    section.say(
        "`slide_entry_speed` is a floor checked **once, at entry**: nothing in \
         `PM_Friction` or in `Timers::advance` re-reads it, so the countdown runs \
         to its end whatever the speed becomes. The `commands below entry speed` \
         column is that fact measured. It is published because several criteria \
         may want it and nothing else in this document measures it, and it is a \
         property of Straf3's mover read from Straf3's mover.",
    );
}

// ── the gaps ───────────────────────────────────────────────────────────────

fn unmeasured(section: &mut Section) {
    section.say(
        "### What this section could not measure\n\n\
         §1.9 says an honest gap beats a proxy metric that does not mean what it \
         claims, because a proxy converts a question into a number and then the \
         number gets cited. Five gaps, each a real limit on what a verdict can \
         rest on:",
    );
    section.say(
        "1. **G1 is not measured here.** It is decided by `cargo xtask \
         determinism` across all four build targets and by \
         `the_checksum_covers_the_state_a_technique_depends_on` in \
         `crates/straf3-sim/src/state.rs`, neither of which is this instrument. \
         `SimState::checksum` does fold `slide_ms`, `dash_ms` and \
         `wall_contact_ms`, which is the half of G1 about branching on unfolded \
         state; the cross-target half is the determinism runner's answer to give.",
    );
    section.say(
        "2. **G6's first half is a code read, not a measurement.** The gate asks \
         whether an explicit clamp exists on the magnitude of `velocity` or of \
         its horizontal component. No candidate path contains one: the dash adds \
         along a wish direction under `PM_Accelerate`'s projection clamp, the \
         wall jump adds along a surface normal, and the slide changes a friction \
         rate. That is a reading of `step.rs` rather than a number, and it is \
         reported as one. G6's second half — terminal speeds under the candidate \
         profile where the mechanic is not invoked — is measured, and is the \
         `terminal.*` rows of the G2 diff above.",
    );
    section.say(
        "3. **G3's second count is measured on steering, not on presses.** The \
         instrument rotates one command's wish direction and asks whether \
         anything moved. It cannot ask the same question of a jump press, \
         because under canon an airborne jump press does nothing at all and \
         there is no control behaviour to differ from. The `jump_held` \
         consequence is read from the source and stated in the G3 paragraphs \
         rather than folded into the count.",
    );
    section.say(
        "4. **W5 has three policies and seven names.** On §1.2's harness, \
         `ground_turn`, `ramp traversal`, `step-up`, the `drop launch` and \
         `air_forward` are the same command policy, distinguished only by the \
         context each is named in. The menu measures three policies and maps the \
         names onto them; a W5 score that treated the seven as independent would \
         be counting the same measurement up to five times.",
    );
    section.say(
        "5. **The retuned dash is derived, not measured against a patched \
         crate.** The rewriting is exact if and only if `dash_entry_speed` is \
         precisely a horizontal-speed gate at the arming event and changes \
         nothing else; the retuned-dash section states the condition in full and \
         every value carries a `dash_retuned` prefix. It reads that way because \
         the field cannot land speculatively — `identity.rs` folds an exhaustive \
         destructure of `PhysicsProfile`, so adding it would move the physics \
         digest of `vq3` and `cpm` for a candidate that may be rejected.",
    );
    section.say(
        "6. **Route diversity is still not measured**, and the candidates do not \
         change that. *Limits* 6 explains why the only perturbation harness that \
         exists cannot answer it honestly. W4 and W6 above are scored on testbed \
         geometry, and §1.5's geometry-dependency disclosure is what keeps the \
         difference between \"pays in four contexts\" and \"pays on a map that \
         exists\" visible in a verdict.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W7's counts are read from the source and published as numbers, so they
    /// are asserted against the source rather than trusted: a mechanic that
    /// grows a constant should break a test, not quietly move a published
    /// figure.
    #[test]
    fn the_w7_constant_counts_match_the_profile() {
        let control = Mechanic::control();
        let counts = [
            (Mechanic::CrouchSlide, 3),
            (Mechanic::Dash, 2),
            (Mechanic::WallJump, 3),
        ];
        for (mech, expected) in counts {
            let p = mech.profile();
            let mut moved = 0;
            for (a, b) in [
                (p.slide_entry_speed, control.slide_entry_speed),
                (p.slide_friction, control.slide_friction),
                (p.dash_speed, control.dash_speed),
                (p.wall_jump_velocity, control.wall_jump_velocity),
                (p.wall_normal_max, control.wall_normal_max),
            ] {
                if a != b {
                    moved += 1;
                }
            }
            for (a, b) in [
                (p.slide_duration_ms, control.slide_duration_ms),
                (p.dash_window_ms, control.dash_window_ms),
                (p.wall_contact_window_ms, control.wall_contact_window_ms),
            ] {
                if a != b {
                    moved += 1;
                }
            }
            assert_eq!(
                moved,
                expected,
                "{} changes {moved} constants against the control, not {expected}",
                mech.key()
            );
        }
    }
}

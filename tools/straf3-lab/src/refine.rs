//! Telling a cliff from a gradient, by refining the grid until one of them
//! gives way.
//!
//! # The question this answers, and why the obvious version of it does not
//!
//! `docs/movement-canon.md`'s G7 asks whether a mechanic hides an **invisible
//! cliff**: a large change in outcome across a small change in input, sitting
//! somewhere the player has no way to see coming. The obvious way to look for
//! one is to sweep the input on a grid and report the largest step between
//! adjacent samples.
//!
//! That measures the grid. Take the lab's own §1 curve — `vq3` / `forward` at
//! 320 ups entry gains 97.71 ups/s at 40° and 177.82 at 50° — and a 5° grid
//! reports a step of about 40 ups across it. Nothing is discontinuous there;
//! the curve is smooth and the number is simply the slope multiplied by the
//! spacing. A rule that flags it rejects **strafejumping**, which is the game.
//!
//! # The rule that works instead
//!
//! > A discontinuity is a step that **does not shrink when the grid is
//! > refined**.
//!
//! Halve the interval around every step that exceeds the materiality threshold
//! and look at the two halves. A gradient's step halves with them — that is what
//! a derivative is. A cliff's step stays where it is, entirely inside one half,
//! and refining only pins down where. So [`largest_step`] descends into
//! whichever half carries the step and keeps going until the interval is
//! narrower than a stated floor; what is left is the step across an interval the
//! player could not have aimed inside anyway.
//!
//! A **kink** — a discontinuity in the slope rather than in the value, which is
//! what the wish-speed clamp opening looks like — behaves like a gradient here,
//! and correctly so: its step shrinks with the grid, just from a larger starting
//! slope. That is the case worth checking, and
//! [`crate::measure::attribution`]'s self-test checks it.
//!
//! # The floor is a parameter of the rule, not a constant
//!
//! Refinement has to stop somewhere, and where it stops decides the answer for
//! any smooth curve: a gradient of slope `k` reports `k · floor`. So the floor
//! is passed in, published beside the result, and chosen for the parameter — an
//! aim the player could not hold finer than, a timing below the command
//! quantum, a geometric offset below what a map can express. A result quoted
//! without its floor is not a result.

use straf3_sim::num::{Scalar, s};

/// One step in a swept curve, and what refinement did to it.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    /// Lower end of the coarse interval the step was first seen across.
    pub from: Scalar,
    /// Upper end of it.
    pub to: Scalar,
    /// How big the step was on the coarse grid.
    pub coarse: Scalar,
    /// How big it still is across the narrowest interval refinement reached.
    pub refined: Scalar,
    /// How wide that interval is. At or below the floor by construction.
    pub width: Scalar,
    /// Where in the parameter the surviving step sits.
    pub at: Scalar,
}

impl Step {
    /// Whether this step survived refinement at all: a gradient's does not,
    /// because halving the interval halves the step.
    ///
    /// The comparison is against the materiality threshold rather than against
    /// the coarse step, because "shrank a bit" is not the question. The question
    /// G7 asks is whether something the player cannot perceive still costs them
    /// a material amount of speed.
    #[must_use]
    pub fn survives(&self, material: Scalar) -> bool {
        self.refined > material
    }
}

/// Sweep `f` from `from` to `to` on a grid of `coarse`, and report the step that
/// best survives refinement down to `floor`.
///
/// Returns `None` when no coarse step exceeds `material` — there is nothing to
/// refine and therefore nothing that could be a cliff.
///
/// # Cost
///
/// One evaluation per coarse sample, plus one per bisection: `log2(coarse /
/// floor)` extra evaluations for each interval that exceeded `material`. Refining
/// 5° down to 1° is three.
pub fn largest_step<F>(
    mut f: F,
    from: Scalar,
    to: Scalar,
    coarse: Scalar,
    floor: Scalar,
    material: Scalar,
) -> Option<Step>
where
    F: FnMut(Scalar) -> Scalar,
{
    debug_assert!(coarse > floor, "refining to a floor coarser than the grid");
    debug_assert!(to > from, "an empty sweep has no steps");

    let mut best: Option<Step> = None;
    let mut a = from;
    let mut fa = f(a);
    while a < to {
        let b = (a + coarse).min(to);
        let fb = f(b);
        let jump = (fb - fa).abs();
        if jump > material {
            let step = bisect(&mut f, a, b, fa, fb, floor, jump, from, to);
            if best.is_none_or(|s| step.refined > s.refined) {
                best = Some(step);
            }
        }
        a = b;
        fa = fb;
    }
    best
}

/// Narrow `[a, b]` to the half carrying the step until the interval is no wider
/// than `floor`, then report the step across a window of **exactly** `floor`
/// centred on it.
///
/// # Why it widens back out at the end
///
/// Bisection overshoots: halving 5° down past a 1° floor lands on 0.625°, and a
/// gradient measured across 0.625° reports five-eighths of what the same
/// gradient reports across the floor. The answer would then depend on how many
/// halvings it happened to take to get under the floor, which is the grid
/// artefact this whole module exists to remove — just one level down.
///
/// So the last act is to measure across one floor exactly. Because the window is
/// centred on an interval already no wider than the floor, it contains that
/// interval whole: a cliff localised inside it is still inside the window, and
/// its full height is still reported.
#[allow(clippy::too_many_arguments)]
fn bisect<F>(
    f: &mut F,
    mut a: Scalar,
    mut b: Scalar,
    mut fa: Scalar,
    mut fb: Scalar,
    floor: Scalar,
    coarse: Scalar,
    limit_low: Scalar,
    limit_high: Scalar,
) -> Step
where
    F: FnMut(Scalar) -> Scalar,
{
    let (from, to) = (a, b);
    while b - a > floor {
        let m = (a + b) * s(0.5);
        // A midpoint that does not land strictly inside the interval means the
        // floor has gone below what the parameter's own representation can
        // express. Stopping is the honest answer; going round again would spin.
        if m <= a || m >= b {
            break;
        }
        let fm = f(m);
        if (fm - fa).abs() >= (fb - fm).abs() {
            b = m;
            fb = fm;
        } else {
            a = m;
            fa = fm;
        }
    }

    let centre = (a + b) * s(0.5);
    let half = floor * s(0.5);
    // Kept inside the swept range: a window that ran off the end would be asking
    // the curve about parameter values the sweep never claimed to cover.
    let low = (centre - half).max(limit_low);
    let high = (centre + half).min(limit_high);
    Step {
        from,
        to,
        coarse,
        refined: (f(high) - f(low)).abs(),
        width: high - low,
        at: centre,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight line is a gradient however steep, and refinement must say so:
    /// the surviving step is the slope times the floor, not the slope times the
    /// grid.
    #[test]
    fn a_gradient_shrinks_with_the_grid() {
        let slope = s(40.0);
        let step = largest_step(|x| x * slope, s(0.0), s(90.0), s(5.0), s(1.0), s(16.0))
            .expect("a 200 ups coarse step is well over the threshold");
        assert!(
            (step.coarse - slope * s(5.0)).abs() < s(0.01),
            "coarse step should be slope × grid, got {}",
            step.coarse
        );
        assert!(
            (step.refined - slope * s(1.0)).abs() < s(0.01),
            "refined step should be slope × floor, got {}",
            step.refined
        );
        // And at this slope it is 40 ups across one degree, which is a real
        // gradient a player feels — it just is not a cliff.
        assert!(step.survives(s(16.0)));
        assert!(!step.survives(s(64.0)));
    }

    /// A jump does not shrink. Refinement finds where it is and reports the same
    /// height however far down it goes.
    #[test]
    fn a_cliff_survives_every_refinement() {
        let edge = s(16.25);
        let step = largest_step(
            |x| if x < edge { s(160.0) } else { s(0.17) },
            s(15.0),
            s(18.0),
            s(0.5),
            s(0.0625),
            s(16.0),
        )
        .expect("a 160 ups drop is a step");
        assert!(
            (step.refined - s(159.83)).abs() < s(0.01),
            "the cliff should keep its whole height, got {}",
            step.refined
        );
        assert!(
            (step.width - s(0.0625)).abs() < s(1e-4),
            "the surviving step is measured across exactly one floor, got {}",
            step.width
        );
        assert!(step.at >= s(16.0) && step.at <= s(16.5), "found at {}", step.at);
        assert!(step.survives(s(16.0)));
    }

    /// A kink — the slope changing abruptly, which is what a clamp opening looks
    /// like — is a gradient, not a cliff, and the rule has to classify it that
    /// way or it rejects half the movement language.
    #[test]
    fn a_kink_is_a_gradient() {
        let step = largest_step(
            |x| if x < s(50.0) { s(0.0) } else { (x - s(50.0)) * s(14.0) },
            s(40.0),
            s(60.0),
            s(5.0),
            s(1.0),
            s(16.0),
        )
        .expect("70 ups across the 50-55 interval is over the threshold");
        // Slope 14 across a 1° floor is at most 14 ups — less where the window
        // straddles the kink and part of it sits on the flat side. Either way it
        // is below materiality, so there is no surviving discontinuity, which is
        // the correct answer for a kink.
        assert!(
            step.refined <= s(14.01),
            "a kink should refine to at most slope × floor, got {}",
            step.refined
        );
        assert!(!step.survives(s(16.0)));
    }

    /// A flat curve has nothing to refine, and saying so is different from
    /// saying the largest step was zero.
    #[test]
    fn a_curve_with_no_material_step_reports_nothing() {
        assert!(largest_step(|_| s(7.0), s(0.0), s(90.0), s(5.0), s(1.0), s(16.0)).is_none());
    }
}

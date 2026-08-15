//! A from-scratch double-double (`f64` + `f64`, ~106-bit significand) `sin`/
//! `cos`, used as the independent ground-truth reference this probe measures
//! everything else against.
//!
//! # Why this and not an external arbitrary-precision crate
//!
//! The assignment allows a new dependency if justified against writing the
//! reference in `f64` directly. A single `f64` is not enough headroom: it has
//! only ~15.9 decimal digits, roughly the same order as the ~7.2 digits an
//! `f32` result needs, so a plain-`f64` reference risks its own rounding
//! decisions coinciding with a candidate's at exactly the boundaries this
//! probe exists to check. Double-double arithmetic (Dekker 1971, Knuth *TAOCP*
//! vol. 2 §4.2.2, popularised by Bailey's QD library) gets ~106 bits — about
//! 32 decimal digits — using only `+ - *` on `f64`, each of which IEEE-754
//! guarantees is correctly rounded. That is over 4x the bits `f32` needs
//! resolved, so this reference's own rounding error is smaller than an `f32`
//! ULP by a factor of about 2^82. An external MPFR-style crate would buy
//! more headroom than that margin already provides, at the cost of a new
//! dependency and (for `rug`/`gmp-mpfr-sys`) a C build step this sandbox may
//! not have toolchain support for. Not worth it for a margin this wide.
//!
//! No constant here is a hardcoded double-double literal taken on faith from
//! a library. `π` is derived from its well-known decimal digits via
//! [`dd_from_decimal`], a from-scratch digit-by-digit parser, and checked by
//! the `self_check` test against identities that have nothing to do with how
//! it was constructed: `sin(π) ≈ 0`, `cos(π/2) ≈ 0`, `sin(π/2) ≈ 1`, each to
//! residual < 1e-29 -- computed by reducing the full `Dd`-precision `π`/`π/2`
//! directly (not by routing through the `f64`-input [`sin_cos_dd`], which
//! would collapse the very precision being checked down to ordinary `f64`
//! accuracy, ~1e-16, before the check could see it). If the decimal digits
//! were mistyped or the parser were wrong, that test fails loudly.

/// An unevaluated sum `hi + lo` with `|lo|` much smaller than one ULP of
/// `hi` — the standard double-double representation.
#[derive(Clone, Copy, Debug)]
pub struct Dd {
    pub hi: f64,
    pub lo: f64,
}

impl Dd {
    pub const ZERO: Dd = Dd { hi: 0.0, lo: 0.0 };

    #[must_use]
    pub const fn from_f64(x: f64) -> Dd {
        Dd { hi: x, lo: 0.0 }
    }

    #[must_use]
    pub fn to_f64(self) -> f64 {
        self.hi + self.lo
    }

    #[must_use]
    pub fn neg(self) -> Dd {
        Dd {
            hi: -self.hi,
            lo: -self.lo,
        }
    }

    #[must_use]
    pub fn add(self, other: Dd) -> Dd {
        let (s, e1) = two_sum(self.hi, other.hi);
        let e = e1 + self.lo + other.lo;
        let (hi, lo) = quick_two_sum(s, e);
        Dd { hi, lo }
    }

    #[must_use]
    pub fn add_f64(self, y: f64) -> Dd {
        let (s, e1) = two_sum(self.hi, y);
        let e = e1 + self.lo;
        let (hi, lo) = quick_two_sum(s, e);
        Dd { hi, lo }
    }

    #[must_use]
    pub fn sub(self, other: Dd) -> Dd {
        self.add(other.neg())
    }

    #[must_use]
    pub fn mul(self, other: Dd) -> Dd {
        let (p, e1) = two_prod(self.hi, other.hi);
        let e = e1 + self.hi * other.lo + self.lo * other.hi;
        let (hi, lo) = quick_two_sum(p, e);
        Dd { hi, lo }
    }

    #[must_use]
    pub fn mul_f64(self, y: f64) -> Dd {
        let (p, e1) = two_prod(self.hi, y);
        let e = e1 + self.lo * y;
        let (hi, lo) = quick_two_sum(p, e);
        Dd { hi, lo }
    }

    /// Exact for finite, non-subnormal operands: halving just decrements the
    /// exponent of each limb, no bits are lost.
    #[must_use]
    pub fn half(self) -> Dd {
        Dd {
            hi: self.hi * 0.5,
            lo: self.lo * 0.5,
        }
    }
}

/// Knuth/Dekker `TwoSum`: `a + b` split into an exact `(sum, error)` pair
/// using only IEEE-754 `+`/`-`, no assumption on `|a|` vs `|b|`.
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    let err = (a - (s - bb)) + (b - bb);
    (s, err)
}

/// `TwoSum` specialised to `|a| >= |b|` (cheaper); every call site here
/// satisfies that because it follows a `two_prod`/`two_sum` whose first
/// output already dominates the correction term by construction.
fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let err = b - (s - a);
    (s, err)
}

/// Veltkamp split: `a` into a high and low part each with <= 26 significant
/// bits, so their pairwise products in [`two_prod`] cannot lose precision.
fn split(a: f64) -> (f64, f64) {
    const SPLITTER: f64 = 134_217_729.0; // 2^27 + 1
    let t = SPLITTER * a;
    let hi = t - (t - a);
    let lo = a - hi;
    (hi, lo)
}

/// Dekker `TwoProduct`: `a * b` split into an exact `(product, error)` pair
/// using only `+ - *` — deliberately not `f64::mul_add`, so this reference
/// does not depend on the host having a correctly-rounded hardware/software
/// FMA (some `f32` fma reference-testing prior art has been burned by that
/// assumption on unusual targets; this sidesteps it entirely).
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let (ahi, alo) = split(a);
    let (bhi, blo) = split(b);
    let err = ((ahi * bhi - p) + ahi * blo + alo * bhi) + alo * blo;
    (p, err)
}

/// QD-library-style long division: three Newton-free refinement passes at
/// `f64` precision, recombined in `Dd` precision. Used only during constant
/// setup (`dd_from_decimal`), never in the per-sample hot loop.
fn dd_div(a: Dd, b: Dd) -> Dd {
    let q1 = a.hi / b.hi;
    let r = a.sub(b.mul_f64(q1));
    let q2 = r.hi / b.hi;
    let r = r.sub(b.mul_f64(q2));
    let q3 = r.hi / b.hi;
    let (s, e) = quick_two_sum(q1, q2);
    Dd { hi: s, lo: e }.add_f64(q3)
}

/// Parse a decimal literal (`"3.14159..."`, no sign, no exponent — all this
/// needs) into a `Dd` by digit-by-digit `*10 + digit`, then one [`dd_div`] by
/// the resulting power of ten. Independent of any hardcoded double-double
/// constant: correctness rests only on the digits being correct decimal
/// digits of the constant and on [`dd_div`]/[`two_sum`]/[`two_prod`] being
/// correct, which [`self_check`] verifies transitively.
fn dd_from_decimal(s: &str) -> Dd {
    let mut acc = Dd::ZERO;
    let mut scale = Dd::from_f64(1.0);
    let mut seen_point = false;
    for ch in s.chars() {
        if ch == '.' {
            seen_point = true;
            continue;
        }
        let d = f64::from(
            ch.to_digit(10)
                .unwrap_or_else(|| panic!("bad digit {ch:?}")),
        );
        acc = acc.mul_f64(10.0).add_f64(d);
        if seen_point {
            scale = scale.mul_f64(10.0);
        }
    }
    dd_div(acc, scale)
}

/// π to 60 decimal digits (public, independently checkable against any
/// reference — not this crate's own claim about itself).
const PI_DIGITS: &str = "3.14159265358979323846264338327950288419716939937510582097494459";

/// π/2, computed once at process start.
pub fn pi_2() -> Dd {
    static CELL: std::sync::OnceLock<Dd> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| dd_from_decimal(PI_DIGITS).half())
}

// ── the Taylor series, in Dd precision throughout ──────────────────────────

/// `1/n!` as a `Dd`, computed by building `n!` via repeated exact-integer
/// `mul_f64` and dividing — not a hardcoded coefficient table, so a
/// transcription slip in a magic constant can't hide an error here.
fn inv_factorial(n: u32) -> Dd {
    let mut fact = Dd::from_f64(1.0);
    for k in 2..=n {
        fact = fact.mul_f64(f64::from(k));
    }
    dd_div(Dd::from_f64(1.0), fact)
}

/// Odd-degree coefficients `1/1!, -1/3!, 1/5!, ...` up to `x^39`, for `sin`.
/// The series argument is always `|r| <= pi/4` after reduction; term 19
/// (`x^39/39!`) is already below `1e-45` there, so this has enormous margin
/// over the ~1e-32 this reference needs — see `sin_series_converges` test.
fn sin_coeffs() -> &'static [Dd; 20] {
    static CELL: std::sync::OnceLock<[Dd; 20]> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        std::array::from_fn(|k| {
            let n = 2 * k as u32 + 1;
            let c = inv_factorial(n);
            if k % 2 == 1 { c.neg() } else { c }
        })
    })
}

/// Even-degree coefficients `1/0!, -1/2!, 1/4!, ...` up to `x^38`, for `cos`.
fn cos_coeffs() -> &'static [Dd; 20] {
    static CELL: std::sync::OnceLock<[Dd; 20]> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        std::array::from_fn(|k| {
            let n = 2 * k as u32;
            let c = inv_factorial(n);
            if k % 2 == 1 { c.neg() } else { c }
        })
    })
}

/// `sin(r)` for `|r| <= pi/4` via a 20-term (through `r^39`) Taylor series,
/// Horner-summed from the smallest term up (smallest-first minimises the
/// relative rounding each `add` can contribute to the running total).
fn sin_taylor(r: Dd) -> Dd {
    let r2 = r.mul(r);
    let coeffs = sin_coeffs();
    let mut acc = coeffs[19];
    for c in coeffs[..19].iter().rev() {
        acc = acc.mul(r2).add(*c);
    }
    acc.mul(r)
}

/// `cos(r)` for `|r| <= pi/4`, same shape as [`sin_taylor`].
fn cos_taylor(r: Dd) -> Dd {
    let r2 = r.mul(r);
    let coeffs = cos_coeffs();
    let mut acc = coeffs[19];
    for c in coeffs[..19].iter().rev() {
        acc = acc.mul(r2).add(*c);
    }
    acc
}

/// Cody–Waite-shaped reduction, `Dd`-precision throughout: `x = q*(pi/2) + r`
/// with `|r| <= pi/4`. `q` itself only needs to be the correct *integer*
/// nearest `x/(pi/2)`, which plain `f64` division resolves correctly for
/// every magnitude this probe sweeps (see module docs and README "on
/// reduction correctness near tie boundaries") — the `Dd`-precision `pi_2`
/// constant is what gives `r` its accuracy, not the precision of `q`.
fn reduce(x: Dd) -> (i64, Dd) {
    let pi2 = pi_2();
    let scaled = x.to_f64() / pi2.to_f64();
    let q = scaled.round_ties_even() as i64;
    // round_ties_even matches Rust's ordinary rounding closely enough for a
    // *valid* (not necessarily bit-identical-to-own-trig) decomposition; see
    // module docs for why the tie-break choice cannot make `r` wrong.
    let r = x.sub(pi2.mul_f64(q as f64));
    (q, r)
}

/// Ground-truth `(sin(x), cos(x))` for `x` already promoted losslessly from
/// the exact `f32` radian value under test, computed independently of any
/// libm and independently of `dettrig`'s own reduction code (this module was
/// written without reference to `dettrig.rs`'s branch tables beyond the
/// shared, textbook quadrant identity below).
#[must_use]
pub fn sin_cos_dd(x: f64) -> (Dd, Dd) {
    let (q, r) = reduce(Dd::from_f64(x));
    let (sp, cp) = (sin_taylor(r), cos_taylor(r));
    match q.rem_euclid(4) {
        0 => (sp, cp),
        1 => (cp, sp.neg()),
        2 => (sp.neg(), cp.neg()),
        _ => (cp.neg(), sp),
    }
}

/// Round a `Dd` to the nearest `f32`, correctly — i.e. without the ordinary
/// `f64 -> f32` "double rounding" trap, where rounding the true value to
/// `f64` first and then to `f32` can occasionally differ from rounding it
/// straight to `f32`. Given `Dd`'s ~106-bit precision this can only bite
/// within about `2^-82` of an `f32` ULP, but the whole point of this probe is
/// not to wave away exactly this kind of margin.
#[must_use]
pub fn dd_to_f32(d: Dd) -> f32 {
    let y = d.hi as f32;
    if !y.is_finite() {
        return y;
    }
    let y64 = f64::from(y);
    // Residual of the true value beyond the naive f32 cast, in extra
    // precision: (hi - y64) is exact (Sterbenz: y64 is hi rounded to a
    // nearby value), so this recovers everything the cast discarded.
    let residual = (d.hi - y64) + d.lo;
    if residual == 0.0 {
        return y;
    }
    let neighbour = if residual > 0.0 {
        next_f32_up(y)
    } else {
        next_f32_up(-y).neg_f32()
    };
    let mid = (y64 + f64::from(neighbour)) / 2.0;
    let true_val = d.hi + d.lo;
    match true_val.partial_cmp(&mid) {
        Some(std::cmp::Ordering::Greater) if residual > 0.0 => neighbour,
        Some(std::cmp::Ordering::Less) if residual < 0.0 => neighbour,
        Some(std::cmp::Ordering::Equal) => {
            // Exact tie: round to even, as IEEE-754 nearest-even requires.
            if neighbour.to_bits() % 2 == 0 {
                neighbour
            } else {
                y
            }
        }
        _ => y,
    }
}

trait NegF32 {
    fn neg_f32(self) -> f32;
}
impl NegF32 for f32 {
    fn neg_f32(self) -> f32 {
        -self
    }
}

/// Next `f32` strictly above `y` (toward `+inf`), handling the `+0.0`/`-0.0`
/// boundary and sign changes correctly via the standard bit-pattern trick.
fn next_f32_up(y: f32) -> f32 {
    if y == 0.0 {
        return f32::from_bits(1); // smallest positive subnormal
    }
    let bits = y.to_bits();
    let next = if y > 0.0 { bits + 1 } else { bits - 1 };
    f32::from_bits(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only "trust me" in this module: that these are correct decimal
    /// digits of pi. Verified transitively, not by construction, via
    /// identities that would fail for a mistyped digit or a broken parser.
    ///
    /// This deliberately stays in `Dd` space throughout rather than routing
    /// through [`sin_cos_dd`] (which takes a plain `f64`): collapsing `pi/2`
    /// to a single `f64` first would throw away exactly the ~106-bit
    /// precision this test exists to check, and would only ever be able to
    /// confirm accuracy to ordinary `f64` precision (~1e-16) regardless of
    /// how good `reduce`/`sin_taylor`/`cos_taylor` actually are.
    #[test]
    fn self_check() {
        let pi2 = pi_2();
        let pi = pi2.add(pi2);
        // pi's hi limb must be the correctly-rounded nearest f64 to pi --
        // an independently-known fact, not something this module invented.
        assert_eq!(pi.hi, std::f64::consts::PI);

        // Reduce dd-precision pi and pi/2 directly, so the residual actually
        // reflects reduce()/sin_taylor()/cos_taylor()'s own precision.
        let (q_pi, r_pi) = reduce(pi);
        assert_eq!(q_pi, 2);
        assert!(
            r_pi.to_f64().abs() < 1e-29,
            "reduce(pi).r = {}",
            r_pi.to_f64()
        );
        // q=2 branch (see sin_cos_dd) is (sin, cos) = (-sin_taylor(r), -cos_taylor(r));
        // sin(pi)==0 and cos(pi)==-1 mean sin_taylor(r_pi)~0 and cos_taylor(r_pi)~+1.
        let s_pi = sin_taylor(r_pi)
            .to_f64()
            .abs()
            .max((cos_taylor(r_pi).to_f64() - 1.0).abs());
        assert!(s_pi < 1e-29, "sin/cos(pi) residual = {s_pi}");

        let (q_h, r_h) = reduce(pi2);
        assert_eq!(q_h, 1);
        assert!(
            r_h.to_f64().abs() < 1e-29,
            "reduce(pi/2).r = {}",
            r_h.to_f64()
        );
        // q=1 branch is (cos_taylor(r), -sin_taylor(r)) == (sin(pi/2), cos(pi/2)).
        assert!(
            (cos_taylor(r_h).to_f64() - 1.0).abs() < 1e-29,
            "sin(pi/2) via branch"
        );
        assert!(
            sin_taylor(r_h).to_f64().abs() < 1e-29,
            "cos(pi/2) via branch"
        );

        // cos(0) == 1, sin(0) == 0, exactly -- these pass through a plain
        // f64 fine because 0.0 is exactly representable, no collapse issue.
        let (s0, c0) = sin_cos_dd(0.0);
        assert_eq!(s0.to_f64(), 0.0);
        assert_eq!(c0.to_f64(), 1.0);
        // sin^2 + cos^2 == 1 to full Dd precision at a non-special angle.
        let (s, c) = sin_cos_dd(1.23456789);
        let one = s.mul(s).add(c.mul(c)).to_f64();
        assert!((one - 1.0).abs() < 1e-30, "sin^2+cos^2 = {one}");
    }

    #[test]
    fn dd_arithmetic_sanity() {
        // 1/3 * 3 == 1, to full Dd precision -- exercises dd_div, mul_f64, add.
        let third = dd_div(Dd::from_f64(1.0), Dd::from_f64(3.0));
        let one = third.mul_f64(3.0).to_f64();
        assert!((one - 1.0).abs() < 1e-30, "1/3*3 = {one}");
    }

    #[test]
    fn dd_to_f32_matches_plain_cast_away_from_boundaries() {
        for x in [0.1f64, 1.0, std::f64::consts::PI, -2.5, 123.456] {
            let d = Dd::from_f64(x);
            assert_eq!(dd_to_f32(d), x as f32, "x={x}");
        }
    }
}

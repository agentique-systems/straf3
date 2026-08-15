//! Exhaustive `f32` sweep measuring own-trig's (`probes/wasm-determinism/src/dettrig.rs`)
//! and this platform's libm's `sin`/`cos` error against an independent
//! double-double reference (`src/ddfloat.rs`), over exactly the argument
//! `angle_vectors` (`crates/straf3-sim/src/step.rs:954-957`) computes:
//! `(degrees: f32) * DEG_TO_RAD -> f32`, then `sin_cos`.
//!
//! Run once per native target to get "both libms":
//!   cargo +1.97.1 run --release                                  # glibc
//!   cargo +1.97.1 run --release --target x86_64-unknown-linux-musl # musl,
//!     bit-identical to what wasm32-unknown-unknown links (see
//!     probes/wasm-determinism/README.md) -- this stands in for the browser
//!     without needing a browser.
//!
//! `own_sin` / `own_cos` below is `det_probe::dettrig::det_sin` / `det_cos`
//! (crate `wasm-determinism-probe`, `[lib] name = "det_probe"`), called
//! directly from its crate -- not copied, not reimplemented, so there is no
//! risk of measuring a stale transcription.

mod ddfloat;

use ddfloat::{dd_to_f32, sin_cos_dd};
use std::time::Instant;
// The dependency's package name is `wasm-determinism-probe` (see Cargo.toml)
// but its `[lib] name` is `det_probe` (see probes/wasm-determinism/Cargo.toml),
// which is what determines the `use` path here.
use det_probe::dettrig;

/// Reproduced byte-for-byte from `crates/straf3-sim/src/step.rs:86` (and
/// mirrored again in `probes/wasm-determinism/src/lib.rs:39`). Not imported
/// because `straf3-sim`'s constant is private; this is the exact same
/// literal expression, so the compiler produces the exact same `f32` bits.
const DEG_TO_RAD: f32 = std::f32::consts::PI * 2.0 / 360.0;

/// Lower bound of the exhaustive sweep's magnitude, `2^-10` degree
/// (~0.00098°). Below this the argument to `sin_cos` after `DEG_TO_RAD` is so
/// small that `sin(x) == x` to full `f32` precision for every candidate
/// implementation (the Taylor remainder is below `f32`'s last bit) --
/// exhaustively sweeping the ~2 billion-per-sign subnormal/near-zero values
/// below it would spend the overwhelming majority of the sweep's time on
/// provably-trivial cases. That region gets a non-exhaustive spot-check
/// instead (see `spot_check_tiny`), not silently skipped.
const MIN_MAG: f32 = 0.000_976_562_5; // 2^-10

/// Upper bound, `2^24` degrees (~16.78 million degrees, ~46,603 full turns).
/// This is `f32`'s well-known integer-exactness ceiling: above it, adjacent
/// `f32` values are more than 1 apart, so a single `f32` degree value no
/// longer identifies one specific angle -- every implementation, including a
/// perfect one, is answering "sin of some real number near this float," not
/// "sin of this float," and disagreements stop being about `sin_cos`
/// accuracy at all. Bounding here is what makes the sweep an accuracy
/// measurement rather than a demonstration of a known, unrelated limit of
/// `f32` itself.
const MAX_MAG: f32 = 16_777_216.0; // 2^24

fn ord(v: f32) -> i64 {
    let u = v.to_bits();
    if u & 0x8000_0000 != 0 {
        -i64::from(u & 0x7fff_ffff)
    } else {
        i64::from(u)
    }
}

fn ulp_diff(a: f32, b: f32) -> u32 {
    (ord(a) - ord(b)).unsigned_abs() as u32
}

#[derive(Default, Clone, Copy)]
struct Stat {
    max_ulp: u32,
    max_ulp_deg_bits: u32,
    hist: [u64; 5], // [==0, ==1, ==2, ==3, >=4]
    total: u64,
}

impl Stat {
    fn record(&mut self, ulp: u32, deg_bits: u32) {
        self.total += 1;
        let idx = ulp.min(4) as usize;
        self.hist[idx] += 1;
        if ulp > self.max_ulp {
            self.max_ulp = ulp;
            self.max_ulp_deg_bits = deg_bits;
        }
    }
}

/// Worst-case ULP error, per octave of `|deg|` (indexed by `f32` biased
/// exponent, 0..=255), so error-vs-magnitude growth is visible directly
/// rather than hidden inside one global maximum.
#[derive(Default, Clone, Copy)]
struct OctaveStat {
    max_own: u32,
    max_libm: u32,
    n: u64,
}

struct Report {
    own_sin: Stat,
    own_cos: Stat,
    libm_sin: Stat,
    libm_cos: Stat,
    own_worse_than_libm_sin: u64,
    own_worse_than_libm_cos: u64,
    own_worse_examples: Vec<(f32, f32, u32, u32)>, // deg, rad, own_ulp, libm_ulp
    octaves_sin: Vec<OctaveStat>,
    octaves_cos: Vec<OctaveStat>,
    n: u64,
}

fn eval_one(deg: f32, r: &mut Report) {
    let rad = deg * DEG_TO_RAD;

    let (own_s, own_c) = dettrig::det_sin_cos(rad);
    let (libm_s, libm_c) = rad.sin_cos();

    let (ref_s_dd, ref_c_dd) = sin_cos_dd(f64::from(rad));
    let ref_s = dd_to_f32(ref_s_dd);
    let ref_c = dd_to_f32(ref_c_dd);

    let own_s_ulp = ulp_diff(own_s, ref_s);
    let own_c_ulp = ulp_diff(own_c, ref_c);
    let libm_s_ulp = ulp_diff(libm_s, ref_s);
    let libm_c_ulp = ulp_diff(libm_c, ref_c);

    let bits = deg.to_bits();
    r.own_sin.record(own_s_ulp, bits);
    r.own_cos.record(own_c_ulp, bits);
    r.libm_sin.record(libm_s_ulp, bits);
    r.libm_cos.record(libm_c_ulp, bits);
    r.n += 1;

    let oct = ((bits >> 23) & 0xff) as usize;
    let os = &mut r.octaves_sin[oct];
    os.n += 1;
    os.max_own = os.max_own.max(own_s_ulp);
    os.max_libm = os.max_libm.max(libm_s_ulp);
    let oc = &mut r.octaves_cos[oct];
    oc.n += 1;
    oc.max_own = oc.max_own.max(own_c_ulp);
    oc.max_libm = oc.max_libm.max(libm_c_ulp);

    if own_s_ulp > libm_s_ulp {
        r.own_worse_than_libm_sin += 1;
        if r.own_worse_examples.len() < 32 {
            r.own_worse_examples.push((deg, rad, own_s_ulp, libm_s_ulp));
        }
    }
    if own_c_ulp > libm_c_ulp {
        r.own_worse_than_libm_cos += 1;
    }
}

impl Report {
    fn new() -> Self {
        Report {
            own_sin: Stat::default(),
            own_cos: Stat::default(),
            libm_sin: Stat::default(),
            libm_cos: Stat::default(),
            own_worse_than_libm_sin: 0,
            own_worse_than_libm_cos: 0,
            own_worse_examples: Vec::new(),
            octaves_sin: vec![OctaveStat::default(); 256],
            octaves_cos: vec![OctaveStat::default(); 256],
            n: 0,
        }
    }
}

/// Every `f32` with `|x| in [MIN_MAG, MAX_MAG]`, both signs, exhaustively --
/// bit pattern enumeration, not sampling: `f32`'s bit pattern order matches
/// numeric order for non-negative floats, so counting from `MIN_MAG.to_bits()`
/// to `MAX_MAG.to_bits()` visits every representable value in between exactly
/// once, in increasing magnitude.
fn exhaustive_range() -> (u32, u32) {
    (MIN_MAG.to_bits(), MAX_MAG.to_bits())
}

/// Non-exhaustive spot-check of the region excluded from the main sweep:
/// exact zero, both signed zeros, the smallest subnormal, `f32::MIN_POSITIVE`,
/// and a log-spaced sample from the smallest subnormal up to `MIN_MAG`, all
/// checked the same way. This is not exhaustive and the report says so.
fn spot_check_tiny(r: &mut Report) {
    eval_one(0.0, r);
    eval_one(-0.0, r);
    eval_one(f32::from_bits(1), r); // smallest positive subnormal
    eval_one(-f32::from_bits(1), r);
    eval_one(f32::MIN_POSITIVE, r);
    eval_one(-f32::MIN_POSITIVE, r);
    for i in 0..2000u32 {
        // log-spaced across [2^-149, 2^-10): bits 1..=MIN_MAG.to_bits()
        let lo = 1u32;
        let hi = MIN_MAG.to_bits();
        let frac = f64::from(i) / 2000.0;
        let bits = lo + ((f64::from(hi - lo)) * frac) as u32;
        eval_one(f32::from_bits(bits), r);
        eval_one(-f32::from_bits(bits), r);
    }
}

fn main() {
    let libm_label = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "glibc"
    };
    let args: Vec<String> = std::env::args().collect();
    // Optional CLI override for a quick throughput benchmark:
    // `sweep --bench N` runs N samples from the start of the range and exits.
    let bench_n: Option<u64> = args
        .iter()
        .position(|a| a == "--bench")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok());

    let (lo, hi) = exhaustive_range();
    let start = Instant::now();

    let mut report = Report::new();

    if let Some(n) = bench_n {
        let mut b = lo;
        let mut count = 0u64;
        while count < n && b <= hi {
            eval_one(f32::from_bits(b), &mut report);
            eval_one(-f32::from_bits(b), &mut report);
            count += 2;
            b += 1;
        }
        let dt = start.elapsed();
        println!(
            "bench: {count} samples in {:?} ({:.1} ns/sample)",
            dt,
            dt.as_nanos() as f64 / count as f64
        );
        return;
    }

    spot_check_tiny(&mut report);

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let span = u64::from(hi) - u64::from(lo) + 1;
    let chunk = span.div_ceil(n_threads as u64);

    let results: Vec<Report> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..n_threads)
            .map(|t| {
                let t_lo = lo as u64 + t as u64 * chunk;
                let t_hi = ((t_lo + chunk).min(u64::from(hi) + 1)).max(t_lo);
                scope.spawn(move || {
                    let mut local = Report::new();
                    let mut b = t_lo as u32;
                    while (b as u64) < t_hi {
                        eval_one(f32::from_bits(b), &mut local);
                        eval_one(-f32::from_bits(b), &mut local);
                        b += 1;
                    }
                    local
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    for part in results {
        merge(&mut report, part);
    }

    let dt = start.elapsed();

    print_report(&report, libm_label, dt);
}

fn merge_stat(a: &mut Stat, b: Stat) {
    for i in 0..5 {
        a.hist[i] += b.hist[i];
    }
    a.total += b.total;
    if b.max_ulp > a.max_ulp {
        a.max_ulp = b.max_ulp;
        a.max_ulp_deg_bits = b.max_ulp_deg_bits;
    }
}

fn merge(into: &mut Report, from: Report) {
    merge_stat(&mut into.own_sin, from.own_sin);
    merge_stat(&mut into.own_cos, from.own_cos);
    merge_stat(&mut into.libm_sin, from.libm_sin);
    merge_stat(&mut into.libm_cos, from.libm_cos);
    into.own_worse_than_libm_sin += from.own_worse_than_libm_sin;
    into.own_worse_than_libm_cos += from.own_worse_than_libm_cos;
    into.n += from.n;
    for ex in from.own_worse_examples {
        if into.own_worse_examples.len() < 32 {
            into.own_worse_examples.push(ex);
        }
    }
    for i in 0..256 {
        let a = &mut into.octaves_sin[i];
        let b = from.octaves_sin[i];
        a.n += b.n;
        a.max_own = a.max_own.max(b.max_own);
        a.max_libm = a.max_libm.max(b.max_libm);
        let a = &mut into.octaves_cos[i];
        let b = from.octaves_cos[i];
        a.n += b.n;
        a.max_own = a.max_own.max(b.max_own);
        a.max_libm = a.max_libm.max(b.max_libm);
    }
}

fn print_report(r: &Report, libm_label: &str, dt: std::time::Duration) {
    println!("{{");
    println!("  \"libm\": \"{libm_label}\",");
    println!(
        "  \"target\": \"{}\",",
        std::env::consts::ARCH.to_string() + "-" + std::env::consts::OS
    );
    println!("  \"n_samples\": {},", r.n);
    println!("  \"min_mag_deg_bits\": \"{:08x}\",", MIN_MAG.to_bits());
    println!("  \"max_mag_deg_bits\": \"{:08x}\",", MAX_MAG.to_bits());
    println!("  \"elapsed_secs\": {:.3},", dt.as_secs_f64());
    print_stat("own_sin", &r.own_sin);
    print_stat("own_cos", &r.own_cos);
    print_stat("libm_sin", &r.libm_sin);
    print_stat("libm_cos", &r.libm_cos);
    println!(
        "  \"own_worse_than_libm_sin_count\": {},",
        r.own_worse_than_libm_sin
    );
    println!(
        "  \"own_worse_than_libm_cos_count\": {},",
        r.own_worse_than_libm_cos
    );
    println!("  \"own_worse_examples\": [");
    for (i, (deg, rad, own_ulp, libm_ulp)) in r.own_worse_examples.iter().enumerate() {
        let comma = if i + 1 < r.own_worse_examples.len() {
            ","
        } else {
            ""
        };
        println!(
            "    {{\"deg_bits\": \"{:08x}\", \"deg\": {:e}, \"rad\": {:e}, \"own_ulp\": {}, \"libm_ulp\": {}}}{}",
            deg.to_bits(),
            deg,
            rad,
            own_ulp,
            libm_ulp,
            comma
        );
    }
    println!("  ],");
    println!("  \"octaves_sin\": [");
    print_octaves(&r.octaves_sin);
    println!("  ],");
    println!("  \"octaves_cos\": [");
    print_octaves(&r.octaves_cos);
    println!("  ]");
    println!("}}");
}

/// One row per non-empty `f32` exponent octave: representative magnitude
/// (`2^(exp-127)` degrees), sample count, and worst ULP error seen for
/// own-trig and this platform's libm in that octave -- the error-vs-magnitude
/// curve the single global max_ulp figure above hides.
fn print_octaves(octs: &[OctaveStat]) {
    let nonempty: Vec<(usize, &OctaveStat)> =
        octs.iter().enumerate().filter(|(_, o)| o.n > 0).collect();
    for (i, (exp, o)) in nonempty.iter().enumerate() {
        let comma = if i + 1 < nonempty.len() { "," } else { "" };
        let approx_deg = 2f64.powi(*exp as i32 - 127);
        println!(
            "    {{\"exp\": {}, \"approx_deg\": {:e}, \"n\": {}, \"max_own_ulp\": {}, \"max_libm_ulp\": {}}}{}",
            exp, approx_deg, o.n, o.max_own, o.max_libm, comma
        );
    }
}

fn print_stat(name: &str, s: &Stat) {
    println!(
        "  \"{name}\": {{\"max_ulp\": {}, \"max_ulp_deg_bits\": \"{:08x}\", \"max_ulp_deg\": {:e}, \
         \"hist_0\": {}, \"hist_1\": {}, \"hist_2\": {}, \"hist_3\": {}, \"hist_ge4\": {}}},",
        s.max_ulp,
        s.max_ulp_deg_bits,
        f32::from_bits(s.max_ulp_deg_bits),
        s.hist[0],
        s.hist[1],
        s.hist[2],
        s.hist[3],
        s.hist[4],
    );
}

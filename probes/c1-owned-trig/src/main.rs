//! Verifies the landed C1 implementation against the work that designed it.
//!
//! Two questions, both answered exhaustively rather than by sampling:
//!
//! 1. **Is `straf3_sim::num::sin_cos` bit-identical to the probe's
//!    `dettrig::det_sin_cos`?** This is the one that matters most, and it is
//!    why this program exists rather than a fresh accuracy study. The
//!    reference was measured in `probes/dettrig-accuracy/` against a
//!    from-scratch double-double `sin`/`cos` over 570,429,352 samples, and
//!    those figures transfer to the landed code *only* if the landed code
//!    returns the identical bits. Anything short of "zero differing samples"
//!    means the accuracy report next door describes a function that is no
//!    longer in the tree, and would have to be redone.
//!
//! 2. **How far is it from the host's libm?** Spec acceptance criterion 1
//!    asks for within 1 ULP across a dense angle sweep. Two figures are given
//!    rather than one, because a single global maximum hides the shape: the
//!    gap is 1 ULP through the angles a player can reach, and grows beyond
//!    that only at magnitudes a 16-bit view angle makes unreachable.
//!
//! 3. **Does it return the same bits on another target?** A digest folded
//!    over every sample's output, built so it does not depend on how many
//!    threads ran it, so the number printed on glibc can be compared against
//!    the number printed by a musl or wasm build of this same program. This
//!    is deliberately narrow — it covers `sin_cos` alone, not a command
//!    stream through the whole simulation, which is C2's job.
//!
//! `dettrig.rs` is included by `#[path]` from the determinism probe rather
//! than copied or depended on. Copied, it could drift; depended on, its
//! `build.rs` would run — and that build script asserts step.rs still has
//! three `.sin_cos()` call sites, which is exactly what C1 removed. Including
//! the one file sidesteps a build script whose job is finished without
//! pretending the file is anything other than the original.
//!
//! ```sh
//! cargo run --release                 # full sweep, ~1 min on 8 threads
//! cargo run --release -- --quick      # strided, a few seconds
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use straf3_sim::num;

// Included whole rather than trimmed, so it stays the original file. Only
// `det_sin_cos` is called from here; `det_sin`/`det_cos` are the same
// reduction and would tell us nothing further.
#[path = "../../wasm-determinism/src/dettrig.rs"]
#[allow(dead_code)]
mod dettrig;

/// Degrees to radians, byte-for-byte the expression `step.rs` uses.
///
/// Reproduced, not imported — the constant is private to `step.rs`. Both fold
/// from the identical expression at compile time, so the sweep feeds
/// `sin_cos` the arguments `angle_vectors` actually feeds it.
const DEG_TO_RAD: f32 = core::f32::consts::PI * 2.0 / 360.0;

/// 2^-10 degrees. Below this every competent implementation returns `x` for
/// `sin(x)` to full `f32` precision; `probes/dettrig-accuracy` spot-checked
/// the tail and found nothing, and its reasoning is not re-litigated here.
const MIN_DEG_BITS: u32 = 0x3a80_0000;

/// 2^24 degrees — `f32`'s integer-exactness ceiling. Past it a single `f32`
/// stops identifying one angle, so disagreements stop being about `sin_cos`.
const MAX_DEG_BITS: u32 = 0x4b80_0000;

/// Where the reference was measured to leave 1 ULP behind (cosine first).
const REACHABLE_DEG_BITS: u32 = 0x4600_0000; // 8192.0

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let stride = if quick { 997 } else { 1 };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());

    let started = Instant::now();
    let totals = sweep(stride, threads);
    let elapsed = started.elapsed();

    println!("# C1 verification — straf3_sim::num::sin_cos");
    println!();
    // Named from cfg rather than written down, so a musl or Windows run
    // cannot be filed under the wrong libm by a stale literal.
    println!(
        "target            {}-{} (host libm: {})",
        std::env::consts::ARCH,
        std::env::consts::OS,
        if cfg!(target_env = "musl") {
            "musl"
        } else if cfg!(target_env = "gnu") && cfg!(target_os = "linux") {
            "glibc"
        } else {
            "the target's own"
        },
    );
    println!("domain            |degrees| in [2^-10, 2^24], both signs");
    println!("stride            every {stride} f32 bit pattern");
    println!(
        "samples           {}",
        totals.samples.load(Ordering::Relaxed)
    );
    println!("threads           {threads}");
    println!("elapsed           {:.1}s", elapsed.as_secs_f64());
    println!();

    let disagreements = totals.reference_disagreements.load(Ordering::Relaxed);
    println!("## 1. Against the reference it was designed from");
    println!();
    println!(
        "probes/wasm-determinism/src/dettrig.rs::det_sin_cos, included verbatim:\n\
         samples differing in any bit: {disagreements}",
    );
    println!();

    println!("## 2. Against the host's libm");
    println!();
    println!(
        "worst ULP over the whole swept domain     sin {}, cos {}",
        totals.max_ulp_sin.load(Ordering::Relaxed),
        totals.max_ulp_cos.load(Ordering::Relaxed),
    );
    println!(
        "worst ULP for |degrees| <= 8192           sin {}, cos {}",
        totals.max_ulp_sin_reachable.load(Ordering::Relaxed),
        totals.max_ulp_cos_reachable.load(Ordering::Relaxed),
    );
    println!();

    println!("## 3. Cross-target digest of every output");
    println!();
    println!(
        "0x{:016x}   (thread-count independent; compare against another target)",
        totals.digest.load(Ordering::Relaxed),
    );
    println!();

    if disagreements != 0 {
        eprintln!(
            "FAIL: the landed sin_cos is not the reference. \
             probes/dettrig-accuracy's measurements do not describe it."
        );
        std::process::exit(1);
    }
    println!("PASS: bit-identical to the reference on every sample swept.");
}

#[derive(Default)]
struct Totals {
    samples: AtomicU64,
    reference_disagreements: AtomicU64,
    max_ulp_sin: AtomicU64,
    max_ulp_cos: AtomicU64,
    max_ulp_sin_reachable: AtomicU64,
    max_ulp_cos_reachable: AtomicU64,
    digest: AtomicU64,
}

/// Fold one sample's output into the cross-target digest.
///
/// Combined with wrapping addition, which is commutative — so the digest does
/// not depend on how the sweep was divided between threads, and the figure
/// printed on a 4-core machine is comparable with the one from a 32-core
/// machine. Each sample is mixed first (the splitmix64 finaliser) so that
/// commutativity does not turn into "two swapped bits cancel out".
fn mix(argument_bits: u32, sin_bits: u32, cos_bits: u32) -> u64 {
    let mut x = (u64::from(argument_bits) << 32)
        ^ u64::from(sin_bits).rotate_left(17)
        ^ u64::from(cos_bits);
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn sweep(stride: u32, threads: usize) -> Totals {
    let totals = Totals::default();
    std::thread::scope(|scope| {
        for t in 0..threads {
            let totals = &totals;
            scope.spawn(move || {
                let mut local = Local::default();
                // Interleave by thread so every thread covers the whole range
                // of magnitudes, not one contiguous slice of it.
                let mut bits = MIN_DEG_BITS + (t as u32) * stride;
                while bits <= MAX_DEG_BITS {
                    let magnitude = f32::from_bits(bits);
                    let reachable = bits <= REACHABLE_DEG_BITS;
                    for degrees in [magnitude, -magnitude] {
                        local.record(degrees * DEG_TO_RAD, reachable);
                    }
                    // Guard the wrap at the top of the u32 range.
                    let Some(next) = bits.checked_add(stride * threads as u32) else {
                        break;
                    };
                    bits = next;
                }
                local.fold_into(totals);
            });
        }
    });
    totals
}

#[derive(Default)]
struct Local {
    samples: u64,
    reference_disagreements: u64,
    max_ulp_sin: u32,
    max_ulp_cos: u32,
    max_ulp_sin_reachable: u32,
    max_ulp_cos_reachable: u32,
    digest: u64,
}

impl Local {
    fn record(&mut self, radians: f32, reachable: bool) {
        self.samples += 1;

        let (own_sin, own_cos) = num::sin_cos(radians);
        let (ref_sin, ref_cos) = dettrig::det_sin_cos(radians);
        if own_sin.to_bits() != ref_sin.to_bits() || own_cos.to_bits() != ref_cos.to_bits() {
            self.reference_disagreements += 1;
        }
        self.digest =
            self.digest
                .wrapping_add(mix(radians.to_bits(), own_sin.to_bits(), own_cos.to_bits()));

        let (libm_sin, libm_cos) = radians.sin_cos();
        let sin_gap = ulp_gap(own_sin, libm_sin);
        let cos_gap = ulp_gap(own_cos, libm_cos);
        self.max_ulp_sin = self.max_ulp_sin.max(sin_gap);
        self.max_ulp_cos = self.max_ulp_cos.max(cos_gap);
        if reachable {
            self.max_ulp_sin_reachable = self.max_ulp_sin_reachable.max(sin_gap);
            self.max_ulp_cos_reachable = self.max_ulp_cos_reachable.max(cos_gap);
        }
    }

    fn fold_into(&self, totals: &Totals) {
        totals.samples.fetch_add(self.samples, Ordering::Relaxed);
        totals.digest.fetch_add(self.digest, Ordering::Relaxed);
        totals
            .reference_disagreements
            .fetch_add(self.reference_disagreements, Ordering::Relaxed);
        for (slot, value) in [
            (&totals.max_ulp_sin, self.max_ulp_sin),
            (&totals.max_ulp_cos, self.max_ulp_cos),
            (&totals.max_ulp_sin_reachable, self.max_ulp_sin_reachable),
            (&totals.max_ulp_cos_reachable, self.max_ulp_cos_reachable),
        ] {
            slot.fetch_max(u64::from(value), Ordering::Relaxed);
        }
    }
}

/// Distance between two `f32`s counted in representable values, with
/// sign-magnitude remapped to a monotone ordering so the count stays
/// meaningful across zero. NaN on either side counts as no gap: the sweep
/// never reaches one, and a NaN comparison here would be noise rather than a
/// finding.
fn ulp_gap(a: f32, b: f32) -> u32 {
    if a.is_nan() || b.is_nan() {
        return 0;
    }
    let ordered = |v: f32| -> i64 {
        let bits = i64::from(v.to_bits() as i32);
        if bits < 0 {
            i64::from(i32::MIN) - bits
        } else {
            bits
        }
    };
    (ordered(a) - ordered(b)).unsigned_abs() as u32
}

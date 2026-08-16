//! Cross-platform determinism probe for `straf3-sim`.
//!
//! One question: does `SimState::checksum()` come out bit-identical when the
//! same fixed input vectors are run natively on x86-64 and in a browser on
//! wasm32? Spec rev 5 §L — the leaderboard's server-side replay verification
//! is only sound if the answer is yes.
//!
//! The same code is compiled twice. Natively it is linked into `src/main.rs`,
//! which prints a JSON report. For wasm32-unknown-unknown it is a `cdylib`
//! whose `extern "C"` exports are called directly by `WebAssembly.instantiate`
//! in the page — no wasm-bindgen, so nothing sits between the sim's floats and
//! the numbers that come back.
//!
//! Beyond whole-run checksums the probe exposes per-step checksums (to find
//! the *first* diverging command, not just a different end state) and raw
//! sin/cos/sqrt bit patterns (to name the operation responsible).

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::FlatGround;
use straf3_sim::{
    Buttons, PhysicsProfile, SimState, TickRate, UserCmd, angle_to_short, step_in_place,
};

use std::sync::OnceLock;

pub mod dettrig;

/// A build-time copy of `straf3-sim` whose `angle_vectors` calls
/// [`dettrig::det_sin_cos`] instead of `libm`'s `sin_cos`. See `build.rs`:
/// three call sites are rewritten, nothing else, and `crates/` is untouched.
///
/// This exists so the "would owning our trig fix it?" question is answered by
/// running the physics, not by reasoning about one operation.
#[allow(missing_docs, clippy::all)]
pub mod patched_sim {
    include!(concat!(env!("OUT_DIR"), "/patched/lib.rs"));
}

/// `straf3-sim`'s own degrees→radians constant (`step.rs`), reproduced exactly
/// so the trig probe feeds `sin_cos` the identical argument the sim does.
const DEG_TO_RAD: Scalar = s(std::f32::consts::PI * 2.0 / 360.0);

// ── the input vectors ───────────────────────────────────────────────────────

/// The fixed cases. Each is a starting state, a profile and an input program.
///
/// The set is chosen so a divergence localises itself: if `cpm-strafe-turn`
/// differs but `cpm-yaw-0` does not, the culprit is an angle-dependent
/// operation; if `cpm-yaw-0` differs too, it is something more basic.
pub const CASE_NAMES: [&str; 6] = [
    "cpm-strafe-turn", // continuous turning, jumping — exercises AngleVectors hard
    "vq3-strafe-turn", // same inputs, VQ3 constants
    "cpm-yaw-0",       // identical inputs but yaw pinned to 0: sin=0, cos=1 exactly
    "cpm-yaw-90",      // yaw pinned to 90: one non-trivial transcendental, reused
    "cpm-still",       // no input at all: fall, land, rest
    "cpm-fine-turn",   // sub-degree yaw steps, no jumping — many distinct angles
];

/// How many commands each case runs.
const STEPS: u32 = 1_200; // 1200 × 8 ms = 9.6 s of simulated time

fn profile_for(case: u32) -> PhysicsProfile {
    if case == 1 {
        PhysicsProfile::vq3()
    } else {
        PhysicsProfile::cpm()
    }
}

/// The yaw this case is looking along on command `i`.
fn yaw_for(case: u32, i: u32) -> Scalar {
    match case {
        2 | 4 => s(0.0),
        3 => s(90.0),
        5 => s(i as f32) * s(0.013),
        _ => s(i as f32) * s(0.37),
    }
}

/// Build command `i` of a case, given whether the player is on the ground.
fn cmd_for(case: u32, i: u32, grounded: bool) -> UserCmd {
    let mut cmd = UserCmd::still_at(TickRate::HZ_125);
    // C3 made the view 16-bit at the command boundary, so the degrees
    // `yaw_for` returns have to be quantised the way a real recording does.
    cmd.view.yaw = angle_to_short(yaw_for(case, i));
    if case == 4 {
        return cmd; // "still": look straight ahead, press nothing
    }
    cmd.forward_move = 127;
    // Alternate the strafe key every 12 commands, the way a player half-beats
    // a strafe-jump; the sign flip is what makes the turn productive.
    cmd.right_move = if (i / 12) % 2 == 0 { 127 } else { -127 };
    // Jump whenever we can, except in the fine-turn case which stays grounded
    // so that friction and acceleration dominate instead of ballistics.
    if grounded && case != 5 {
        cmd.buttons = Buttons::JUMP;
    }
    cmd
}

fn spawn_state() -> SimState {
    SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0))
}

/// Run one case, returning the checksum after every command.
fn trajectory(case: u32) -> Vec<u64> {
    let world = FlatGround::at(s(0.0));
    let profile = profile_for(case);
    let mut state = spawn_state();
    let mut out = Vec::with_capacity(STEPS as usize);
    for i in 0..STEPS {
        let grounded = state.player.ground.is_grounded();
        let cmd = cmd_for(case, i, grounded);
        step_in_place(&mut state, &cmd, &world, &profile);
        out.push(state.checksum());
    }
    out
}

fn trajectories() -> &'static Vec<Vec<u64>> {
    static CACHE: OnceLock<Vec<Vec<u64>>> = OnceLock::new();
    CACHE.get_or_init(|| (0..CASE_NAMES.len() as u32).map(trajectory).collect())
}

// ── the same cases, through the patched sim ─────────────────────────────────

/// The identical input program run through [`patched_sim`], whose
/// `angle_vectors` uses [`dettrig`] instead of `libm`. Same six cases, same
/// 1200 commands, same checksum function.
fn patched_trajectory(case: u32) -> Vec<u64> {
    use patched_sim::cmd::{
        Buttons as PB, TickRate as PT, UserCmd as PU, angle_to_short as p_angle_to_short,
    };
    use patched_sim::profile::PhysicsProfile as PP;
    use patched_sim::state::SimState as PS;
    use patched_sim::world::FlatGround as PF;

    let world = PF::at(s(0.0));
    let profile = if case == 1 { PP::vq3() } else { PP::cpm() };
    let mut state = PS::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));
    let mut out = Vec::with_capacity(STEPS as usize);
    for i in 0..STEPS {
        let grounded = state.player.ground.is_grounded();
        let mut cmd = PU::still_at(PT::HZ_125);
        cmd.view.yaw = p_angle_to_short(yaw_for(case, i));
        if case != 4 {
            cmd.forward_move = 127;
            cmd.right_move = if (i / 12) % 2 == 0 { 127 } else { -127 };
            if grounded && case != 5 {
                cmd.buttons = PB::JUMP;
            }
        }
        patched_sim::step::step_in_place(&mut state, &cmd, &world, &profile);
        out.push(state.checksum());
    }
    out
}

fn patched_trajectories() -> &'static Vec<Vec<u64>> {
    static CACHE: OnceLock<Vec<Vec<u64>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        (0..CASE_NAMES.len() as u32)
            .map(patched_trajectory)
            .collect()
    })
}

/// Final checksum of a case run through the patched sim.
#[must_use]
pub fn patched_case_checksum(case: u32) -> u64 {
    *patched_trajectories()[case as usize].last().unwrap()
}

/// One digest over every per-command checksum of every patched case. This is
/// the end-to-end answer to "does owning the trig fix it?": if this number is
/// the same natively and in the browser, it does.
#[must_use]
pub fn patched_grand_checksum() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for traj in patched_trajectories() {
        for c in traj {
            for b in c.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

/// How many of a case's 1200 commands the patched sim and the stock sim
/// disagree on, on *this* platform — i.e. how much the fix moves the physics.
#[must_use]
pub fn patched_vs_stock_diff_count(case: u32) -> u32 {
    let a = &trajectories()[case as usize];
    let b = &patched_trajectories()[case as usize];
    a.iter().zip(b).filter(|(x, y)| x != y).count() as u32
}

// ── the raw-operation probes ────────────────────────────────────────────────

/// Angles fed to the trig probe, in degrees: a 0.5° sweep over a full turn,
/// followed by the exact yaw values `cpm-strafe-turn` produces.
fn probe_angles() -> &'static Vec<Scalar> {
    static CACHE: OnceLock<Vec<Scalar>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut a: Vec<Scalar> = (0..=720).map(|i| s(i as f32) * s(0.5)).collect();
        a.extend((0..256).map(|i| yaw_for(0, i)));
        a
    })
}

/// One scalar's worth of evidence about a single operation at one angle.
fn op_bits(op: u32, i: u32) -> u32 {
    let deg = probe_angles()[i as usize];
    let rad = deg * DEG_TO_RAD;
    let v = match op {
        0 => rad.sin_cos().0, // exactly what AngleVectors calls
        1 => rad.sin_cos().1,
        2 => rad.sin(), // separate calls, in case they lower differently
        3 => rad.cos(),
        4 => (rad.abs() + s(1.0)).sqrt(), // IEEE-exact control
        5 => rad,                         // the argument itself: pure multiply
        6 => dettrig::det_sin(rad),       // the proposed fix
        7 => dettrig::det_cos(rad),
        _ => f32::NAN,
    };
    v.to_bits()
}

/// How many f32 operations the probe reports per angle.
pub const N_OPS: u32 = 8;

/// A wide sweep: 200 000 angles from -720° to +720°, folded into one digest.
///
/// `kind` 0 uses `libm` (`sin_cos`, what the sim calls today), `kind` 1 uses
/// [`dettrig`]. Comparing digest 1 across platforms is the direct test of
/// whether shipping our own trig closes the gap.
#[must_use]
pub fn sweep_digest(kind: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for i in 0..SWEEP_N {
        let rad = sweep_angle(i);
        let (a, b) = if kind == 0 {
            rad.sin_cos()
        } else {
            (dettrig::det_sin(rad), dettrig::det_cos(rad))
        };
        for bits in [a.to_bits(), b.to_bits()] {
            for byte in bits.to_le_bytes() {
                h ^= u64::from(byte);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

const SWEEP_N: u32 = 200_000;

fn sweep_angle(i: u32) -> Scalar {
    let deg = (s(i as f32) - s(100_000.0)) * s(0.0072);
    deg * DEG_TO_RAD
}

/// Largest ULP gap between [`dettrig`] and this platform's `libm` over the
/// wide sweep — i.e. how much the proposed fix would move the physics.
/// `kind` 0 = sin, 1 = cos.
#[must_use]
pub fn sweep_max_ulp(kind: u32) -> u32 {
    let mut worst = 0u32;
    for i in 0..SWEEP_N {
        let rad = sweep_angle(i);
        let (libm_v, det_v) = if kind == 0 {
            (rad.sin(), dettrig::det_sin(rad))
        } else {
            (rad.cos(), dettrig::det_cos(rad))
        };
        let ord = |v: f32| {
            let u = v.to_bits();
            if u & 0x8000_0000 != 0 {
                -((u & 0x7fff_ffff) as i64)
            } else {
                i64::from(u)
            }
        };
        let d = (ord(libm_v) - ord(det_v)).unsigned_abs() as u32;
        worst = worst.max(d);
    }
    worst
}

/// Fraction (in parts per million) of the wide sweep where [`dettrig`] and
/// `libm` disagree at all. `kind` 0 = sin, 1 = cos.
#[must_use]
pub fn sweep_mismatch_ppm(kind: u32) -> u32 {
    let mut n = 0u64;
    for i in 0..SWEEP_N {
        let rad = sweep_angle(i);
        let differs = if kind == 0 {
            rad.sin().to_bits() != dettrig::det_sin(rad).to_bits()
        } else {
            rad.cos().to_bits() != dettrig::det_cos(rad).to_bits()
        };
        n += u64::from(differs);
    }
    (n * 1_000_000 / u64::from(SWEEP_N)) as u32
}

/// Edge cases of the *other* float operations the sim uses (`max`, `min`,
/// `clamp`, `abs`, division) plus NaN payloads.
///
/// These matter because the checksum folds exact bits: `+0.0` and `-0.0` are
/// different states, and `f32::max`/`min` are the one family whose signed-zero
/// and NaN behaviour is known to differ between LLVM's native lowering and
/// wasm's `f32.max`/`f32.min` instructions. Everything goes through
/// `black_box` so the compiler folds none of it at build time.
#[must_use]
// `z / z` and `inf - inf` are the point, not a mistake: they are how you get a
// NaN at runtime, and NaN *payloads* are exactly what a checksum would fold.
#[allow(clippy::eq_op)]
pub fn edge_bits(i: u32) -> u32 {
    use std::hint::black_box;
    let z = black_box(s(0.0));
    let nz = black_box(s(-0.0));
    let one = black_box(s(1.0));
    let nan = black_box(f32::NAN);
    let inf = black_box(f32::INFINITY);
    let v = match i {
        0 => z.max(nz),
        1 => nz.max(z),
        2 => z.min(nz),
        3 => nz.min(z),
        4 => nan.max(one),
        5 => one.max(nan),
        6 => nan.min(one),
        7 => one.min(nan),
        8 => nz.abs(),
        9 => nz.clamp(nz, z),
        10 => z * black_box(s(-1.0)),
        11 => nz.sqrt(),
        12 => z / z,                     // NaN payload from 0/0
        13 => inf - inf,                 // NaN payload from ∞-∞
        14 => black_box(s(-1.0)).sqrt(), // NaN payload from sqrt(-1)
        15 => nz + z,
        _ => f32::NAN,
    };
    v.to_bits()
}

/// How many edge cases [`edge_bits`] reports.
pub const N_EDGE: u32 = 16;

/// A field of a case's final state, as raw bits: 0..3 origin xyz,
/// 3..6 velocity xyz.
#[must_use]
pub fn final_field_bits(case: u32, field: u32) -> u32 {
    let f = final_states()[case as usize];
    let v = match field {
        0 => f.player.origin.x,
        1 => f.player.origin.y,
        2 => f.player.origin.z,
        3 => f.player.velocity.x,
        4 => f.player.velocity.y,
        _ => f.player.velocity.z,
    };
    v.to_bits()
}

fn final_states() -> &'static Vec<SimState> {
    static CACHE: OnceLock<Vec<SimState>> = OnceLock::new();
    CACHE.get_or_init(|| {
        (0..CASE_NAMES.len() as u32)
            .map(|case| {
                let world = FlatGround::at(s(0.0));
                let profile = profile_for(case);
                let mut state = spawn_state();
                for i in 0..STEPS {
                    let grounded = state.player.ground.is_grounded();
                    let cmd = cmd_for(case, i, grounded);
                    step_in_place(&mut state, &cmd, &world, &profile);
                }
                state
            })
            .collect()
    })
}

/// The same in f64, since a wasm libm may compute f32 sin via f64 internally.
fn op_bits64(op: u32, i: u32) -> u64 {
    let rad = f64::from(probe_angles()[i as usize]) * f64::from(DEG_TO_RAD);
    let v = match op {
        0 => rad.sin(),
        1 => rad.cos(),
        2 => rad.sqrt().abs(),
        _ => f64::NAN,
    };
    v.to_bits()
}

// ── the API, shared by the native binary and the wasm exports ───────────────

/// Number of fixed cases.
#[must_use]
pub fn case_count() -> u32 {
    CASE_NAMES.len() as u32
}

/// Commands per case.
#[must_use]
pub fn step_count() -> u32 {
    STEPS
}

/// Checksum of the final state of a case.
#[must_use]
pub fn case_checksum(case: u32) -> u64 {
    *trajectories()[case as usize].last().unwrap()
}

/// Checksum after command `stepi` of a case.
#[must_use]
pub fn step_checksum(case: u32, stepi: u32) -> u64 {
    trajectories()[case as usize][stepi as usize]
}

/// A single digest over every per-step checksum of every case: one number that
/// changes if anything anywhere in any trajectory changes.
#[must_use]
pub fn grand_checksum() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for traj in trajectories() {
        for c in traj {
            for b in c.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

/// Number of angles in the trig probe.
#[must_use]
pub fn angle_count() -> u32 {
    probe_angles().len() as u32
}

/// The probe angle itself, in degrees, as raw bits.
#[must_use]
pub fn angle_bits(i: u32) -> u32 {
    probe_angles()[i as usize].to_bits()
}

/// Raw bits of operation `op` at angle `i`. See [`op_bits`] for the op codes.
#[must_use]
pub fn f32_op_bits(op: u32, i: u32) -> u32 {
    op_bits(op, i)
}

/// Raw bits of the f64 operation `op` at angle `i`.
#[must_use]
pub fn f64_op_bits(op: u32, i: u32) -> u64 {
    op_bits64(op, i)
}

/// A digest over every f32 op at every probe angle.
#[must_use]
pub fn trig_checksum(op: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for i in 0..angle_count() {
        for b in op_bits(op, i).to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

// ── wasm exports (raw C ABI: no wasm-bindgen in the loop) ───────────────────

/// # Safety
/// These are plain value-in/value-out functions; the `extern "C"` surface
/// exists only so the browser can call them without a JS glue layer.
#[unsafe(no_mangle)]
pub extern "C" fn p_case_count() -> u32 {
    case_count()
}

#[unsafe(no_mangle)]
pub extern "C" fn p_step_count() -> u32 {
    step_count()
}

#[unsafe(no_mangle)]
pub extern "C" fn p_case_checksum(case: u32) -> u64 {
    case_checksum(case)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_step_checksum(case: u32, stepi: u32) -> u64 {
    step_checksum(case, stepi)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_grand_checksum() -> u64 {
    grand_checksum()
}

#[unsafe(no_mangle)]
pub extern "C" fn p_angle_count() -> u32 {
    angle_count()
}

#[unsafe(no_mangle)]
pub extern "C" fn p_angle_bits(i: u32) -> u32 {
    angle_bits(i)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_f32_op_bits(op: u32, i: u32) -> u32 {
    f32_op_bits(op, i)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_f64_op_bits(op: u32, i: u32) -> u64 {
    f64_op_bits(op, i)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_trig_checksum(op: u32) -> u64 {
    trig_checksum(op)
}

/// A sanity export: the sim's spawn-state checksum, computed without running
/// any physics. If even this differs the problem is not floating point.
#[unsafe(no_mangle)]
pub extern "C" fn p_spawn_checksum() -> u64 {
    spawn_state().checksum()
}

#[unsafe(no_mangle)]
pub extern "C" fn p_n_ops() -> u32 {
    N_OPS
}

#[unsafe(no_mangle)]
pub extern "C" fn p_patched_case_checksum(case: u32) -> u64 {
    patched_case_checksum(case)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_patched_grand_checksum() -> u64 {
    patched_grand_checksum()
}

#[unsafe(no_mangle)]
pub extern "C" fn p_patched_vs_stock_diff_count(case: u32) -> u32 {
    patched_vs_stock_diff_count(case)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_n_edge() -> u32 {
    N_EDGE
}

#[unsafe(no_mangle)]
pub extern "C" fn p_edge_bits(i: u32) -> u32 {
    edge_bits(i)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_sweep_digest(kind: u32) -> u64 {
    sweep_digest(kind)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_sweep_max_ulp(kind: u32) -> u32 {
    sweep_max_ulp(kind)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_sweep_mismatch_ppm(kind: u32) -> u32 {
    sweep_mismatch_ppm(kind)
}

#[unsafe(no_mangle)]
pub extern "C" fn p_final_field_bits(case: u32, field: u32) -> u32 {
    final_field_bits(case, field)
}

/// Vector-length probe (Q3's `VectorNormalize` shape) on a fixed vector, to
/// separate a `sqrt` disagreement from a trig one.
#[unsafe(no_mangle)]
pub extern "C" fn p_norm_bits(i: u32) -> u32 {
    let v: Vec3 = vec3(
        s(i as f32) * s(1.7),
        s(i as f32) * s(-0.3) + s(11.0),
        s(3.25),
    );
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    (v.x * (s(1.0) / len)).to_bits()
}

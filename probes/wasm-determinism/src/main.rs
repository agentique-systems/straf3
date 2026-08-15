//! Native side of the determinism probe: runs the fixed vectors on x86-64 and
//! writes the same JSON report the browser page produces, so the two can be
//! diffed key by key.
//!
//! Usage: `cargo run --release --bin native-report > native.json`

use det_probe as p;
use std::fmt::Write as _;

fn main() {
    let mut o = String::with_capacity(1 << 20);
    o.push_str("{\n");
    let _ = writeln!(
        o,
        "  \"platform\": \"native-{}-{}\",",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    let _ = writeln!(o, "  \"spawn\": \"{:016x}\",", spawn_checksum());
    let _ = writeln!(o, "  \"grand\": \"{:016x}\",", p::grand_checksum());
    let _ = writeln!(
        o,
        "  \"patched_grand\": \"{:016x}\",",
        p::patched_grand_checksum()
    );
    let _ = writeln!(o, "  \"step_count\": {},", p::step_count());

    o.push_str("  \"cases\": [\n");
    for case in 0..p::case_count() {
        let _ = write!(
            o,
            "    {{\"name\": \"{}\", \"final\": \"{:016x}\", \"steps\": [",
            p::CASE_NAMES[case as usize],
            p::case_checksum(case)
        );
        for stepi in 0..p::step_count() {
            if stepi > 0 {
                o.push(',');
            }
            let _ = write!(o, "\"{:016x}\"", p::step_checksum(case, stepi));
        }
        o.push_str("], \"final_fields\": [");
        for f in 0..6 {
            if f > 0 {
                o.push(',');
            }
            let _ = write!(o, "\"{:08x}\"", p::final_field_bits(case, f));
        }
        let _ = write!(
            o,
            "], \"patched_final\": \"{:016x}\", \"patched_vs_stock\": {}}}",
            p::patched_case_checksum(case),
            p::patched_vs_stock_diff_count(case)
        );
        if case + 1 < p::case_count() {
            o.push(',');
        }
        o.push('\n');
    }
    o.push_str("  ],\n");

    o.push_str("  \"trig_digest\": [");
    for op in 0..p::N_OPS {
        if op > 0 {
            o.push(',');
        }
        let _ = write!(o, "\"{:016x}\"", p::trig_checksum(op));
    }
    o.push_str("],\n");

    o.push_str("  \"angles\": [\n");
    for i in 0..p::angle_count() {
        let _ = write!(o, "    {{\"deg\": \"{:08x}\", \"f32\": [", p::angle_bits(i));
        for op in 0..p::N_OPS {
            if op > 0 {
                o.push(',');
            }
            let _ = write!(o, "\"{:08x}\"", p::f32_op_bits(op, i));
        }
        o.push_str("], \"f64\": [");
        for op in 0..3 {
            if op > 0 {
                o.push(',');
            }
            let _ = write!(o, "\"{:016x}\"", p::f64_op_bits(op, i));
        }
        o.push_str("]}");
        if i + 1 < p::angle_count() {
            o.push(',');
        }
        o.push('\n');
    }
    o.push_str("  ],\n");

    o.push_str("  \"norm\": [");
    for i in 0..64 {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "\"{:08x}\"", det_probe::p_norm_bits(i));
    }
    o.push_str("],\n");

    o.push_str("  \"edge\": [");
    for i in 0..p::N_EDGE {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "\"{:08x}\"", p::edge_bits(i));
    }
    o.push_str("],\n");

    let _ = write!(
        o,
        "  \"sweep\": {{\"libm\": \"{:016x}\", \"det\": \"{:016x}\", \
         \"max_ulp_sin\": {}, \"max_ulp_cos\": {}, \
         \"mismatch_ppm_sin\": {}, \"mismatch_ppm_cos\": {}}}\n}}\n",
        p::sweep_digest(0),
        p::sweep_digest(1),
        p::sweep_max_ulp(0),
        p::sweep_max_ulp(1),
        p::sweep_mismatch_ppm(0),
        p::sweep_mismatch_ppm(1),
    );

    print!("{o}");
}

/// Mirrors the `p_spawn_checksum` wasm export.
fn spawn_checksum() -> u64 {
    // The library exposes it only through the C ABI; calling that directly
    // keeps the two reports computing the identical thing.
    p_spawn()
}

fn p_spawn() -> u64 {
    det_probe::p_spawn_checksum()
}

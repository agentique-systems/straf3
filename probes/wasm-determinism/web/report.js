// Builds the same JSON report the native binary prints, by calling the raw
// C-ABI exports of det_probe.wasm. Shared verbatim by the Node runner (fast
// proxy) and the browser page (ground truth) so the two cannot drift.
//
// `instance` is a WebAssembly.Instance of det_probe.wasm.
export function buildReport(instance, platform) {
  const e = instance.exports;
  const h64 = (v) => BigInt.asUintN(64, v).toString(16).padStart(16, "0");
  const h32 = (v) => (v >>> 0).toString(16).padStart(8, "0");

  const nCases = e.p_case_count();
  const nSteps = e.p_step_count();
  const cases = [];
  for (let c = 0; c < nCases; c++) {
    const steps = [];
    for (let i = 0; i < nSteps; i++) steps.push(h64(e.p_step_checksum(c, i)));
    cases.push({ index: c, final: h64(e.p_case_checksum(c)), steps });
  }

  const nOps = e.p_n_ops();
  const trig_digest = [];
  for (let op = 0; op < nOps; op++) trig_digest.push(h64(e.p_trig_checksum(op)));

  const nAngles = e.p_angle_count();
  const angles = [];
  for (let i = 0; i < nAngles; i++) {
    const f32 = [];
    for (let op = 0; op < nOps; op++) f32.push(h32(e.p_f32_op_bits(op, i)));
    const f64 = [];
    for (let op = 0; op < 3; op++) f64.push(h64(e.p_f64_op_bits(op, i)));
    angles.push({ deg: h32(e.p_angle_bits(i)), f32, f64 });
  }

  const norm = [];
  for (let i = 0; i < 64; i++) norm.push(h32(e.p_norm_bits(i)));

  for (const c of cases) {
    c.final_fields = [];
    for (let f = 0; f < 6; f++) c.final_fields.push(h32(e.p_final_field_bits(c.index, f)));
    c.patched_final = h64(e.p_patched_case_checksum(c.index));
    c.patched_vs_stock = e.p_patched_vs_stock_diff_count(c.index);
  }

  const edge = [];
  for (let i = 0; i < e.p_n_edge(); i++) edge.push(h32(e.p_edge_bits(i)));

  const sweep = {
    libm: h64(e.p_sweep_digest(0)),
    det: h64(e.p_sweep_digest(1)),
    max_ulp_sin: e.p_sweep_max_ulp(0),
    max_ulp_cos: e.p_sweep_max_ulp(1),
    mismatch_ppm_sin: e.p_sweep_mismatch_ppm(0),
    mismatch_ppm_cos: e.p_sweep_mismatch_ppm(1),
  };

  return {
    platform,
    spawn: h64(e.p_spawn_checksum()),
    grand: h64(e.p_grand_checksum()),
    patched_grand: h64(e.p_patched_grand_checksum()),
    step_count: nSteps,
    cases,
    trig_digest,
    angles,
    norm,
    edge,
    sweep,
  };
}

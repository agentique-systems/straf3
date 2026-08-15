// Diff two probe reports (native vs wasm) and localise any divergence:
// which case, which command index, which angle, which operation, how many ULP.
import { readFileSync } from "node:fs";

const [aPath, bPath] = process.argv.slice(2);
const A = JSON.parse(readFileSync(aPath, "utf8"));
const B = JSON.parse(readFileSync(bPath, "utf8"));

const NAMES = A.cases.map((c) => c.name ?? `case${c.index}`);
const OPS = [
  "sin_cos.sin",
  "sin_cos.cos",
  "sin",
  "cos",
  "sqrt",
  "deg*DEG_TO_RAD",
  "det_sin (fix)",
  "det_cos (fix)",
];
const OPS64 = ["f64 sin", "f64 cos", "f64 sqrt"];

const f32 = (hex) => new Float32Array(new Uint32Array([parseInt(hex, 16)]).buffer)[0];
const f64 = (hex) =>
  new Float64Array(new BigUint64Array([BigInt("0x" + hex)]).buffer)[0];
// ULP distance for finite same-sign-ish floats, via the monotone ordered-int trick.
const ord32 = (hex) => {
  const u = parseInt(hex, 16) >>> 0;
  return u & 0x80000000 ? -(u & 0x7fffffff) : u;
};
const ulp32 = (x, y) => Math.abs(ord32(x) - ord32(y));

const out = [];
const say = (s) => out.push(s);

say(`A = ${A.platform}`);
say(`B = ${B.platform}`);
say(`spawn checksum:  A ${A.spawn}  B ${B.spawn}  ${A.spawn === B.spawn ? "MATCH" : "DIFFER"}`);
say(`grand checksum:  A ${A.grand}  B ${B.grand}  ${A.grand === B.grand ? "MATCH" : "DIFFER"}`);
if (A.patched_grand && B.patched_grand) {
  say(
    `patched-sim grand (own trig in AngleVectors): A ${A.patched_grand}  B ${B.patched_grand}  ` +
      `${A.patched_grand === B.patched_grand ? "MATCH — the fix closes the gap end to end" : "DIFFER"}`,
  );
}
say("");

say("── per-case ────────────────────────────────────────────────");
for (let c = 0; c < A.cases.length; c++) {
  const a = A.cases[c], b = B.cases[c];
  const same = a.final === b.final;
  let first = -1, ndiff = 0;
  for (let i = 0; i < a.steps.length; i++) {
    if (a.steps[i] !== b.steps[i]) { if (first < 0) first = i; ndiff++; }
  }
  say(
    `${NAMES[c].padEnd(18)} final ${same ? "MATCH " : "DIFFER"}  A ${a.final} B ${b.final}` +
      (first < 0
        ? "  — identical at all " + a.steps.length + " commands"
        : `  — first divergence at command #${first} (t=${first * 8} ms), ${ndiff}/${a.steps.length} commands differ`),
  );
  if (a.patched_final !== undefined) {
    say(
      `    patched: ${a.patched_final === b.patched_final ? "MATCH " : "DIFFER"} ` +
        `${a.patched_final} / ${b.patched_final}` +
        `  — own trig moves this case on ${a.patched_vs_stock}/${a.steps.length} commands (A), ` +
        `${b.patched_vs_stock}/${b.steps.length} (B)`,
    );
  }
  if (a.final_fields && b.final_fields && !same) {
    const labels = ["origin.x", "origin.y", "origin.z", "vel.x", "vel.y", "vel.z"];
    for (let f = 0; f < 6; f++) {
      const x = f32(a.final_fields[f]), y = f32(b.final_fields[f]);
      if (a.final_fields[f] !== b.final_fields[f]) {
        say(`    ${labels[f].padEnd(9)} A ${x}  B ${y}  Δ ${Math.abs(x - y)} units`);
      }
    }
  }
}
say("");

say("── raw operations, over " + A.angles.length + " probe angles ──────────────");
for (let op = 0; op < OPS.length; op++) {
  const bad = [];
  for (let i = 0; i < A.angles.length; i++) {
    const x = A.angles[i].f32[op], y = B.angles[i].f32[op];
    if (x !== y) bad.push([i, x, y]);
  }
  say(
    `${OPS[op].padEnd(16)} ${bad.length === 0 ? "identical at every angle" : `${bad.length}/${A.angles.length} angles differ`}`,
  );
  for (const [i, x, y] of bad.slice(0, 8)) {
    const deg = f32(A.angles[i].deg);
    say(
      `    deg=${deg}  A=${x} (${f32(x)})  B=${y} (${f32(y)})  ${ulp32(x, y)} ULP`,
    );
  }
  if (bad.length > 8) say(`    … ${bad.length - 8} more`);
  if (bad.length) {
    const maxUlp = Math.max(...bad.map(([, x, y]) => ulp32(x, y)));
    const degs = bad.map(([i]) => f32(A.angles[i].deg));
    say(`    max ${maxUlp} ULP; degree range ${Math.min(...degs)} … ${Math.max(...degs)}`);
  }
}
say("");

say("── f64 operations ──────────────────────────────────────────");
for (let op = 0; op < OPS64.length; op++) {
  const bad = [];
  for (let i = 0; i < A.angles.length; i++) {
    const x = A.angles[i].f64[op], y = B.angles[i].f64[op];
    if (x !== y) bad.push([i, x, y]);
  }
  say(`${OPS64[op].padEnd(16)} ${bad.length === 0 ? "identical at every angle" : `${bad.length}/${A.angles.length} angles differ`}`);
  for (const [i, x, y] of bad.slice(0, 4)) {
    say(`    deg=${f32(A.angles[i].deg)}  A=${x} (${f64(x)})  B=${y} (${f64(y)})`);
  }
}
say("");

if (A.norm && B.norm) {
  const bad = A.norm.filter((v, i) => v !== B.norm[i]).length;
  say(`normalize (x/sqrt(len²)): ${bad === 0 ? "identical at all 64 vectors" : `${bad}/64 differ`}`);
}
say(`trig digests: ${A.trig_digest.map((v, i) => (v === B.trig_digest[i] ? "=" : "≠")).join(" ")}  (${OPS.join(", ")})`);

if (A.edge && B.edge) {
  const EDGE = [
    "0.0.max(-0.0)", "(-0.0).max(0.0)", "0.0.min(-0.0)", "(-0.0).min(0.0)",
    "NaN.max(1.0)", "1.0.max(NaN)", "NaN.min(1.0)", "1.0.min(NaN)",
    "(-0.0).abs()", "(-0.0).clamp(-0.0,0.0)", "0.0 * -1.0", "(-0.0).sqrt()",
    "0.0/0.0 payload", "inf-inf payload", "sqrt(-1) payload", "-0.0 + 0.0",
  ];
  say("");
  say("── signed-zero / NaN edge cases (max, min, clamp, abs, div) ─");
  let bad = 0;
  for (let i = 0; i < A.edge.length; i++) {
    if (A.edge[i] !== B.edge[i]) {
      bad++;
      say(`    ${EDGE[i].padEnd(22)} A ${A.edge[i]}  B ${B.edge[i]}  DIFFER`);
    }
  }
  if (!bad) say(`    all ${A.edge.length} identical (incl. NaN bit payloads)`);
}

if (A.sweep && B.sweep) {
  say("");
  say("── 200 000-angle sweep, -720°…+720° ────────────────────────");
  say(`libm sin/cos digest:  A ${A.sweep.libm}  B ${B.sweep.libm}  ${A.sweep.libm === B.sweep.libm ? "MATCH" : "DIFFER"}`);
  say(`own-trig digest:      A ${A.sweep.det}  B ${B.sweep.det}  ${A.sweep.det === B.sweep.det ? "MATCH" : "DIFFER"}`);
  say(
    `own trig vs this platform's libm: max ULP sin ${A.sweep.max_ulp_sin}/${B.sweep.max_ulp_sin}, ` +
      `cos ${A.sweep.max_ulp_cos}/${B.sweep.max_ulp_cos}; ` +
      `disagreeing angles ${(A.sweep.mismatch_ppm_sin / 10000).toFixed(2)}% (A) vs ${(B.sweep.mismatch_ppm_sin / 10000).toFixed(2)}% (B)`,
  );
}

console.log(out.join("\n"));

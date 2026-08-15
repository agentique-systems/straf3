// Compact every full report down to the numbers worth keeping in the repo:
// digests, per-case finals, and which probe angles diverge. The full reports
// are ~300 KB each of per-command checksums and stay out of git.
import { readFileSync } from "node:fs";
import { basename } from "node:path";

const out = {};
for (const path of process.argv.slice(2)) {
  const j = JSON.parse(readFileSync(path, "utf8"));
  out[basename(path, ".json")] = {
    platform: j.platform,
    spawn: j.spawn,
    grand: j.grand,
    patched_grand: j.patched_grand,
    patched_case_finals: j.cases.map((c) => c.patched_final),
    patched_vs_stock: j.cases.map((c) => c.patched_vs_stock),
    step_count: j.step_count,
    case_finals: j.cases.map((c) => c.final),
    case_final_fields: j.cases.map((c) => c.final_fields),
    trig_digest: j.trig_digest,
    edge: j.edge,
    norm_digest: j.norm ? j.norm.join("").slice(0, 32) : null,
    sweep: j.sweep,
    wasm_imports: j.wasm_imports ?? null,
  };
}
process.stdout.write(JSON.stringify(out, null, 1) + "\n");

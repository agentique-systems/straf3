// Fast proxy: run det_probe.wasm under Node's V8 and emit the report.
// This is *not* the answer — the answer is the same wasm in Chrome — but V8
// here is the same engine family, so it catches problems in seconds.
import { readFile } from "node:fs/promises";
import { buildReport } from "./report.js";

const path = process.argv[2] ?? "det_probe.wasm";
const bytes = await readFile(path);
const mod = await WebAssembly.compile(bytes);

const imports = WebAssembly.Module.imports(mod);
const exports = WebAssembly.Module.exports(mod);
process.stderr.write(
  `imports: ${JSON.stringify(imports)}\nexport count: ${exports.length}\n`,
);

const instance = await WebAssembly.instantiate(mod, {});
const report = buildReport(instance, `node-v8-${process.versions.v8}`);
report.wasm_imports = imports;
process.stdout.write(JSON.stringify(report, null, 1));

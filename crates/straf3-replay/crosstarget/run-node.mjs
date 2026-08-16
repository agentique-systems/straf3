// Run straf3_replay.wasm under Node and print the .s3d cross-target report.
//
//     node run-node.mjs <path-to.wasm>
//
// Modelled on tools/det-runner/web/run-node.mjs, deliberately: the *wasm*
// renders the report and this script copies the bytes out of linear memory, so
// the JavaScript and the native binary cannot format the same numbers
// differently. No wasm-bindgen — the module is instantiated raw, so nothing
// sits between the simulation and the digest that comes back.
//
// V8 is the same engine family the browser runs; probes/wasm-determinism
// measured Node and headless Chrome agreeing bit-for-bit.
import { readFile } from "node:fs/promises";

const path = process.argv[2];
if (!path) {
  process.stderr.write("usage: node run-node.mjs <path-to.wasm>\n");
  process.exit(2);
}

const mod = await WebAssembly.compile(await readFile(path));

// The module should import nothing at all. If it does, say which — an
// unexplained import is a finding in its own right for a build that is
// supposed to be self-contained, and a silent stub could change behaviour.
const imports = WebAssembly.Module.imports(mod);
const importObject = {};
for (const { module, name, kind } of imports) {
  process.stderr.write(
    `run-node.mjs: WARNING stubbing unexpected ${kind} import ${module}.${name}\n`,
  );
  importObject[module] ??= {};
  importObject[module][name] = kind === "function" ? () => 0 : 0;
}

const instance = await WebAssembly.instantiate(mod, importObject);
const e = instance.exports;

for (const name of [
  "s3d_report_ptr",
  "s3d_report_len",
  "s3d_report_version",
  "memory",
]) {
  if (!(name in e)) {
    process.stderr.write(
      `run-node.mjs: ${path} does not export ${name} — is it built from crates/straf3-replay?\n`,
    );
    process.exit(1);
  }
}

process.stderr.write(
  `run-node.mjs: node ${process.version}, V8 ${process.versions.v8}, report v${e.s3d_report_version()}\n`,
);

const ptr = e.s3d_report_ptr() >>> 0;
const len = e.s3d_report_len() >>> 0;
const bytes = new Uint8Array(e.memory.buffer, ptr, len);
process.stdout.write(Buffer.from(bytes));

// The native binary exits non-zero when a case fails its own on-target
// assertions; do the same here so the driver does not have to special-case
// wasm.
if (!Buffer.from(bytes).toString("utf8").includes("\nall-ok true\n")) {
  process.stderr.write("run-node.mjs: a case failed on wasm32\n");
  process.exit(1);
}

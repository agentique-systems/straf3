// Run straf3_det_runner.wasm under Node and print the report it renders.
//
//     node run-node.mjs <path-to.wasm>
//
// Modelled on probes/wasm-determinism/web/node-run.mjs, with one deliberate
// difference: that probe rebuilt the report in JavaScript from a hundred
// numeric exports, which meant the JS and the native binary could format the
// same numbers differently. Here the *wasm* renders the report and this script
// copies the bytes out of linear memory, so the two texts cannot drift.
//
// No wasm-bindgen: the module is instantiated raw, so nothing sits between the
// simulation's floats and the digest that comes back. V8 is the same engine
// family the browser runs, which is what makes this a meaningful stand-in for
// the browser target; the probe measured Node and headless Chrome agreeing
// bit-for-bit on all six of its cases.
import { readFile } from "node:fs/promises";

const path = process.argv[2];
if (!path) {
  process.stderr.write("usage: node run-node.mjs <path-to.wasm>\n");
  process.exit(2);
}

const mod = await WebAssembly.compile(await readFile(path));

// The module should import nothing at all. If it does, say which — a silent
// stub could change behaviour, and an unexplained import is a finding in its
// own right for a build that is supposed to be self-contained.
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

for (const name of ["d_report_ptr", "d_report_len", "d_report_version", "memory"]) {
  if (!(name in e)) {
    process.stderr.write(
      `run-node.mjs: ${path} does not export ${name} — is it built from tools/det-runner?\n`,
    );
    process.exit(1);
  }
}

process.stderr.write(
  `run-node.mjs: node ${process.version}, V8 ${process.versions.v8}, report v${e.d_report_version()}\n`,
);

const ptr = e.d_report_ptr() >>> 0;
const len = e.d_report_len() >>> 0;
const bytes = new Uint8Array(e.memory.buffer, ptr, len);
process.stdout.write(Buffer.from(bytes));

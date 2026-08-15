// Pull the report JSON out of a headless-Chrome --dump-dom capture.
import { readFileSync } from "node:fs";

const dom = readFileSync(process.argv[2], "utf8");
const m = dom.match(/<pre id="out">([\s\S]*?)<\/pre>/);
if (!m) {
  const status = dom.match(/id="status">([^<]*)/);
  throw new Error(`no report in DOM (status: ${status ? status[1] : "unknown"})`);
}
process.stdout.write(
  m[1]
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&"),
);

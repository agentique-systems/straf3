// A static server for driving the browser build without the site.
//
// `web/dev/serve.mjs` is the one origin the wave ships — it proxies `/v1` to
// the records service and serves `web/site` as the pages. This one is smaller
// on purpose: it serves the client bundle and the maps and nothing else, so
// that the browser client can be loaded, played and debugged when the site or
// the service is not running (or not yet written). What it mounts is a subset
// of what the real origin mounts, at the same paths, so a URL that works here
// works there.
//
//   node crates/straf3-game/web/serve.mjs [port]
//
// On a software-only machine Chrome needs `--enable-unsafe-webgpu
// --use-angle=swiftshader` before it will offer a WebGPU adapter at all.

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const repo = join(here, "..", "..", "..");
const port = Number(process.argv[2] || 8788);

// The two mounts the wave contract reserves as first URL segments (URLS.md §6).
// `/assets/maps` is READ ONLY here — another session owns that directory.
const mounts = [
  ["/client/", here],
  ["/assets/maps/", join(repo, "assets", "maps")],
];

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".map": "text/plain; charset=utf-8",
  ".s3d": "application/octet-stream",
  ".json": "application/json",
};

// Resolve a URL path to a file, refusing anything that climbs out of a mount.
function resolve(pathname) {
  for (const [prefix, root] of mounts) {
    if (pathname.startsWith(prefix)) {
      const file = normalize(join(root, pathname.slice(prefix.length)));
      return file.startsWith(root) ? file : null;
    }
  }
  // `/play/<map>` and `/watch/<run>` are client-side routes: there is no file
  // behind them, and the shell reads the path itself. Everything else falls
  // through to the bundle directory so `/pkg/...` and `/index.html` work.
  const first = pathname.split("/")[1];
  if (first === "play" || first === "watch" || pathname === "/") {
    return join(here, "index.html");
  }
  const file = normalize(join(here, pathname));
  return file.startsWith(here) ? file : null;
}

createServer(async (request, response) => {
  const { pathname } = new URL(request.url, `http://localhost:${port}`);
  const file = resolve(decodeURIComponent(pathname));
  if (!file) {
    response.writeHead(403).end("outside the served roots");
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, {
      "content-type": types[extname(file)] || "application/octet-stream",
      // The bundle is rebuilt constantly while this is in use, and a cached
      // wasm module that no longer matches its JS glue fails in a way that
      // looks like a code bug rather than a stale file.
      "cache-control": "no-store",
    });
    response.end(body);
  } catch (e) {
    response.writeHead(e.code === "ENOENT" ? 404 : 500, {
      "content-type": "application/json",
    });
    response.end(JSON.stringify({ error: "not_found", detail: `${pathname}: ${e.code}` }));
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`straf3 client shell on http://127.0.0.1:${port}/play/coil`);
});

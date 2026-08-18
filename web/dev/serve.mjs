#!/usr/bin/env node
// @ts-check
/**
 * The straf3 site dev server.
 *
 *   node web/dev/serve.mjs [--port 8080] [--api http://127.0.0.1:3000] [--no-coi]
 *
 * Node built-ins only, no dependencies, no `node_modules`, no build step. The
 * site is plain ES modules and the browser loads them directly, so this server
 * is the entire toolchain. That is a deliberate choice and the reasoning is in
 * `docs/web/SITE.md` §2.
 *
 * It exists because three requirements are not satisfied by
 * `python3 -m http.server`:
 *
 *  1. **`.wasm` must be served as `application/wasm`.** `WebAssembly.
 *     instantiateStreaming` refuses any other type, and the error it produces
 *     names the wasm module rather than the MIME type, so the misconfiguration
 *     costs an hour the first time.
 *  2. **Durable links must survive a cold load** — `docs/web/URLS.md` §7. A
 *     path with no extension that matches no file serves the shell with status
 *     200. A missing *file* stays a 404.
 *  3. **Cross-origin isolation.** `Cross-Origin-Opener-Policy: same-origin`
 *     plus `Cross-Origin-Embedder-Policy: require-corp` are what a browser
 *     requires before it will hand out `SharedArrayBuffer`, which is what
 *     threaded wasm needs. The client build does not need it today; turning it
 *     on now means the day it does, nothing about the serving story changes.
 *     `--no-coi` turns it off if a cross-origin resource ever has to load.
 *
 * Two dev-only routes exist so the site is developable before the records
 * service does. Both are marked as dev-only in their responses, and both are
 * refused unless `--allow-local-runs` is passed:
 *
 *   GET /dev/runs                  the `.s3d` files in `runs/`, listed
 *   GET /dev/runs/<file>.s3d       one of them, as bytes
 *
 * The site labels anything sourced from them as coming from a local file, and
 * never as a verified record. A local file has no verification result, because
 * nothing verified it.
 */

import { createServer } from 'node:http';
import { createReadStream } from 'node:fs';
import { readFile, readdir, stat } from 'node:fs/promises';
import { extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const REPO = resolve(HERE, '..', '..');
const SITE_ROOT = join(REPO, 'web', 'site');
const RUNS_DIR = join(REPO, 'runs');

/** Extension → Content-Type. `.wasm` is the one that must be right. */
const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.map': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
  '.s3d': 'application/octet-stream',
};

// ── arguments ───────────────────────────────────────────────────────────────

/** @param {string[]} argv */
function parseArgs(argv) {
  const opts = {
    port: Number(process.env.STRAF3_SITE_PORT ?? 8080),
    host: process.env.STRAF3_SITE_HOST ?? '127.0.0.1',
    api: process.env.STRAF3_API_ORIGIN ?? '',
    coi: true,
    allowLocalRuns: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--port') opts.port = Number(argv[++i]);
    else if (a === '--host') opts.host = String(argv[++i]);
    else if (a === '--api') opts.api = String(argv[++i]);
    else if (a === '--no-coi') opts.coi = false;
    else if (a === '--allow-local-runs') opts.allowLocalRuns = true;
    else if (a === '--help' || a === '-h') {
      process.stdout.write(
        [
          'usage: node web/dev/serve.mjs [options]',
          '',
          '  --port N              listen port (default 8080)',
          '  --host H              bind address (default 127.0.0.1)',
          '  --api ORIGIN          proxy /v1/* to ORIGIN (the records service)',
          '  --no-coi              do not send COOP/COEP',
          '  --allow-local-runs    expose runs/*.s3d under /dev/runs (dev only)',
          '',
        ].join('\n'),
      );
      process.exit(0);
    } else {
      process.stderr.write(`unknown argument: ${a}\n`);
      process.exit(2);
    }
  }
  if (!Number.isInteger(opts.port) || opts.port < 1 || opts.port > 65535) {
    process.stderr.write(`bad --port\n`);
    process.exit(2);
  }
  return opts;
}

const opts = parseArgs(process.argv.slice(2));

// ── helpers ─────────────────────────────────────────────────────────────────

/** @param {import('node:http').ServerResponse} res */
function baseHeaders(res) {
  // Development server: never cache, or an edited module is invisible until a
  // hard reload and the next twenty minutes are spent debugging the wrong file.
  res.setHeader('Cache-Control', 'no-store');
  if (opts.coi) {
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
    res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
    res.setHeader('Cross-Origin-Resource-Policy', 'same-origin');
  }
}

/**
 * @param {import('node:http').ServerResponse} res
 * @param {number} code
 * @param {string} body
 * @param {string} [type]
 */
function send(res, code, body, type = 'text/plain; charset=utf-8') {
  baseHeaders(res);
  res.writeHead(code, { 'Content-Type': type });
  res.end(body);
}

/**
 * @param {import('node:http').ServerResponse} res
 * @param {number} code
 * @param {unknown} value
 */
function sendJson(res, code, value) {
  send(res, code, JSON.stringify(value, null, 2), 'application/json; charset=utf-8');
}

/**
 * Resolve a URL path to a file inside `root`, or `null` if it escapes.
 *
 * The check is on the *resolved* path, not on the raw one: `%2e%2e` decodes
 * after `decodeURIComponent`, and a prefix test against the un-normalised
 * string would pass it.
 *
 * @param {string} root
 * @param {string} urlPath
 * @returns {string|null}
 */
function safeJoin(root, urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath);
  } catch {
    return null;
  }
  if (decoded.includes('\0')) return null;
  const full = resolve(join(root, normalize(decoded)));
  if (full !== root && !full.startsWith(root + sep)) return null;
  return full;
}

/**
 * @param {import('node:http').ServerResponse} res
 * @param {string} file
 * @param {number} [code]
 */
async function sendFile(res, file, code = 200) {
  const info = await stat(file);
  baseHeaders(res);
  res.writeHead(code, {
    'Content-Type': MIME[extname(file).toLowerCase()] ?? 'application/octet-stream',
    'Content-Length': String(info.size),
  });
  createReadStream(file).pipe(res);
}

// ── request handling ────────────────────────────────────────────────────────

const server = createServer((req, res) => {
  handle(req, res).catch((err) => {
    process.stderr.write(`500 ${req.method} ${req.url}: ${err?.stack ?? err}\n`);
    if (!res.headersSent) send(res, 500, `server error: ${err?.message ?? err}\n`);
    else res.end();
  });
});

/**
 * @param {import('node:http').IncomingMessage} req
 * @param {import('node:http').ServerResponse} res
 */
async function handle(req, res) {
  const url = new URL(req.url ?? '/', `http://${req.headers.host ?? 'localhost'}`);
  const path = url.pathname;

  if (req.method !== 'GET' && req.method !== 'HEAD' && !path.startsWith('/v1/')) {
    return send(res, 405, 'method not allowed\n');
  }

  if (path === '/health') return sendJson(res, 200, { ok: true, server: 'straf3-site-dev' });

  // The records service. Proxied so a page never learns whether the API is
  // same-process or elsewhere (docs/web/URLS.md §6).
  if (path === '/v1' || path.startsWith('/v1/')) return proxyApi(req, res, url);

  // Dev-only local recordings.
  if (path === '/dev/runs' || path.startsWith('/dev/runs/')) return localRuns(res, path);

  const file = safeJoin(SITE_ROOT, path === '/' ? '/index.html' : path);
  if (file === null) return send(res, 400, 'bad path\n');

  try {
    const info = await stat(file);
    if (info.isDirectory()) {
      const index = join(file, 'index.html');
      try {
        await stat(index);
        return await sendFile(res, index);
      } catch {
        /* fall through to the routing rules below */
      }
    } else {
      return await sendFile(res, file);
    }
  } catch {
    /* not a real file — the routing rules below decide */
  }

  // docs/web/URLS.md §7. A path with an extension that does not exist is a
  // genuine 404: without this carve-out a missing `.wasm` returns HTML with a
  // 200 and the failure surfaces as a wasm magic-word error three layers away
  // from the missing file.
  if (extname(path) !== '') {
    return send(res, 404, `not found: ${path}\n`);
  }

  // A route. Serve the shell with 200 — not 404, or every crawler, cache and
  // link checker is told the page does not exist while the browser renders it.
  const shell = join(SITE_ROOT, 'index.html');
  try {
    return await sendFile(res, shell, 200);
  } catch {
    return send(res, 500, `the shell is missing: ${shell}\n`);
  }
}

/**
 * Proxy `/v1/*` to the records service, or answer honestly that there is none.
 *
 * The 503 body is a shape the site's API client understands, so an absent
 * service renders as "the records service is not running" rather than as an
 * empty leaderboard. Those are different facts and the site must not conflate
 * them.
 *
 * @param {import('node:http').IncomingMessage} req
 * @param {import('node:http').ServerResponse} res
 * @param {URL} url
 */
async function proxyApi(req, res, url) {
  if (!opts.api) {
    return sendJson(res, 503, {
      error: 'no_records_service',
      detail:
        'This dev server was started without --api, so there is no records service to ask. ' +
        'Start one and re-run with --api http://127.0.0.1:PORT.',
    });
  }
  const target = new URL(url.pathname + url.search, opts.api);
  /** @type {Record<string,string>} */
  const headers = {};
  for (const [k, v] of Object.entries(req.headers)) {
    if (k === 'host' || k === 'connection') continue;
    if (typeof v === 'string') headers[k] = v;
  }
  /** @type {RequestInit} */
  const init = { method: req.method, headers, redirect: 'manual' };
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    /** @type {Buffer[]} */
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    init.body = Buffer.concat(chunks);
  }
  let upstream;
  try {
    upstream = await fetch(target, init);
  } catch (err) {
    return sendJson(res, 502, {
      error: 'records_service_unreachable',
      detail: `${opts.api}: ${err instanceof Error ? err.message : String(err)}`,
    });
  }
  baseHeaders(res);
  const out = Object.fromEntries(upstream.headers);
  delete out['content-encoding'];
  delete out['content-length'];
  delete out['transfer-encoding'];
  res.writeHead(upstream.status, out);
  res.end(Buffer.from(await upstream.arrayBuffer()));
}

/**
 * Dev-only: the `.s3d` files sitting in `runs/`.
 *
 * `runs/` is outside this seat's ownership and is read here, never written.
 * The listing carries `source: "local-file"` so the site can label it as such;
 * a recording read off disk is a real recording and a real set of header
 * facts, and it is *not* a verified record, because nothing verified it.
 *
 * @param {import('node:http').ServerResponse} res
 * @param {string} path
 */
async function localRuns(res, path) {
  if (!opts.allowLocalRuns) {
    return sendJson(res, 403, {
      error: 'local_runs_disabled',
      detail: 'Restart the dev server with --allow-local-runs to read runs/*.s3d.',
    });
  }
  if (path === '/dev/runs') {
    /** @type {{name: string, bytes: number}[]} */
    const files = [];
    try {
      for (const name of (await readdir(RUNS_DIR)).sort()) {
        if (!name.endsWith('.s3d')) continue;
        files.push({ name, bytes: (await stat(join(RUNS_DIR, name))).size });
      }
    } catch (err) {
      return sendJson(res, 200, { source: 'local-file', dir: RUNS_DIR, files: [], error: String(err) });
    }
    return sendJson(res, 200, { source: 'local-file', dir: RUNS_DIR, files });
  }
  const name = path.slice('/dev/runs/'.length);
  if (!/^[A-Za-z0-9._-]+\.s3d$/.test(name)) return send(res, 400, 'bad recording name\n');
  const file = safeJoin(RUNS_DIR, '/' + name);
  if (file === null) return send(res, 400, 'bad path\n');
  try {
    return await sendFile(res, file);
  } catch {
    return send(res, 404, `no such recording: ${name}\n`);
  }
}

server.listen(opts.port, opts.host, () => {
  const origin = `http://${opts.host}:${opts.port}`;
  process.stdout.write(
    [
      `straf3 site  →  ${origin}`,
      `  root            ${SITE_ROOT}`,
      `  cross-origin    ${opts.coi ? 'isolated (COOP/COEP on)' : 'OFF (--no-coi)'}`,
      `  records service ${opts.api || 'none — /v1/* answers 503 no_records_service'}`,
      `  local runs      ${opts.allowLocalRuns ? `${RUNS_DIR} at /dev/runs` : 'disabled'}`,
      '',
      `  ${origin}/                          the map index`,
      `  ${origin}/m/coil/cpm                a category board`,
      `  ${origin}/play/coil                 launch coil        [criterion 15]`,
      '',
    ].join('\n'),
  );
});

// Drive the browser client in a real Chrome, over the DevTools protocol.
//
// `run-in-chrome.sh` in `probes/wasm-render` loads a page and dumps the DOM,
// which is enough to answer "did it render" and nothing else. Pointer lock,
// mouse-look and recording a run are all *interactive*, so they need a driver
// that can click, hold a key for a while, move the mouse and then read
// something back. That is what this is.
//
//   node crates/straf3-game/web/drive.mjs <steps.json> [--headful]
//
// The steps file is a JSON array; see `step()` below for the vocabulary. Each
// step's result is printed as one line of JSON, so the transcript of a run is
// greppable and can be committed as evidence.
//
// Chrome is launched with the probe's own flags. This box has no hardware GPU
// (WSL2 resolves Vulkan to lavapipe), and `--enable-unsafe-swiftshader` is
// what makes Chrome offer a software WebGPU adapter instead of refusing one:
// it tests the code path rather than the driver. Anything this reports about
// *timing* is therefore worthless — see the note in the report it produces.

import { spawn } from "node:child_process";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";

const CHROME = process.env.CHROME || "google-chrome";
const PORT = 9200 + Math.floor(Math.random() * 300);

// ── CDP, over the page target's own socket ──────────────────────────────────

let nextId = 1;
const pending = new Map();
const consoleLines = [];
let socket;

function send(method, params = {}) {
  const id = nextId++;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => {
      if (pending.delete(id)) reject(new Error(`${method} timed out`));
    }, 60000);
  });
}

/// Evaluate an expression in the page and return its value.
///
/// `awaitPromise` so a step can await something in the page; `returnByValue`
/// so the result crosses the protocol as JSON rather than as a remote handle.
async function evaluate(expression) {
  const { result, exceptionDetails } = await send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (exceptionDetails) {
    throw new Error(exceptionDetails.exception?.description || exceptionDetails.text);
  }
  return result.value;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/// Poll an expression until it is truthy. Used instead of a fixed sleep
/// wherever there is something real to wait for — the whole point of a
/// software rasteriser is that you cannot guess how long anything takes.
async function waitFor(expression, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (await evaluate(expression)) return true;
    if (Date.now() > deadline) throw new Error(`timed out waiting for: ${expression}`);
    await sleep(200);
  }
}

// ── input ───────────────────────────────────────────────────────────────────

// winit binds *physical* keys, so `code` is what matters and `key` is along
// for the ride. The virtual key codes are Windows ones, which is what CDP
// wants whatever the host is.
const KEYS = {
  KeyW: { key: "w", vk: 87 },
  KeyA: { key: "a", vk: 65 },
  KeyS: { key: "s", vk: 83 },
  KeyD: { key: "d", vk: 68 },
  KeyR: { key: "r", vk: 82 },
  Space: { key: " ", vk: 32 },
  ControlLeft: { key: "Control", vk: 17 },
  ShiftLeft: { key: "Shift", vk: 16 },
  Escape: { key: "Escape", vk: 27 },
};

async function keyEvent(type, code) {
  const spec = KEYS[code];
  if (!spec) throw new Error(`no key spec for ${code}`);
  await send("Input.dispatchKeyEvent", {
    type,
    code,
    key: spec.key,
    windowsVirtualKeyCode: spec.vk,
    nativeVirtualKeyCode: spec.vk,
    text: type === "keyDown" && spec.key.length === 1 ? spec.key : undefined,
  });
}

async function click(x, y) {
  const common = { x, y, button: "left", clickCount: 1 };
  await send("Input.dispatchMouseEvent", { type: "mousePressed", buttons: 1, ...common });
  await sleep(30);
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", buttons: 0, ...common });
}

/// Move the mouse in `steps` increments, the way a hand does.
///
/// One big jump is not the same input: the client accumulates deltas into an
/// absolute angle, and a single 600-count event exercises neither the
/// accumulation nor the per-frame sampling that mouse-look actually depends
/// on. `from` is where the cursor is now — under pointer lock the cursor does
/// not move, but the event still carries a position and Chrome derives
/// `movementX/Y` from consecutive ones.
async function mouseMove(from, dx, dy, steps = 20, delayMs = 8) {
  let [x, y] = from;
  for (let i = 0; i < steps; i++) {
    x += dx / steps;
    y += dy / steps;
    await send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: Math.round(x),
      y: Math.round(y),
      buttons: 0,
    });
    await sleep(delayMs);
  }
  return [x, y];
}

// ── the step vocabulary ─────────────────────────────────────────────────────

let cursor = [640, 360];

async function step(s) {
  switch (s.do) {
    case "navigate":
      await send("Page.navigate", { url: s.url });
      return { url: s.url };
    case "wait":
      await waitFor(s.until, s.timeout_ms);
      return { waited: s.until };
    case "sleep":
      await sleep(s.ms);
      return { slept: s.ms };
    case "eval":
      return { value: await evaluate(s.js) };
    // An escape hatch for protocol calls this vocabulary has no verb for, so
    // that finding out what a browser needs does not mean editing the driver
    // between every attempt.
    case "cdp":
      return { cdp: s.method, result: await send(s.method, s.params ?? {}) };
    case "click":
      cursor = [s.x ?? cursor[0], s.y ?? cursor[1]];
      await click(cursor[0], cursor[1]);
      return { clicked: cursor };
    case "key":
      await keyEvent(s.type ?? "keyDown", s.code);
      return { key: s.code, type: s.type ?? "keyDown" };
    // Hold keys down for a while, moving the mouse meanwhile. This is one step
    // rather than three because that is what playing is: a strafe is a key and
    // a mouse movement happening *together*, and a driver that could only do
    // them in sequence could not produce one.
    case "hold": {
      for (const code of s.keys ?? []) await keyEvent("keyDown", code);
      const until = Date.now() + s.ms;
      while (Date.now() < until) {
        if (s.mouse) cursor = await mouseMove(cursor, s.mouse[0], s.mouse[1], 10, 10);
        else await sleep(50);
      }
      for (const code of s.keys ?? []) await keyEvent("keyUp", code);
      return { held: s.keys ?? [], ms: s.ms };
    }
    case "mouse":
      cursor = await mouseMove(cursor, s.dx ?? 0, s.dy ?? 0, s.steps ?? 20, s.delay_ms ?? 8);
      return { moved: [s.dx ?? 0, s.dy ?? 0] };
    case "screenshot": {
      // `fromSurface` decides which of two different things is captured, and
      // on a software-only headless host they do not agree: true asks the
      // browser's compositor surface (which here comes back without the WebGPU
      // layer), false asks the renderer directly.
      const { data } = await send("Page.captureScreenshot", {
        format: "png",
        fromSurface: s.from_surface ?? true,
        captureBeyondViewport: false,
      });
      await mkdir(dirname(s.path), { recursive: true });
      await writeFile(s.path, Buffer.from(data, "base64"));
      return { screenshot: s.path };
    }
    // Pull the last finished run out of wasm and write the `.s3d`. This is
    // requirement r6's evidence leaving the browser.
    case "save_run": {
      const run = await evaluate(
        `(() => { const r = globalThis.__straf3_module?.straf3_last_run?.();
                  return r ? { time_ms: r.time_ms, run_digest_hex16: r.run_digest_hex16,
                               sim_time_ms: r.sim_time_ms, command_count: r.command_count,
                               map: r.map, physics: r.physics,
                               s3d: Array.from(r.s3d) } : null; })()`
      );
      if (!run) return { saved: null };
      await mkdir(dirname(s.path), { recursive: true });
      await writeFile(s.path, Buffer.from(run.s3d));
      const { s3d, ...rest } = run;
      return { saved: s.path, bytes: s3d.length, ...rest };
    }
    default:
      throw new Error(`unknown step: ${s.do}`);
  }
}

// ── main ────────────────────────────────────────────────────────────────────

const stepsPath = process.argv[2];
const headful = process.argv.includes("--headful");
const steps = JSON.parse(await readFile(stepsPath, "utf8"));

const profile = `/tmp/straf3-chrome-${PORT}`;
const chrome = spawn(
  CHROME,
  [
    ...(headful ? [] : ["--headless=new"]),
    "--no-sandbox",
    "--disable-dev-shm-usage",
    `--user-data-dir=${profile}`,
    `--remote-debugging-port=${PORT}`,
    // This box has no hardware GPU. Without these Chrome offers no WebGPU
    // adapter at all and the client correctly refuses to start — which is the
    // right behaviour, and is not what we are here to test.
    //
    // The exact flags are version-dependent and are therefore overridable:
    // `probes/wasm-render` used `--enable-unsafe-swiftshader
    // --enable-features=Vulkan,WebGPU`, which was enough for the Chrome of the
    // day and yields no adapter at all on Chrome 146.
    ...(process.env.CHROME_FLAGS ?? "--enable-unsafe-webgpu --use-angle=swiftshader")
      .split(/\s+/)
      .filter(Boolean),
    "--window-size=1280,720",
    "about:blank",
  ],
  { stdio: ["ignore", "ignore", "pipe"] }
);
chrome.stderr.on("data", (d) => {
  const text = `${d}`;
  if (/ERROR|FATAL/.test(text)) process.stderr.write(`[chrome] ${text}`);
});

// Wait for the debugging endpoint, then take the page target's own socket —
// attaching to the browser target instead would mean routing every message
// through sessions for no benefit here.
// Wait for the debugging endpoint itself, which is up long before any tab is.
let version;
for (let i = 0; i < 600; i++) {
  try {
    version = await fetch(`http://127.0.0.1:${PORT}/json/version`).then((r) => r.json());
    break;
  } catch {
    /* not up yet */
  }
  await sleep(100);
}
if (!version) {
  chrome.kill();
  throw new Error("Chrome never opened its debugging port");
}

// Ask for the tab rather than waiting for the one the command line implied.
// Headful Chrome under WSLg lists no page target for its initial window —
// only a component extension's service worker — so waiting for one is a
// timeout, and `/json/new` produces a real tab in both modes.
const target = await fetch(`http://127.0.0.1:${PORT}/json/new?about:blank`, {
  method: "PUT",
}).then((r) => r.json());
if (!target.webSocketDebuggerUrl) {
  chrome.kill();
  throw new Error(`Chrome would not open a tab: ${JSON.stringify(target)}`);
}

socket = new WebSocket(target.webSocketDebuggerUrl);
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    message.error ? reject(new Error(message.error.message)) : resolve(message.result);
    return;
  }
  if (message.method === "Runtime.consoleAPICalled") {
    const text = message.params.args
      .map((a) => a.value ?? a.description ?? a.unserializableValue ?? "")
      .join(" ");
    consoleLines.push(`${message.params.type}: ${text}`);
  }
  if (message.method === "Runtime.exceptionThrown") {
    consoleLines.push(
      `exception: ${message.params.exceptionDetails.exception?.description ?? ""}`
    );
  }
});
await new Promise((resolve) => socket.addEventListener("open", resolve));

await send("Page.enable");
await send("Runtime.enable");
// The client's module handle, so `save_run` can reach `straf3_last_run()`.
// The shell imports it as an ES module, which is not reachable from an
// evaluated expression otherwise.
await send("Page.addScriptToEvaluateOnNewDocument", {
  source: `globalThis.__straf3_capture = (m) => { globalThis.__straf3_module = m; };`,
});

let failed = false;
for (const s of steps) {
  try {
    const result = await step(s);
    console.log(JSON.stringify({ step: s.do, ...result }));
  } catch (e) {
    failed = true;
    console.log(JSON.stringify({ step: s.do, error: `${e.message}` }));
    break;
  }
}

console.log(JSON.stringify({ console: consoleLines }, null, 1));
socket.close();
chrome.kill();
process.exit(failed ? 1 : 0);

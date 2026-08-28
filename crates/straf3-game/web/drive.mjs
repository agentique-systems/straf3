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
// Chrome is launched with the probe's own flags, and THE DEFAULTS BELOW ARE
// WRONG ON ANY MACHINE WITH A GPU. They were written for a WSL2 box where
// Vulkan resolves to lavapipe and `--use-angle=swiftshader` was what made
// Chrome offer a software WebGPU adapter instead of refusing one — a way to
// test the code path rather than the driver, and worthless for timing.
//
// On a host with a real GPU, override them:
//
//   CHROME="C:/Program Files/Google/Chrome/Application/chrome.exe" \
//   CHROME_FLAGS="--no-first-run --no-default-browser-check" \
//   node crates/straf3-game/web/drive.mjs <steps.json> --headful
//
// Measured on native Windows with an RTX 3060 Ti (Chrome 151), the swiftshader
// default does not quietly downgrade to software — it yields NO adapter at all,
// `requestAdapter()` returns null and the client refuses to start. So the flags
// fail loudly rather than silently recording a run on a rasteriser. That is the
// safe direction, but it still means a driver run with the defaults on this
// host tests nothing. `docs/web/evidence/r17-browser.txt` §5 has both halves of
// the comparison.
//
// `--headful` matters too: it is the only mode in which a number here describes
// a real window on a real display.

import { spawn, spawnSync } from "node:child_process";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

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

/// Send an input event without waiting for Chrome to acknowledge it.
///
/// `send` writes to the socket synchronously and Chrome processes messages in
/// order, so ordering is preserved; the only thing dropped is the round trip.
/// That round trip is not free and it is not constant: measured on this
/// Windows host, `Input.dispatch*` acknowledges on the order of 80 ms because
/// the acknowledgement waits on the renderer, which is vsync-bound. A `hold`
/// step that awaited each of its ~7 events per iteration therefore managed
/// about 1.5 iterations a second, and delivered its mouse motion as a ~8°
/// staircase every 640 ms rather than as a turn.
///
/// That is not a cosmetic difference. Q3 air acceleration is driven by the
/// *rate* the view turns, so a driver that can only turn in coarse infrequent
/// bursts cannot strafejump — measured: 320 ups in, 320 ups out. Awaiting only
/// the last event of a burst is what makes the input rate a property of the
/// game loop instead of a property of the protocol.
function fire(method, params = {}) {
  // The rejection path still exists (the 60 s timeout), and nothing awaits it
  // any more, so it has to be swallowed or it surfaces as an unhandled
  // rejection and kills the process mid-run.
  send(method, params).catch(() => {});
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
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
/// Fold an angle difference into (-180, 180], so "turn to 90 from 350" is a
/// 100-degree turn and not a 260-degree one.
const wrap180 = (d) => (((d + 180) % 360) + 360) % 360 - 180;

/// Poll an expression until it is truthy. Used instead of a fixed sleep
/// wherever there is something real to wait for — the whole point of a
/// software rasteriser is that you cannot guess how long anything takes.
async function waitFor(expression, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  let last = "";
  for (;;) {
    // A throw is "not yet", not a failure. `Page.navigate` resolves before the
    // new document exists, so the first few polls run against the old one — or
    // against no document at all — and reading an element that is not there
    // yet raises rather than returning false.
    try {
      if (await evaluate(expression)) return true;
    } catch (e) {
      last = e.message;
    }
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for: ${expression}${last ? ` (last: ${last})` : ""}`);
    }
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
  fire("Input.dispatchKeyEvent", {
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
    // Fired rather than awaited: `delayMs` is meant to be the spacing between
    // samples of a hand's motion, and awaiting the acknowledgement added ~80 ms
    // of vsync-bound latency on top of it — turning a 10 ms cadence into a
    // 90 ms one and the sweep into a staircase. See `fire`.
    fire("Input.dispatchMouseEvent", {
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

/// The box a dispatched pointer position must stay inside. Inset from the
/// 1280×720 window so a sweep cannot land exactly on the edge.
const BOUNDS = { x0: 60, x1: 1220, y0: 60, y1: 660 };

/// Put the virtual cursor back at `(x, y)` without turning the view.
///
/// Mouse motion only turns the view while the pointer is grabbed, so the walk
/// back is free as long as it happens with the lock released. Escape releases
/// it; Chrome then refuses a fresh `requestPointerLock` for about a second
/// after a user-initiated exit, which is what the wait is for.
async function reanchor(x, y) {
  await keyEvent("keyDown", "Escape");
  await keyEvent("keyUp", "Escape");
  await sleep(1400);
  await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, buttons: 0 });
  cursor = [x, y];
  await send("Page.bringToFront");
  await click(x, y);
  await sleep(300);
}

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
      const until = Date.now() + s.ms;
      const samples = [];
      let nextSample = 0;
      while (Date.now() < until) {
        // Re-pressed rather than pressed once. Losing window focus makes the
        // client let go of every held key — deliberately, so that a player who
        // alt-tabs mid-strafe does not come back still strafing — and this box
        // blurs a busy page on its own. Without re-pressing, a run would
        // silently stop moving part-way through and look like a physics bug.
        // winit ignores OS key repeats, and these are not repeats.
        for (const code of s.keys ?? []) await keyEvent("keyDown", code);
        // Tapped rather than held. Q3's jump is edge-triggered — holding
        // +moveup jumps once and then does nothing — so a bunny hop is a
        // press-release cycle, not a key you lean on.
        for (const code of s.tap ?? []) {
          await keyEvent("keyDown", code);
          await sleep(20);
          await keyEvent("keyUp", code);
        }
        if (s.mouse) cursor = await mouseMove(cursor, s.mouse[0], s.mouse[1], 4, 10);
        else await sleep(60);
        if (s.sample_ms && Date.now() >= nextSample) {
          nextSample = Date.now() + s.sample_ms;
          const state = await evaluate("globalThis.__straf3_module?.straf3_debug_state?.() ?? null");
          if (state) {
            samples.push(
              `t${state.time_ms} (${state.x.toFixed(0)},${state.y.toFixed(0)},${state.z.toFixed(0)}) ` +
              `yaw${state.yaw.toFixed(0)} ${state.speed.toFixed(0)}ups ` +
              `${state.grounded ? "gnd" : "air"} run${state.run}:${state.run_ms}`
            );
          }
        }
      }
      for (const code of s.keys ?? []) await keyEvent("keyUp", code);
      return { held: s.keys ?? [], ms: s.ms, ...(samples.length ? { samples } : {}) };
    }
    case "mouse":
      cursor = await mouseMove(cursor, s.dx ?? 0, s.dy ?? 0, s.steps ?? 20, s.delay_ms ?? 8);
      return { moved: [s.dx ?? 0, s.dy ?? 0] };
    // Put the virtual cursor back in the middle of the viewport without
    // turning the view.
    //
    // `look_to` leaves the cursor wherever aiming happened to end, which is
    // routinely hard against a bound — a 90-degree turn is 818 counts and the
    // viewport is 1160 wide. A `hold` that strafes from there then dispatches
    // coordinates *outside* the viewport, where the deltas stop being the ones
    // sent (see `look_to`'s note), and it does so asymmetrically: the phase
    // turning away from the edge is delivered in full and the phase turning
    // into it is silently truncated. Measured, that reads as a strafe that
    // drifts steadily to one side for no reason visible in the step file —
    // which is how a run ends up in a wall.
    case "recenter":
      await reanchor(s.x ?? 640, s.y ?? 360);
      return { recentered: cursor };
    // Aim at an absolute view angle, by measuring rather than by dead
    // reckoning. Two things make open-loop aiming wrong: entering pointer lock
    // emits one warped delta of its own (measured: -16 counts of pitch), and a
    // sweep long enough to turn 90 degrees runs the cursor off the viewport,
    // where the deltas stop. So this reads the angle the client actually has,
    // moves by the remaining error in viewport-sized chunks, and looks again.
    case "look_to": {
      const perCount = 0.022 * 5; // Quake's m_yaw/m_pitch times cl_sensitivity
      const attempts = [];
      let reached = null;
      for (let attempt = 0; attempt < 8; attempt++) {
        const state = await evaluate("__straf3_module.straf3_debug_state()");
        reached = { yaw: +state.yaw.toFixed(2), pitch: +state.pitch.toFixed(2) };
        const yawError = s.yaw === undefined ? 0 : wrap180(s.yaw - state.yaw);
        const pitchError = s.pitch === undefined ? 0 : s.pitch - state.pitch;
        if (Math.abs(yawError) < 0.4 && Math.abs(pitchError) < 0.4) break;

        // Turning left is a negative delta, so the yaw error's sign flips.
        const wantX = -yawError / perCount;
        const wantY = pitchError / perCount;
        // Never leave the viewport. Measured: once a dispatched coordinate
        // goes outside it, the deltas stop being the ones sent — 80 events of
        // -20 came back as 108 events summing to -4277, with ±475 spikes in
        // them, because the real cursor starts being warped. Inside the
        // viewport every delta arrives exactly as sent.
        const dx = clamp(wantX, BOUNDS.x0 - cursor[0], BOUNDS.x1 - cursor[0]);
        const dy = clamp(wantY, BOUNDS.y0 - cursor[1], BOUNDS.y1 - cursor[1]);
        attempts.push(`err(${yawError.toFixed(1)},${pitchError.toFixed(1)}) move(${dx.toFixed(0)},${dy.toFixed(0)})`);

        if (Math.abs(dx) < 1 && Math.abs(dy) < 1) {
          // Hard against an edge with turning still to do. Drop the lock, walk
          // the cursor back across the viewport — which the client ignores,
          // because motion only turns the view while the pointer is
          // grabbed — and take it again.
          await reanchor(wantX < 0 ? BOUNDS.x1 : BOUNDS.x0, wantY < 0 ? BOUNDS.y1 : BOUNDS.y0);
          continue;
        }
        cursor = await mouseMove(cursor, dx, dy, Math.max(4, Math.round(Math.abs(dx + dy) / 20)), 6);
        await sleep(150);
      }
      return { look_to: { yaw: s.yaw, pitch: s.pitch }, reached, attempts };
    }
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

// `os.tmpdir()` rather than a literal `/tmp`: this driver is also run on
// Windows, where a Windows Chrome handed `/tmp/...` resolves it drive-relative
// against whatever the current drive is and silently writes its profile
// somewhere nobody looks.
const profile = join(tmpdir(), `straf3-chrome-${PORT}`);
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
shutDownChrome();
process.exit(failed ? 1 : 0);

/// Kill the browser, and mean the whole browser.
///
/// `chrome.kill()` signals only the process we spawned. Chrome is
/// multi-process, and on Windows the launcher hands off to a browser process
/// that is not in our job object, so killing the launcher orphans the tree:
/// the renderer, the GPU process and the crashpad handler all survive. Nothing
/// reaps them, they hold their profile directory, and they keep using the GPU.
///
/// Measured here after a dozen driver runs: 82 live `chrome.exe` from this
/// driver, and the host at 89 % CPU across 12 logical cores. That is not a
/// tidiness problem — this driver exists to take frame-pacing measurements, and
/// the leaked processes from earlier runs are contention that lands directly in
/// the number the next run reports. `taskkill /T` walks the tree; `/F` is
/// needed because a renderer told to close politely may wait on the browser
/// process we have already killed.
/// `spawnSync`, not `spawn`: the caller calls `process.exit()` on the next
/// line, which tears this process down before an asynchronous child has been
/// exec'd — measured, that leaves the tree alive exactly as if nothing had
/// been done. The kill has to complete before we are allowed to leave.
function shutDownChrome() {
  if (process.platform === "win32") {
    const killed = spawnSync("taskkill", ["/PID", `${chrome.pid}`, "/T", "/F"], {
      stdio: "ignore",
    });
    if (!killed.error) return;
  }
  chrome.kill();
}

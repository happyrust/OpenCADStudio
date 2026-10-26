import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const PROG = "browser.mjs";
const DEFAULT_CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const STEP_KINDS = ["eval", "click", "wait", "screenshot", "console", "snapshot"];
const MAX_PORT_ATTEMPTS = 3;
const DEVTOOLS_TIMEOUT_MS = 15000;
const READYSTATE_TIMEOUT_MS = 15000;
const CONSOLE_MAX_MESSAGES = 1000;
const CONSOLE_TEXT_MAX = 500;

const USAGE = `Usage: node ${PROG} --url <url> --out <evidence-dir> --steps <steps.json> [--timeout-ms <n>] [--chrome <path>]
       node ${PROG} --help

Zero-dependency headless-Chrome evidence harness. Launches Chrome with a
throwaway profile, drives it over the Chrome DevTools Protocol, executes a
declarative step list, prints one JSON line per executed step to stdout, and
writes screenshots under --out. The spawned Chrome and its temp profile are
always removed afterwards.

Options:
  --url <url>       Page URL to open; http(s):// and file:/// URLs accepted.
  --out <dir>       Evidence directory for screenshots; created recursively.
  --steps <file>    JSON array of step objects (eval | click | wait | screenshot |
                    console | snapshot).
  --timeout-ms <n>  Overall watchdog timeout in milliseconds (default 90000).
  --chrome <path>   Chrome binary path (default: CHROME_PATH env var, then
                    ${DEFAULT_CHROME}).
  --help            Print this help and exit without launching Chrome.

Step kinds:
  eval        { "kind":"eval", "expression":"<js>" }
  click       { "kind":"click", "selector":"<css>"[, "index":0] }
  wait        { "kind":"wait", "ms":<non-negative integer> }
  screenshot  { "kind":"screenshot"[, "file":"<name.png>"] }
  console     { "kind":"console"[, "clear":true|false] }
  snapshot    { "kind":"snapshot" }

Exit codes: 0 = navigation and all steps ok; 1 = step or navigation failed;
            2 = usage/config error; 3 = watchdog timeout.`;

class UsageError extends Error {}

class RunError extends Error {
  constructor(message, exitCode = 1) {
    super(message);
    this.exitCode = exitCode;
  }
}

function stderr(msg) {
  process.stderr.write(`${PROG}: ${msg}\n`);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function parseArgs(argv) {
  const args = { url: null, out: null, steps: null, timeoutMs: 90000, chrome: null, help: false };
  const needValue = (i, flag) => {
    if (i + 1 >= argv.length) throw new UsageError(`${flag} requires a value`);
    return argv[i + 1];
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--help" || a === "-h") {
      args.help = true;
      return args;
    } else if (a === "--url") {
      args.url = needValue(i, a); i++;
    } else if (a === "--out") {
      args.out = needValue(i, a); i++;
    } else if (a === "--steps") {
      args.steps = needValue(i, a); i++;
    } else if (a === "--chrome") {
      args.chrome = needValue(i, a); i++;
    } else if (a === "--timeout-ms") {
      const raw = needValue(i, a); i++;
      const v = Number(raw);
      if (!Number.isFinite(v) || v <= 0) {
        throw new UsageError(`--timeout-ms must be a positive number, got ${JSON.stringify(raw)}`);
      }
      args.timeoutMs = v;
    } else {
      throw new UsageError(`unknown argument: ${a}`);
    }
  }
  const missing = [];
  if (!args.url) missing.push("--url");
  if (!args.out) missing.push("--out");
  if (!args.steps) missing.push("--steps");
  if (missing.length) throw new UsageError(`missing required argument(s): ${missing.join(", ")}`);
  return args;
}

function loadSteps(file) {
  let raw;
  try {
    raw = fs.readFileSync(file, "utf8");
  } catch (e) {
    throw new RunError(`cannot read steps file ${file}: ${e.message}`, 2);
  }
  let steps;
  try {
    steps = JSON.parse(raw.charCodeAt(0) === 0xfeff ? raw.slice(1) : raw);
  } catch (e) {
    throw new RunError(`invalid JSON in steps file ${file}: ${e.message}`, 2);
  }
  if (!Array.isArray(steps)) {
    throw new RunError(`steps file must contain a JSON array: ${file}`, 2);
  }
  return steps;
}

// Minimal CDP JSON-RPC client over the built-in WebSocket.
class Cdp {
  constructor(wsUrl) {
    this.nextId = 0;
    this.pending = new Map();
    this.listeners = new Set();
    this.dead = false;
    const ws = new WebSocket(wsUrl);
    this.ws = ws;
    this.ready = new Promise((resolve, reject) => {
      const settle = () => {
        ws.removeEventListener("open", onOpen);
        ws.removeEventListener("error", onFail);
        ws.removeEventListener("close", onFail);
      };
      const onOpen = () => { settle(); resolve(); };
      const onFail = () => { settle(); reject(new RunError(`cannot connect to devtools websocket: ${wsUrl}`)); };
      ws.addEventListener("open", onOpen);
      ws.addEventListener("error", onFail);
      ws.addEventListener("close", onFail);
    });
    ws.addEventListener("message", (ev) => this._onMessage(ev));
    const onDead = () => this._rejectAll(new RunError("devtools websocket closed unexpectedly"));
    ws.addEventListener("close", onDead);
    ws.addEventListener("error", onDead);
  }

  _onMessage(ev) {
    let msg;
    try {
      msg = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (!msg) return;
    if (msg.id === undefined) {
      if (typeof msg.method === "string") {
        for (const fn of this.listeners) {
          try {
            fn(msg);
          } catch {}
        }
      }
      return;
    }
    const p = this.pending.get(msg.id);
    if (!p) return;
    this.pending.delete(msg.id);
    if (msg.error) p.reject(new RunError(msg.error.message || JSON.stringify(msg.error)));
    else p.resolve(msg.result === undefined ? {} : msg.result);
  }

  _rejectAll(err) {
    this.dead = true;
    const pend = [...this.pending.values()];
    this.pending.clear();
    for (const p of pend) p.reject(err);
  }

  call(method, params = {}, sessionId = null) {
    if (this.dead) return Promise.reject(new RunError("devtools websocket is closed"));
    const id = ++this.nextId;
    const msg = { id, method, params };
    if (sessionId) msg.sessionId = sessionId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      try {
        this.ws.send(JSON.stringify(msg));
      } catch (e) {
        this.pending.delete(id);
        reject(e);
      }
    });
  }

  onEvent(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  close() {
    this.dead = true;
    try { this.ws.close(); } catch {}
  }
}

function removeProfile(dir) {
  try {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  } catch {}
}

function killProcessTree(child) {
  if (!child) return;
  try {
    if (child.exitCode === null && child.signalCode === null) child.kill();
  } catch {}
  if (process.platform === "win32" && child.pid) {
    try {
      spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    } catch {}
  }
}

function waitForExit(child, ms) {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      child.removeListener("exit", onExit);
      resolve(false);
    }, ms);
    const onExit = () => {
      clearTimeout(timer);
      resolve(true);
    };
    child.once("exit", onExit);
  });
}

async function pollDevTools(port, child, getSpawnErr, errLines) {
  const deadline = Date.now() + DEVTOOLS_TIMEOUT_MS;
  const endpoint = `http://127.0.0.1:${port}/json/version`;
  while (Date.now() < deadline) {
    const se = getSpawnErr();
    if (se) throw new RunError(`chrome failed to start: ${se.message}`, 2);
    if (child.exitCode !== null || child.signalCode !== null) {
      const code = child.exitCode === null ? child.signalCode : child.exitCode;
      const tail = errLines.slice(-10).join(" | ") || "(no chrome output)";
      throw new RunError(`chrome exited before devtools was ready (exit ${code}): ${tail}`);
    }
    try {
      const res = await fetch(endpoint);
      if (res.ok) {
        const info = await res.json();
        if (info && typeof info.webSocketDebuggerUrl === "string" && info.webSocketDebuggerUrl) {
          return info.webSocketDebuggerUrl;
        }
      }
    } catch {}
    await sleep(100);
  }
  return null;
}

async function launchChrome(chromePath) {
  const tried = new Set();
  let lastError = "unknown";
  for (let attempt = 0; attempt < MAX_PORT_ATTEMPTS; attempt++) {
    let port;
    do {
      port = 9400 + Math.floor(Math.random() * 600); // 9400-9999, never 3110-3119
    } while (tried.has(port));
    tried.add(port);

    let profileDir;
    try {
      profileDir = fs.mkdtempSync(path.join(os.tmpdir(), "ocs-browser-"));
    } catch (e) {
      throw new RunError(`cannot create temp profile: ${e.message}`, 2);
    }

    const chromeArgs = [
      "--headless=new",
      "--disable-gpu",
      "--no-sandbox",
      "--hide-scrollbars",
      "--window-size=1280,800",
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-background-networking",
      "--disable-component-update",
      "--disable-sync",
      "--disable-extensions",
      "--mute-audio",
      `--user-data-dir=${profileDir}`,
      `--remote-debugging-port=${port}`,
      "about:blank",
    ];

    let child;
    try {
      child = spawn(chromePath, chromeArgs, { detached: false, stdio: ["ignore", "pipe", "pipe"] });
    } catch (e) {
      removeProfile(profileDir);
      throw new RunError(`failed to spawn chrome at ${chromePath}: ${e.message}`, 2);
    }

    const errLines = [];
    let spawnErr = null;
    child.stderr.on("data", (d) => {
      for (const line of String(d).split(/\r?\n/)) {
        if (line.trim()) {
          errLines.push(line.trim());
          if (errLines.length > 30) errLines.shift();
        }
      }
    });
    child.stdout.on("data", () => {});
    child.once("error", (e) => { spawnErr = e; });

    let wsUrl = null;
    let attemptError = null;
    try {
      wsUrl = await pollDevTools(port, child, () => spawnErr, errLines);
    } catch (e) {
      attemptError = e;
    }
    if (wsUrl) return { child, profileDir, wsUrl, port };
    killProcessTree(child);
    removeProfile(profileDir);
    if (attemptError) throw attemptError;
    lastError = `devtools endpoint not reachable on port ${port} within ${DEVTOOLS_TIMEOUT_MS / 1000}s`;
  }
  throw new RunError(`headless chrome did not start after ${MAX_PORT_ATTEMPTS} attempts (${lastError})`);
}

async function waitForReady(cdp, sessionId, url) {
  const deadline = Date.now() + READYSTATE_TIMEOUT_MS;
  let lastErr = null;
  while (Date.now() < deadline) {
    try {
      const r = await cdp.call("Runtime.evaluate", {
        expression: "document.readyState",
        returnByValue: true,
      }, sessionId);
      if (r && r.result && r.result.value === "complete") return;
    } catch (e) {
      lastErr = e;
    }
    await sleep(50);
  }
  throw new RunError(
    `document.readyState did not reach "complete" within ${READYSTATE_TIMEOUT_MS / 1000}s for ${url}` +
    (lastErr ? ` (last error: ${lastErr.message})` : "")
  );
}

function exceptionText(details) {
  const ex = details && details.exception;
  let text = "evaluation raised an exception";
  if (ex && ex.description) text = ex.description;
  else if (ex && ex.value !== undefined) text = String(ex.value);
  else if (details && details.text) text = details.text;
  text = String(text);
  return text.length > 500 ? `${text.slice(0, 500)}...` : text;
}

function resolveValue(result) {
  if (!result || typeof result !== "object") return null;
  if (Object.prototype.hasOwnProperty.call(result, "value")) return result.value;
  if (Object.prototype.hasOwnProperty.call(result, "unserializableValue")) return result.unserializableValue;
  if (result.type === "undefined") return null;
  if (Object.prototype.hasOwnProperty.call(result, "description")) return result.description;
  return null;
}

async function evaluate(cdp, sessionId, expression) {
  const r = await cdp.call("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
    userGesture: true,
  }, sessionId);
  if (r.exceptionDetails) throw new RunError(exceptionText(r.exceptionDetails));
  return r;
}

// Collects Runtime.consoleAPICalled / Runtime.exceptionThrown / Log.entryAdded
// events for one session into a bounded buffer drained by `console` steps.
function createConsoleCollector(sessionId) {
  const messages = [];
  let droppedCount = 0;

  const normText = (s) => {
    const t = String(s).trim();
    return t.length > CONSOLE_TEXT_MAX ? t.slice(0, CONSOLE_TEXT_MAX) : t;
  };

  const push = (level, text, source, url, line) => {
    messages.push({ level, text: normText(text), source, url: url || "", line: line || 0 });
    while (messages.length > CONSOLE_MAX_MESSAGES) {
      let idx = messages.findIndex((m) => m.level === "log" || m.level === "info");
      if (idx === -1) idx = messages.findIndex((m) => m.level === "warning");
      if (idx === -1) return; // buffer holds only errors; errors are never dropped
      messages.splice(idx, 1);
      droppedCount++;
    }
  };

  const onConsoleApi = (params) => {
    const type = params.type;
    const level = type === "error" || type === "assert" ? "error"
      : type === "warning" ? "warning"
      : type === "info" ? "info"
      : "log";
    const args = Array.isArray(params.args) ? params.args : [];
    const text = args.map((a) => {
      if (a && typeof a === "object") {
        if (a.value !== undefined) return String(a.value);
        if (a.description !== undefined) return String(a.description);
        if (a.type !== undefined) return String(a.type);
      }
      return String(a);
    }).join(" ");
    let url = "";
    let line = 0;
    const frames = params.stackTrace && params.stackTrace.callFrames;
    if (Array.isArray(frames) && frames.length) {
      url = frames[0].url || "";
      line = frames[0].lineNumber || 0;
    }
    push(level, text, "console", url, line);
  };

  const onException = (params) => {
    const d = params.exceptionDetails || {};
    const ex = d.exception;
    let text;
    if (ex && ex.description) text = String(ex.description);
    else if (ex && ex.value !== undefined) text = String(ex.value);
    else text = String(d.text || "");
    if (!text.startsWith("Uncaught")) text = `Uncaught ${text}`;
    push("error", text, "exception", d.url || "", d.lineNumber || 0);
  };

  const onLogEntry = (params) => {
    const entry = params.entry || {};
    const lvl = entry.level;
    const level = lvl === "error" ? "error"
      : lvl === "warning" ? "warning"
      : lvl === "info" ? "info"
      : "log"; // verbose and any other level
    const text = normText(entry.text || "");
    const dupe = messages.some(
      (m) => (m.source === "console" || m.source === "exception") &&
        m.level === level && m.text === text
    );
    if (dupe) return;
    push(level, text, "log", entry.url || "", entry.lineNumber || 0);
  };

  const handle = (msg) => {
    if (!msg || typeof msg !== "object") return;
    if (msg.sessionId !== undefined && msg.sessionId !== sessionId) return;
    const params = msg.params && typeof msg.params === "object" ? msg.params : {};
    if (msg.method === "Runtime.consoleAPICalled") onConsoleApi(params);
    else if (msg.method === "Runtime.exceptionThrown") onException(params);
    else if (msg.method === "Log.entryAdded") onLogEntry(params);
  };

  const report = ({ clear } = {}) => {
    const result = {
      errorCount: 0,
      warningCount: 0,
      infoCount: 0,
      logCount: 0,
      totalCount: messages.length,
      droppedCount,
      messages: messages.slice(),
    };
    for (const m of messages) {
      if (m.level === "error") result.errorCount++;
      else if (m.level === "warning") result.warningCount++;
      else if (m.level === "info") result.infoCount++;
      else result.logCount++;
    }
    if (clear) {
      messages.length = 0;
      droppedCount = 0;
    }
    return result;
  };

  return { handle, report };
}

// IIFE evaluated by `snapshot` steps; always returns a plain object.
const SNAPSHOT_EXPRESSION = `(() => {
  const norm = (s) => String(s == null ? "" : s).replace(/\\s+/g, " ").trim();
  const preferred = "[data-testid='main-nav'] a";
  const navSelector = document.querySelector(preferred) ? preferred : "nav a";
  const nav = [...document.querySelectorAll(navSelector)].map((a) => ({
    text: norm(a.textContent),
    href: a.getAttribute("href") || "",
    screen: a.getAttribute("data-screen") || "",
  }));
  const headings = [...document.querySelectorAll("h1, h2, h3")].map((h) => ({
    level: Number(h.tagName.slice(1)),
    text: norm(h.textContent),
  }));
  const testids = [...new Set(
    [...document.querySelectorAll("[data-testid]")].map((el) => el.getAttribute("data-testid"))
  )].sort();
  let body = "";
  try {
    body = document.body ? norm(document.body.innerText) : "";
  } catch {}
  return {
    title: document.title,
    url: location.href,
    hash: location.hash,
    navSelector: navSelector,
    nav: nav,
    navCount: nav.length,
    headings: headings,
    testids: testids,
    text: body.length > 4000 ? body.slice(0, 4000) : body,
  };
})()`;

async function execStep(cdp, sessionId, step, n, outDir, collector) {
  switch (step.kind) {
    case "eval": {
      if (typeof step.expression !== "string" || !step.expression) {
        throw new RunError("eval step requires a non-empty 'expression' string");
      }
      const r = await evaluate(cdp, sessionId, step.expression);
      return resolveValue(r.result);
    }
    case "click": {
      if (typeof step.selector !== "string" || !step.selector) {
        throw new RunError("click step requires a non-empty 'selector' string");
      }
      const index = step.index === undefined ? 0 : step.index;
      if (!Number.isInteger(index) || index < 0) {
        throw new RunError(`click step 'index' must be a non-negative integer, got ${JSON.stringify(step.index)}`);
      }
      const expression = `(() => {
        const els = document.querySelectorAll(${JSON.stringify(step.selector)});
        const count = els.length;
        const idx = ${index};
        if (idx >= count) return { found: false, count: count };
        const el = els[idx];
        el.scrollIntoView({ block: "center", inline: "center" });
        el.click();
        const text = (el.textContent || "").trim().slice(0, 80);
        return { found: true, count: count, tag: el.tagName.toLowerCase(), text: text };
      })()`;
      const r = await evaluate(cdp, sessionId, expression);
      const v = resolveValue(r.result);
      if (!v || !v.found) {
        const count = v && typeof v.count === "number" ? v.count : 0;
        throw new RunError(
          `selector not matched: ${step.selector} (${count} element${count === 1 ? "" : "s"} matched` +
          (index > 0 ? `, index ${index} out of range` : "") + `)`
        );
      }
      return v;
    }
    case "wait": {
      const ms = step.ms;
      if (!Number.isInteger(ms) || ms < 0) {
        throw new RunError(`wait step requires 'ms' as a non-negative integer, got ${JSON.stringify(ms === undefined ? step.ms : ms)}`);
      }
      await sleep(ms);
      return ms;
    }
    case "screenshot": {
      const file = step.file === undefined ? `step-${n}.png` : step.file;
      if (typeof file !== "string" || !file) {
        throw new RunError(`screenshot 'file' must be a non-empty string, got ${JSON.stringify(step.file)}`);
      }
      if (path.isAbsolute(file)) {
        throw new RunError(`screenshot path must be relative to --out: ${file}`);
      }
      const outRoot = path.resolve(outDir);
      const dest = path.resolve(outRoot, file);
      if (!dest.startsWith(outRoot + path.sep)) {
        throw new RunError(`screenshot path escapes --out: ${file}`);
      }
      await cdp.call("Page.bringToFront", {}, sessionId);
      const shot = await cdp.call("Page.captureScreenshot", { format: "png" }, sessionId);
      const data = shot && shot.data;
      if (!data || typeof data !== "string" || data.length === 0) {
        throw new RunError("Page.captureScreenshot returned no data");
      }
      const buf = Buffer.from(data, "base64");
      if (buf.length < 1) {
        throw new RunError("screenshot base64 decoded to 0 bytes");
      }
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.writeFileSync(dest, buf);
      return { file: dest, bytes: buf.length };
    }
    case "console": {
      if (step.clear !== undefined && typeof step.clear !== "boolean") {
        throw new RunError(`console step 'clear' must be a boolean, got ${JSON.stringify(step.clear)}`);
      }
      if (!collector) throw new RunError("console step requires an active console collector");
      return collector.report({ clear: step.clear === true });
    }
    case "snapshot": {
      const r = await evaluate(cdp, sessionId, SNAPSHOT_EXPRESSION);
      return resolveValue(r.result);
    }
    default:
      throw new RunError(
        `unknown step kind ${JSON.stringify(step.kind)} (allowed kinds: ${STEP_KINDS.join(", ")})`
      );
  }
}

async function runSteps(cdp, sessionId, steps, outDir, collector) {
  for (let i = 0; i < steps.length; i++) {
    const n = i + 1;
    const raw = steps[i];
    const step = raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
    const kind = step.kind === undefined ? null : step.kind;
    const line = { step: n, kind, value: null, error: null };
    try {
      line.value = await execStep(cdp, sessionId, step, n, outDir, collector);
    } catch (e) {
      line.error = `step ${n} (${kind}) failed: ${e && e.message ? e.message : String(e)}`;
      process.stdout.write(JSON.stringify(line) + "\n");
      return line;
    }
    process.stdout.write(JSON.stringify(line) + "\n");
  }
  return null;
}

const state = { child: null, profileDir: null, cdp: null, watchdog: null, cleaned: false };

async function cleanup() {
  if (state.cleaned) return;
  state.cleaned = true;
  try {
    if (state.cdp && !state.cdp.dead) {
      await Promise.race([state.cdp.call("Browser.close"), sleep(2000)]);
    }
  } catch {}
  try {
    if (state.cdp) state.cdp.close();
  } catch {}
  const child = state.child;
  if (child) {
    try { if (child.stdout) child.stdout.destroy(); } catch {}
    try { if (child.stderr) child.stderr.destroy(); } catch {}
    if (!(await waitForExit(child, 3000))) {
      try { child.kill(); } catch {}
      if (process.platform === "win32" && !(await waitForExit(child, 1000))) {
        try {
          spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
        } catch {}
      }
      await waitForExit(child, 2000);
    }
  }
  if (state.profileDir) removeProfile(state.profileDir);
}

let watchdogFired = false;
async function onWatchdog(timeoutMs) {
  if (watchdogFired) return;
  watchdogFired = true;
  stderr(`watchdog timeout after ${timeoutMs} ms; aborting run`);
  try { await cleanup(); } catch {}
  process.exit(3);
}

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    stderr(e.message);
    process.stderr.write(USAGE + "\n");
    process.exit(2);
  }
  if (args.help) {
    process.stdout.write(USAGE + "\n");
    process.exit(0);
  }

  let steps;
  try {
    steps = loadSteps(args.steps);
  } catch (e) {
    stderr(e.message);
    process.exit(e.exitCode || 2);
  }

  const chromePath = args.chrome || process.env.CHROME_PATH || DEFAULT_CHROME;
  if (!fs.existsSync(chromePath)) {
    stderr(`chrome binary not found: ${chromePath} (use --chrome <path> or set CHROME_PATH)`);
    process.exit(2);
  }

  try {
    fs.mkdirSync(args.out, { recursive: true });
  } catch (e) {
    stderr(`cannot create output dir ${args.out}: ${e.message}`);
    process.exit(2);
  }

  state.watchdog = setTimeout(() => { void onWatchdog(args.timeoutMs); }, args.timeoutMs);
  const onSignal = (sig, code) => {
    stderr(`received ${sig}; cleaning up`);
    cleanup().then(() => process.exit(code), () => process.exit(code));
  };
  process.on("SIGINT", () => onSignal("SIGINT", 130));
  process.on("SIGTERM", () => onSignal("SIGTERM", 143));

  let exitCode = 0;
  let summary;
  try {
    const launched = await launchChrome(chromePath);
    state.child = launched.child;
    state.profileDir = launched.profileDir;

    state.cdp = new Cdp(launched.wsUrl);
    await state.cdp.ready;
    const { targetId } = await state.cdp.call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await state.cdp.call("Target.attachToTarget", { targetId, flatten: true });
    await state.cdp.call("Page.enable", {}, sessionId);
    await state.cdp.call("Runtime.enable", {}, sessionId);
    try {
      await state.cdp.call("Log.enable", {}, sessionId);
    } catch {}
    const consoleCollector = createConsoleCollector(sessionId);
    state.cdp.onEvent((msg) => consoleCollector.handle(msg));
    await state.cdp.call("Page.bringToFront", {}, sessionId);
    const nav = await state.cdp.call("Page.navigate", { url: args.url }, sessionId);
    if (nav && nav.errorText) {
      exitCode = 1;
      summary = `navigation failed for ${args.url}: ${nav.errorText}`;
    } else {
      await waitForReady(state.cdp, sessionId, args.url);
      const failed = await runSteps(state.cdp, sessionId, steps, args.out, consoleCollector);
      if (failed) {
        exitCode = 1;
        summary = failed.error;
      } else {
        summary = `${steps.length}/${steps.length} steps ok`;
      }
    }
  } catch (e) {
    exitCode = e.exitCode || 1;
    summary = e && e.message ? e.message : String(e);
  } finally {
    if (state.watchdog) clearTimeout(state.watchdog);
    await cleanup();
  }
  stderr(summary);
  process.exit(exitCode);
}

main().catch(async (e) => {
  stderr(`unhandled error: ${e && e.stack ? e.stack : e}`);
  try { await cleanup(); } catch {}
  process.exit(1);
});

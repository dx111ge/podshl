// Drives the real client window, over the web view's own DevTools protocol.
//
// Not synthetic keystrokes: those collide with whoever is at the keyboard, and
// on 2026-09-14 an attempt to automate the operator's desktop with `ydotool`
// was called "pure rubbish" and was. This talks to the window's own engine and
// works with the DOM. The window appears while it runs; nothing is typed at the
// operating system, so nothing a person is doing can be interrupted by it.
//
// Needs Node 22 or newer, for the built-in WebSocket. No dependencies.

import { spawn, execFileSync } from "node:child_process";
import { readFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";

const sleep = ms => new Promise(r => setTimeout(r, ms));

/** Where the client is. Named rather than searched for: a test that finds a
 *  binary by walking the disk is one that eventually finds the wrong one. */
export function clientPath() {
  const named = process.env.PODSHL_CLIENT;
  if (named) {
    if (!existsSync(named)) throw new Error(`PODSHL_CLIENT is ${named}, which does not exist`);
    return resolve(named);
  }
  const win = process.platform === "win32";
  // Resolved here rather than at `spawn`, which runs it with a `cwd` of the
  // binary's own directory — a relative path is then relative to where the
  // program is, and the answer is ENOENT for a file that plainly exists.
  const here = dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
  const guesses = [
    resolve(here, "..", "..", "target", "debug", win ? "podshl-client.exe" : "podshl-client"),
    resolve(here, "..", "..", "target", "release", win ? "podshl-client.exe" : "podshl-client"),
  ];
  for (const g of guesses) if (existsSync(g)) return g;
  throw new Error(
    "no client to drive. Build one with `scripts/build_client.ps1 <operator>` or " +
    "`scripts/build_client.sh <operator>`, or set PODSHL_CLIENT. These tests walk " +
    "the real binary; there is nothing to assert without it.");
}

/** Refuse a client that was not built for an operator.
 *
 *  `cargo test` rebuilds `target/debug` without `PODSHL_BUILD_*`, and what
 *  comes out points at loopback with no pinned log key — it starts, it draws
 *  the window, and it finds no published project at all. Driving it produces a
 *  timeout twenty-five seconds later with nothing to read, which is how this
 *  harness failed the first time it was run after a `cargo test`. The binary
 *  can answer the question, so it is asked. */
export function refuseDevelopmentBuild(bin) {
  if (process.env.UITEST_ALLOW_DEV_BUILD === "1") return;
  let ep;
  try {
    ep = JSON.parse(execFileSync(bin, ["invoke", "endpoints", "{}"], { encoding: "utf8" }));
  } catch (e) {
    throw new Error(`${bin} could not answer \`invoke endpoints\`: ${e.message}`);
  }
  if (ep.built === false || ep.has_key === false) {
    throw new Error(
      `${bin} is a development build: it talks to ${ep.operator} and has ` +
      `${ep.has_key === false ? "no pinned log key" : "a key"}, so no published project is ` +
      `found and this walk would time out with nothing to read.
` +
      `  Build one that is: scripts/build_client.ps1 <operator>  (or build_client.sh)
` +
      `  and point PODSHL_CLIENT at it, because \`cargo test\` overwrites target/debug.
` +
      `  Set UITEST_ALLOW_DEV_BUILD=1 only if you mean to drive one.`);
  }
}

/** Start the client with its web view listening, and connect to it. */
export async function openWindow({ port = 9444, env = {} } = {}) {
  const bin = clientPath();
  refuseDevelopmentBuild(bin);
  const logDir = join(tmpdir(), `podshl-uitest-${process.pid}-${port}`);
  rmSync(logDir, { recursive: true, force: true });
  mkdirSync(logDir, { recursive: true });
  const logFile = join(logDir, "client.log");

  // **Nothing else may be on this port.** If a window from an earlier run is
  // still listening, the code below connects to *that* one and drives it while
  // reading the log file of the one it just started — so every panel behaves
  // and every assertion about the log fails, which is a confusing way to spend
  // twenty minutes. Ask first.
  try {
    const stale = await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(1500) });
    if (stale.ok) {
      throw new Error(
        `something is already listening on ${port} — a window from an earlier run?
` +
        `  Close it, or pass UITEST_PORT with a free one. Attaching to it would drive ` +
        `that window while reading this one's log.`);
    }
  } catch (e) {
    if (e instanceof Error && e.message.startsWith("something is already listening")) throw e;
    // Anything else means nothing answered, which is what we want.
  }

  const child = spawn(bin, [], {
    cwd: dirname(bin),
    stdio: "ignore",
    env: {
      ...process.env,
      // **WebView2 only, which means Windows only.** This was written claiming
      // WebKitGTK takes the port the same way. It does not: it has no CDP
      // endpoint at all, it exposes WebKit's own remote inspector through
      // `WEBKIT_INSPECTOR_SERVER`, and nothing here speaks that protocol. So
      // this harness runs on Windows and nowhere else — which is the reason
      // it cannot be the flow's coverage in CI, and the reason that gap has to
      // be closed somewhere else.
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
      PODSHL_LOG: logFile,
      ...env,
    },
  });

  let page = null;
  for (let i = 0; i < 80 && !page; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      page = list.find(t => t.type === "page");
    } catch { /* not listening yet */ }
    if (!page) await sleep(250);
  }
  if (!page) {
    child.kill();
    throw new Error(`the window never opened a debugging port on ${port}`);
  }

  const ws = new WebSocket(page.webSocketDebuggerUrl);
  const pending = new Map();
  let id = 0;
  ws.addEventListener("message", e => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  });
  await new Promise((res, rej) => {
    ws.addEventListener("open", res);
    ws.addEventListener("error", rej);
  });

  const send = (method, params) => new Promise(res => {
    const n = ++id;
    pending.set(n, res);
    ws.send(JSON.stringify({ id: n, method, params }));
  });

  /** Evaluate in the page and return the value. Throws what the page threw. */
  async function evaluate(expression) {
    const r = await send("Runtime.evaluate", {
      expression, returnByValue: true, awaitPromise: true, userGesture: true,
    });
    const ex = r.result?.exceptionDetails;
    if (ex) throw new Error(`the window threw: ${ex.exception?.description || ex.text}`);
    return r.result?.result?.value;
  }

  /** Wait until an expression is truthy, and say what was being waited for. */
  async function waitFor(expression, what, ms = 25000) {
    const until = Date.now() + ms;
    while (Date.now() < until) {
      if (await evaluate(expression)) return true;
      await sleep(200);
    }
    throw new Error(
      `timed out after ${ms}ms waiting for ${what}.\n` +
      `  panels on screen: ${JSON.stringify(await panels())}\n` +
      `  the client log:\n${log().split("\n").map(l => "    " + l).join("\n")}`);
  }

  /** Wait until the client log matches, and say what was being waited for.
   *
   *  **The log is written behind the DOM, on purpose.** `logUi` is
   *  `Promise.resolve(invoke("log_line", ...)).catch(() => {})` — fire and
   *  forget, so that writing a line can never stall or break the flow it is
   *  describing. The consequence is that the line arrives a moment *after* the
   *  panel whose appearance proves it happened.
   *
   *  Reading the log once, at the instant a panel appears, is therefore a race.
   *  It was one: on 2026-09-16 the first run of the day reported that the window
   *  "did not record reaching the published path" about a window that had just
   *  drawn the published panel, and every run after it passed. A flaky test is
   *  worse than a red one, because a red one is read and a flaky one is re-run.
   *
   *  **Observed once, and it would not reproduce on demand** -- not with a cold
   *  index cache, not on either version. So this is a fix argued from the code
   *  rather than one demonstrated by turning it red again, and that is said out
   *  loud rather than dressed up. Waiting cannot be worse than reading once.
   *
   *  This waits instead, and still fails — with the whole log — if the line
   *  never comes. Returns the log it matched, so a caller can make its *other*
   *  assertions against one settled snapshot rather than re-reading a moving
   *  file. */
  async function waitForLog(re, what, ms = 10000) {
    const until = Date.now() + ms;
    for (;;) {
      const seen = log();
      if (re.test(seen)) return seen;
      if (Date.now() >= until) {
        throw new Error(
          `timed out after ${ms}ms waiting for ${what} in the client log.\n` +
          `  looked for: ${re}\n` +
          `  panels on screen: ${JSON.stringify(await panels())}\n` +
          `  the client log:\n${seen.split("\n").map(l => "    " + l).join("\n")}`);
      }
      await sleep(100);
    }
  }

  /** The heading of every panel, which is what a person sees. */
  const panels = () => evaluate(
    `[...document.querySelectorAll(".panel")].map(p => (p.querySelector("h3")||p).textContent.trim())`);

  /** What the binary and the window wrote down. `log_line` is why the window
   *  half exists: every gate on the published path is decided in JavaScript. */
  const log = () => (existsSync(logFile) ? readFileSync(logFile, "utf8") : "");

  async function close() {
    try { ws.close(); } catch { /* already gone */ }
    child.kill();
    for (let i = 0; i < 20 && !child.killed; i++) await sleep(100);
  }

  return { evaluate, waitFor, waitForLog, panels, log, close, logFile };
}

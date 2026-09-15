// Talking to a page over the DevTools protocol, for whatever is showing it.
//
// Two things show this window: the client itself, through WebView2, and a
// headless Chromium with the Tauri IPC replaced. They are different programs
// and the same protocol, so this is the half they share.
//
// Node 22 or newer for the built-in WebSocket — or Node 20 with
// `--experimental-websocket`, which is what the container has and what
// `scripts/ci.sh` detects rather than assumes. No dependencies either way.

const sleep = ms => new Promise(r => setTimeout(r, ms));

/** The page target on a debugging port, once something is listening. */
export async function findPage(port, { timeoutMs = 20000 } = {}) {
  const until = Date.now() + timeoutMs;
  while (Date.now() < until) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = list.find(t => t.type === "page");
      if (page) return page;
    } catch { /* not listening yet */ }
    await sleep(250);
  }
  return null;
}

/** Refuse to attach to something that was already there.
 *
 *  A window or a browser left over from an earlier run answers, gets driven,
 *  and every assertion about what the run produced fails for a reason nobody
 *  would guess — because what is being read belongs to the process that was
 *  just started and then ignored. */
export async function refuseIfOccupied(port) {
  try {
    const stale = await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(1500) });
    if (stale.ok) {
      throw new Error(
        `something is already listening on ${port} — a window or browser from an ` +
        `earlier run? Close it, or use a free port. Attaching to it would drive ` +
        `that one while reading this one's output.`);
    }
  } catch (e) {
    if (e instanceof Error && e.message.startsWith("something is already listening")) throw e;
    // Anything else means nothing answered, which is what we want.
  }
}

/** Attach to a page target and return the two things a test needs. */
export async function attach(page) {
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
    if (ex) throw new Error(`the page threw: ${ex.exception?.description || ex.text}`);
    return r.result?.result?.value;
  }

  return { send, evaluate, close: () => { try { ws.close(); } catch { /* gone */ } } };
}

/** Wait until an expression is truthy, and say what was waited for.
 *
 *  `context()` is whatever the caller can add to the message — the panels on
 *  screen, the log — because a timeout that says only "timed out" is a timeout
 *  somebody has to reproduce before they can read it. */
export async function waitFor(evaluate, expression, what, { ms = 25000, context = null } = {}) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    if (await evaluate(expression)) return true;
    await sleep(200);
  }
  const extra = context ? "\n" + (await context()) : "";
  throw new Error(`timed out after ${ms}ms waiting for ${what}.${extra}`);
}

export { sleep };

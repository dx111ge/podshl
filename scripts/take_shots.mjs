// Photographs the operator's pages for ONBOARDING.md and engram's walkthrough.
//
// Taken from the running system, like the screencasts: the pages are served by
// the operator, the builder checks against the real `/validate`, the dashboards
// are real projects with real reports. Re-run it when a page changes, rather
// than leaving a document describing a screen that no longer exists.
//
// Needs Node 22+, the stack up with engram enrolled and a seeded project
// (`scripts/seed_large_dashboard.py` with HOST=your-project.example), and Chrome
// started with a debugging port:
//
//   & "C:\Program Files\Google\Chrome\Application\chrome.exe" --headless=new `
//       --remote-debugging-port=9334 --user-data-dir=$env:TEMP\podshl-shots-chrome about:blank
//   $env:PROJECT_TOKEN = '<the seed script prints it>'
//   $env:ENGRAM_TOKEN  = '<a claim token for engram.localhost>'
//   node scripts\take_shots.mjs
//
// Dark theme, 1280 wide, device scale 2 so text stays sharp in a document.
import { readFileSync, writeFileSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";

const PORT = process.env.CDP_PORT || "9334";
const SERVER = process.env.PODSHL_SERVER_URL || "http://127.0.0.1:8725";
const PROJECT = process.env.PROJECT_HOST || "your-project.example";
const DOCS = "docs/shots";
const ENGRAM = "examples/engram/shots";
const sleep = ms => new Promise(r => setTimeout(r, ms));
mkdirSync(DOCS, { recursive: true });

const list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
const target = list.find(t => t.type === "page");
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = ev => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
};
const send = (method, params = {}) => new Promise(res => {
  const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params }));
});
async function js(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error("page: " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result?.result?.value;
}
async function waitFor(expr, what, ms = 20000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try { if (await js(expr)) return; } catch {}
    await sleep(150);
  }
  throw new Error(`timed out waiting for ${what}`);
}
async function go(url) {
  await send("Page.navigate", { url });
  await sleep(900);
  await waitFor(`document.readyState === 'complete'`, url);
}

await send("Page.enable");
await send("Runtime.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 900, deviceScaleFactor: 2, mobile: false });
await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });

/* One element, with a margin of page around it — or the viewport when no
   selector is given. The element is scrolled into view first, because a clip
   outside the rendered area comes back blank. */
async function shot(file, selector, pad = 16, until = null) {
  await sleep(350);
  let clip;
  if (selector) {
    // `until`, when given, ends the picture at the bottom of that element — so
    // a long list below the part that matters is not in a document.
    const box = await js(`(()=>{ const e=${selector}; if(!e) return null; e.scrollIntoView({block:'start'});
      const r=e.getBoundingClientRect(); const u=${until || "null"};
      const bottom = u ? u.getBoundingClientRect().bottom : r.bottom;
      return {x:r.left+scrollX, y:r.top+scrollY, w:r.width, h:bottom - r.top}; })()`);
    if (!box) throw new Error(`nothing to photograph for ${file}`);
    await sleep(250);
    clip = { x: Math.max(0, box.x - pad), y: Math.max(0, box.y - pad), width: box.w + 2 * pad,
             height: Math.min(box.h + 2 * pad, 4000), scale: 1 };
  }
  const r = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: !!clip, ...(clip ? { clip } : {}) });
  writeFileSync(file, Buffer.from(r.result.data, "base64"));
  console.log("  shot", file);
}

const byId = sel => `document.getElementById(${JSON.stringify(sel)})`;

// ------------------------------------------------------------------ the builder

await go(`${SERVER}/publish/build`);
await js(`(localStorage.removeItem('podshl.build.draft'), true)`);
await go(`${SERVER}/publish/build`);
await waitFor(`${byId("catalogue")}.options.length > 1`, "the vocabulary");
await js(`${byId("example")}.click(), true`);
await sleep(300);
await shot(`${DOCS}/01-builder.png`);
await js(`${byId("check")}.click(), true`);
await waitFor(`/mirror would/.test(${byId("verdict")}.innerText)`, "the check");
await shot(`${DOCS}/02-builder-checked.png`, `document.querySelector('.builder-out .sticky')`);
await js(`(document.querySelector('#solutions .card'), true)`);
await shot(`${DOCS}/03-builder-solution.png`, `document.querySelector('#solutions .card')`);

// Loading existing files: engram's, as a maintainer would pick them.
const engramDir = "examples/engram/.podshl";
const files = [{ name: "agent.yaml", text: readFileSync(join(engramDir, "agent.yaml"), "utf8") }]
  .concat(readdirSync(join(engramDir, "solutions")).map(n => ({ name: n, text: readFileSync(join(engramDir, "solutions", n), "utf8") })));
await js(`(()=>{ const dt=new DataTransfer();
  ${JSON.stringify(files)}.forEach(f => dt.items.add(new File([f.text], f.name, {type:'text/plain'})));
  const i=${byId("load")}; i.files=dt.files; i.dispatchEvent(new Event('change')); return true })()`);
await waitFor(`/^Loaded/.test(${byId("loaded")}.textContent)`, "the loaded files");
await js(`(${byId("anchor")}.value='https://dx111ge.github.io/engram/', ${byId("anchor")}.dispatchEvent(new Event('input')), true)`);
// engram's `ollama.host`: a reading that also asks a question, which the form
// has no field for — so it is kept as written, and says so.
await shot(`${DOCS}/04-builder-loaded.png`,
  `[...document.querySelectorAll('#probes .card')].find(c => [...c.querySelectorAll('input')].some(i => i.value === 'ollama.host'))`);
await js(`(localStorage.removeItem('podshl.build.draft'), true)`);

// ------------------------------------------------------------------ register

await go(`${SERVER}/register`);
await shot(`${DOCS}/05-register.png`);

// ------------------------------------------------------------------ the dashboard

async function signIn(host, token) {
  // Through a blank page: the same page under another `#host` does not reload,
  // and would keep the last project's open tab.
  await go("about:blank");
  await go(`${SERVER}/dashboard#${host}`);
  await js(`(()=>{ localStorage.clear(); ${byId("host")}.value=${JSON.stringify(host)};
    const t=${byId("token")}; t.type='password'; t.value=${JSON.stringify(token)}; ${byId("open")}.click(); return true })()`);
}

await go(`${SERVER}/dashboard#${PROJECT}`);
await js(`(()=>{ ${byId("host")}.value=${JSON.stringify(PROJECT)}; ${byId("token")}.value='served-not-the-token';
  ${byId("open")}.click(); return true })()`);
await waitFor(`!${byId("signin-error")}.hidden`, "the refusal");
await shot(`${DOCS}/06-dashboard-refused.png`, byId("signin"));

await signIn(PROJECT, process.env.PROJECT_TOKEN || "");
await waitFor(`!${byId("view").hidden} && document.querySelectorAll('#groups details.group').length > 0`, "the dashboard");
await js(`window.scrollTo(0,0), true`);
await shot(`${DOCS}/07-dashboard-problems.png`);
// An answer whose fork separates on a reading and offers an `answers.when` to
// paste — the whole point of the suggestion, rather than one that cannot.
await js(`(()=>{ document.querySelectorAll('#groups details.group').forEach(g => { g.open = true; }); return true })()`);
await waitFor(`[...document.querySelectorAll('#groups details.group .panel.warn')].some(p => p.querySelector('pre'))`, "a pasteable fork");
await js(`(()=>{ const keep=[...document.querySelectorAll('#groups details.group')].find(g => (g.querySelector('.panel.warn')||{}).querySelector && g.querySelector('.panel.warn pre'));
  document.querySelectorAll('#groups details.group').forEach(g => { if (g !== keep) g.open = false; }); return true })()`);
await sleep(300);
await shot(`${DOCS}/08-dashboard-fork.png`,
  `[...document.querySelectorAll('#groups details.group')].find(d=>d.open)`, 16,
  `[...document.querySelectorAll('#groups details.group')].find(d=>d.open).querySelector('.panel.warn')`);
await js(`${byId("tab-files")}.click(), true`);
await shot(`${DOCS}/09-dashboard-files.png`, byId("panel-files"));

await signIn("engram.localhost", process.env.ENGRAM_TOKEN || "");
await waitFor(`!${byId("view").hidden} && [...document.querySelectorAll('#chips button')].some(b=>b.textContent.startsWith('All'))`, "engram's dashboard");
await js(`(()=>{ [...document.querySelectorAll('#chips button')].find(b=>b.textContent.startsWith('All')).click();
  document.querySelector('#groups details.group').open=true; return true })()`);
// A row's body is built when it is opened, on the toggle event, which fires later.
await waitFor(`!!document.querySelector('#groups details.group details.cfg')`, "the first configuration");
await js(`(document.querySelector('#groups details.group details.cfg').open=true, window.scrollTo(0,0), true)`);
await sleep(400);
await shot(`${ENGRAM}/engram-dashboard.png`, byId("view"));

// ------------------------------------------------------------------ the public pages

await go(`${SERVER}/projects`);
await waitFor(`/projects?/.test(document.querySelector('.plist-info').textContent)`, "the catalogue");
await js(`(()=>{ const f=document.querySelector('.plist-filter input'); f.value='engram'; f.dispatchEvent(new Event('input')); window.scrollTo(0,0); return true })()`);
await shot(`${ENGRAM}/projects-with-engram.png`);

await go(`${SERVER}/log`);
await waitFor(`/All|newest/.test(${byId("count")}.textContent)`, "the log", 60000);
// The entries name the anchor as its URL. engram's development anchor is a
// loopback address, so that is what is searched for here; a published project's
// URL carries its own domain.
await js(`(()=>{ const f=document.querySelector('.plist-filter input'); f.value=${JSON.stringify(process.env.ENGRAM_ANCHOR || "127.0.0.1:8728")};
  f.dispatchEvent(new Event('input')); const s=document.querySelectorAll('.plist-select select')[1]; s.value='10'; s.dispatchEvent(new Event('change')); return true })()`);
await shot(`${ENGRAM}/log-with-engram.png`, `document.querySelector('.plist')`);

ws.close();

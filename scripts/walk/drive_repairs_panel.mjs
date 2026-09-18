// Walks the repair panel in the real window, in a chosen language.
//
// `drive_window.mjs` walks the published path; this walks the panel that
// greets somebody at start when a recorded fix wants another look — Watch,
// Undo and Keep — and reads it in the language the picker is set to rather
// than the one the binary happens to speak.
//
// The language matters more here than anywhere else in the client. These
// panels are the only place a person meets the repair record, and the first
// walk of them read English only. `PODSHL_LANG` is not the switch: the window
// chooses, so this drives `#lang` and its own `onchange`, which is what a
// person clicking the picker does.
//
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
//   $env:VS_ROOT = '<a seeded state directory>'
//   Start-Process .\target\debug\podshl-client.exe
//   node scripts\walk\drive_repairs_panel.mjs de
//
// Exits non-zero and says what it saw when a step does not hold.
const LANG = process.argv[2] || "de";
const PORT = process.env.CDP_PORT || "9333";
const sleep = ms => new Promise(r => setTimeout(r, ms));

async function target() {
  for (let i = 0; i < 60; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      const page = list.find(t => t.type === "page");
      if (page) return page;
    } catch {}
    await sleep(500);
  }
  throw new Error("no WebView2 page on the debugging port");
}

const page = await target();
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
const lines = [];
ws.onmessage = ev => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  if (m.method === "Runtime.consoleAPICalled")
    lines.push(m.params.type + ": " + m.params.args.map(a => a.value ?? a.description).join(" "));
  if (m.method === "Runtime.exceptionThrown")
    lines.push("EXCEPTION: " + JSON.stringify(m.params.exceptionDetails.exception?.description
                                              || m.params.exceptionDetails.text));
};
const send = (method, params = {}) => new Promise(res => {
  const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params }));
});
await send("Runtime.enable");
await send("Page.enable");

async function js(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails)
    throw new Error("page: " + JSON.stringify(r.result.exceptionDetails.exception?.description));
  return r.result?.result?.value;
}
async function waitFor(expr, what, ms = 20000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try { if (await js(expr)) return; } catch {}
    await sleep(150);
  }
  throw new Error(`timed out waiting for ${what}\nconsole:\n${lines.join("\n")}`);
}

const steps = [];
function check(what, ok, saw) {
  steps.push({ what, ok: !!ok, saw });
  console.log(`${ok ? "  ok  " : "  NO  "} ${what}${ok ? "" : `\n         saw: ${JSON.stringify(saw)}`}`);
}

// The window is up and has drawn something.
await waitFor(`!!document.getElementById('lang')`, "the window");

// Set the language the way a person does: the picker, and its own handler.
await js(`(async () => {
  const s = document.getElementById('lang');
  s.value = ${JSON.stringify(LANG)};
  s.dispatchEvent(new Event('change'));
})()`);
await sleep(1500);
check(`the picker is on ${LANG}`, await js(`document.getElementById('lang').value`) === LANG,
      await js(`document.getElementById('lang').value`));

// The repair panels are drawn at start. Give them their review call.
await waitFor(`[...document.querySelectorAll('#main .panel.repair')].length > 0`, "a repair panel");

const table = await js(`(async () => (await (await fetch('i18n/${LANG}.json')).json()))()`);
const t = k => table[k];

const panelText = () => js(`[...document.querySelectorAll('#main .panel.repair')]
  .map(p => p.textContent.replace(/\\s+/g,' ').trim()).join('\\n---\\n')`);

let text = await panelText();
check("the panel heading is in the chosen language",
      text.includes(t("repairs_h")), text.slice(0, 200));
check("the flag that brought it up is named",
      text.includes(t("repair_file_changed")), text.slice(0, 300));
check("the buttons are in the chosen language",
      text.includes(t("repair_keep")) && text.includes(t("undo")), text.slice(0, 400));
// The handover's case: a watched issue that turned out to be closed says so,
// in the chosen language, on the panel that greets somebody at start.
check("a closed upstream issue is said, in the chosen language",
      text.includes(t("repair_issue_closed")), text.slice(0, 400));

// --- Watch, and the sentence that comes with it -------------------------
const hasWatch = await js(`!!document.querySelector('#main .panel.repair .watch')`);
check("a record with an upstream issue offers Watch", hasWatch, hasWatch);
if (hasWatch) {
  const before = await js(`document.querySelector('#main .panel.repair .watch').textContent.trim()`);
  check("Watch is offered in the chosen language", before === t("repair_watch"), before);
  const noteBefore = await js(`(document.querySelector('#main .panel.repair p.note.c-muted')||{}).textContent`);
  check("what watching costs is said before it is switched on",
        (noteBefore || "").includes(t("repair_watch_note").slice(0, 40)), noteBefore);

  await js(`document.querySelector('#main .panel.repair .watch').click()`);
  await sleep(2500);
  const after = await js(`document.querySelector('#main .panel.repair .watch').textContent.trim()`);
  check("Watch becomes Stop watching", after === t("repair_unwatch"), after);
  const noteAfter = await js(`(document.querySelector('#main .panel.repair p.note.c-muted')||{}).textContent`);
  check("and the sentence changes with it",
        (noteAfter || "").includes(t("repair_watching_note").slice(0, 40)), noteAfter);
}

// --- Undo, and the file it puts back ------------------------------------
const hasUndo = await js(`!!document.querySelector('#main .panel.repair .undo')`);
check("a record with a copy offers Undo", hasUndo, hasUndo);
if (hasUndo) {
  const label = await js(`document.querySelector('#main .panel.repair .undo').textContent.trim()`);
  check("Undo is offered in the chosen language", label === t("undo"), label);
  await js(`document.querySelector('#main .panel.repair .undo').click()`);
  await sleep(2500);
  text = await panelText();
  check("the panel says the change was taken back",
        text.includes(t("undone_h")) && text.includes(t("undone_1")), text.slice(0, 300));
}

console.log("\n" + JSON.stringify({ lang: LANG, steps }, null, 1));
const bad = steps.filter(s => !s.ok);
if (bad.length) {
  console.error(`\n${bad.length} of ${steps.length} did not hold`);
  process.exit(1);
}
console.log(`\nall ${steps.length} held`);
process.exit(0);

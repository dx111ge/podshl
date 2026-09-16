// Records the three PODSHL screencasts from the real surfaces, with subtitles.
//
//   VIDEO=user        the client window, from a question to a receipt
//   VIDEO=maintainer  the operator's pages: publish, register, dashboard, log
//   VIDEO=short       sixty seconds, vertical, the moments that matter
//
// Nothing is mocked. The client is the shipped window over WebView2's DevTools
// protocol (DOM events only, nothing typed at the OS level), the operator pages
// are served by the running operator, and every number on screen is what the
// running system answered. Frames come from `Page.startScreencast`, arrive only
// when something changes, and are timed into a constant-rate H.264 by ffmpeg.
//
// Needs Node 22+, ffmpeg on PATH, the stack up with engram enrolled, and:
//   user, short:  the client started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=
//                 --remote-debugging-port=9333 (see scripts/walk/drive_window.mjs),
//                 ENGRAM_DIR (a folder with engram in it, not on PATH) and
//                 LOG_FILE (examples/engram/harness/fixtures/ollama-server.log)
//   maintainer:   Chrome started with --remote-debugging-port=9334, PROJECT_TOKEN
//                 for the seeded your-project.example (scripts/dev/seed_large_dashboard.py)
// UI_LANG=de records the German cut (client window and subtitles; the operator's
// pages exist in English only) as *-de.mp4. OUT is the directory it is written to.
import { writeFileSync, mkdirSync, rmSync, readFileSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";

const VIDEO = process.env.VIDEO || "user";
// Absolute: ffmpeg reads the frame list relative to the list's own directory, so
// a relative OUT named every frame twice over and the encode found none.
const OUT = resolve(process.env.OUT || ".");
const FRAMES = join(OUT, `.frames-${VIDEO}`);
const PORT = process.env.CDP_PORT || (VIDEO === "maintainer" ? "9334" : "9333");
const SERVER = process.env.PODSHL_SERVER_URL || "http://127.0.0.1:8725";
const sleep = ms => new Promise(r => setTimeout(r, ms));

rmSync(FRAMES, { recursive: true, force: true });
mkdirSync(FRAMES, { recursive: true });

// ------------------------------------------------------------------ plumbing

async function connect() {
  for (let i = 0; i < 60; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      const page = list.find(t => t.type === "page");
      if (page) return page;
    } catch {}
    await sleep(500);
  }
  throw new Error(`nothing to record on port ${PORT}`);
}
const page = await connect();
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
const frames = [];
ws.onmessage = ev => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  if (m.method === "Page.screencastFrame") {
    const n = frames.length;
    const file = join(FRAMES, `f${String(n).padStart(5, "0")}.jpg`);
    writeFileSync(file, Buffer.from(m.params.data, "base64"));
    frames.push({ file, t: m.params.metadata.timestamp });
    send("Page.screencastFrameAck", { sessionId: m.params.sessionId });
  }
};
const send = (method, params = {}) => new Promise(res => {
  const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params }));
});
async function js(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error("page: " + JSON.stringify(r.result.exceptionDetails.exception?.description));
  return r.result?.result?.value;
}
async function waitFor(expr, what, ms = 20000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try { if (await js(expr)) return; } catch {}
    await sleep(120);
  }
  throw new Error(`timed out waiting for ${what}`);
}
await send("Runtime.enable");
await send("Page.enable");

// The subtitle, drawn into the page with CSSOM — which a page's CSP permits,
// where an inline style attribute would be blocked.
const SUB = `(()=>{ if(window.__sub) return true;
  const el=document.createElement('div'); el.id='__sub';
  const s=el.style; s.position='fixed'; s.left='50%'; s.transform='translateX(-50%)';
  s.bottom=(window.__subBottom||'96px'); s.maxWidth='88%'; s.padding='14px 26px';
  s.borderRadius='12px'; s.background='rgba(10,14,16,.92)'; s.color='#fff';
  s.font='600 '+(window.__subSize||'22px')+'/1.35 Segoe UI, system-ui, sans-serif';
  s.textAlign='center'; s.zIndex='2147483647'; s.pointerEvents='none';
  s.boxShadow='0 8px 30px rgba(0,0,0,.35)'; s.transition='opacity .25s ease'; s.opacity='0';
  document.body.appendChild(el);
  window.__sub=t=>{ el.style.opacity='0'; setTimeout(()=>{ el.textContent=t; el.style.opacity=t?'1':'0'; },200); };
  return true })()`;
async function sub(text, hold = 2600) {
  await js(SUB);
  await js(`window.__sub(${JSON.stringify(text)})`);
  await sleep(hold);
}
const lastPanel = `[...document.querySelectorAll('#main > div')].pop()`;
// A subtitle belongs to what it was said over. Faded before the view moves,
// or it reads as a caption for whatever scrolls in underneath it.
const fade = () => js(`(window.__sub && window.__sub(''), true)`);
async function scrollTo(expr) {
  await fade();
  await js(`(()=>{ const e=${expr}; if(e) e.scrollIntoView({block:'center', behavior:'smooth'}); return !!e })()`);
  await sleep(900);
}

// The window on screen resized to the frame being recorded. An override the
// window does not match paints the page into part of it and leaves the rest
// blank, which is what anyone watching the recording sees. Windows only, like
// the client under WebView2.
function fitWindow(width, height) {
  const ps = `
Add-Type @'
using System; using System.Runtime.InteropServices;
public class Fit { [StructLayout(LayoutKind.Sequential)] public struct R { public int L,T,Ri,B; }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out R r);
[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f); }
'@
$p = Get-Process podshl-client | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1
if (-not $p) { exit 2 }
$h = $p.MainWindowHandle; $w = New-Object Fit+R; $c = New-Object Fit+R
[Fit]::GetWindowRect($h, [ref]$w) | Out-Null; [Fit]::GetClientRect($h, [ref]$c) | Out-Null
$s = [Fit]::GetDpiForWindow($h) / 96
$ow = [int]([math]::Round(${width} * $s)) + ($w.Ri - $w.L) - $c.Ri
$oh = [int]([math]::Round(${height} * $s)) + ($w.B - $w.T) - $c.B
[Fit]::SetWindowPos($h, [IntPtr]::Zero, $w.L, $w.T, $ow, $oh, 0x0014) | Out-Null`;
  const r = spawnSync("pwsh", ["-NoProfile", "-Command", ps], { stdio: "inherit" });
  if (r.status !== 0) console.error("note: could not resize the client window; the frame is still recorded");
}

async function record(size, body) {
  // A recording needs a fixed frame, and this is the one place that is worth
  // an override. **WebView2 will not undo one** — `clearDeviceMetricsOverride`,
  // a 0x0 override and `resetPageScaleFactor` are all ignored — so the window
  // this leaves behind renders at `size` until the process is restarted.
  // Acceptable here and nowhere else: recording is a deliberate act on a
  // window nobody is trying to use, and it says so rather than surprising
  // whoever finds the window afterwards.
  console.error("note: WebView2 cannot undo a size override — restart the client "
                + "before using this window again");
  if (VIDEO !== "maintainer") fitWindow(size.width, size.height);
  await send("Emulation.setDeviceMetricsOverride", size);
  await sleep(400);
  await send("Page.startScreencast", { format: "jpeg", quality: 88, everyNthFrame: 1 });
  try { await body(); } finally {
    await sleep(800);
    await send("Page.stopScreencast");
    frames.push({ file: null, t: Date.now() / 1000 });
  }
}

function encode(name, w, h) {
  // Frames arrive only on change, so each one lasts until the next.
  const lines = [];
  for (let i = 0; i < frames.length - 1; i++) {
    if (!frames[i].file) continue;
    const d = Math.max(0.02, frames[i + 1].t - frames[i].t);
    lines.push(`file '${frames[i].file.replace(/\\/g, "/").replace(/'/g, "'\\''")}'`, `duration ${d.toFixed(3)}`);
  }
  const lastReal = [...frames].reverse().find(f => f.file);
  lines.push(`file '${lastReal.file.replace(/\\/g, "/")}'`);
  const list = join(FRAMES, "list.txt");
  writeFileSync(list, lines.join("\n"));
  const out = join(OUT, name);
  const r = spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", list,
    "-vf", `scale=${w}:${h}:force_original_aspect_ratio=decrease,pad=${w}:${h}:(ow-iw)/2:(oh-ih)/2:color=0x0E1315,fps=30,format=yuv420p`,
    "-c:v", "libx264", "-preset", "slow", "-crf", "26", "-movflags", "+faststart", out], { stdio: "inherit" });
  if (r.status !== 0) throw new Error("ffmpeg failed");
  const seconds = frames[frames.length - 1].t - frames[0].t;
  console.log(`${out}  ${frames.length - 1} frames, ${seconds.toFixed(1)} s`);
  rmSync(FRAMES, { recursive: true, force: true });
}

// ------------------------------------------------------------------ the client

async function clientReady(lang) {
  await waitFor(`document.getElementById('go') && document.getElementById('go').textContent.length > 0`, "boot");
  await js(`(()=>{ const s=document.getElementById('lang'); s.value='${lang}'; s.onchange(); return true })()`);
  await waitFor(`document.getElementById('go').textContent === t('go')`, "the language");
  await waitFor(`window.__TAURI__.core.invoke('index_status').then(s => s.have && s.entries > 0)`, "index", 20000);
}

/** The published path, paced for a person to follow. `beat` scales every hold. */
async function clientStory(beat, lines) {
  const hold = ms => Math.round(ms * beat);
  await sub(lines.intro, hold(3200));
  await js(`(()=>{ const p=document.getElementById('problem'); p.value=''; return true })()`);
  const problem = "Search in engram returns nothing, and it used to work";
  for (let i = 1; i <= problem.length; i += 3) {
    await js(`document.getElementById('problem').value=${JSON.stringify(problem)}.slice(0,${i}), true`);
    await sleep(35);
  }
  await js(`document.getElementById('problem').value=${JSON.stringify(problem)}, true`);
  await js(`document.getElementById('vquery').value='engram', true`);
  await sub(lines.ask, hold(2400));
  await js(`document.getElementById('go').click(), true`);

  await waitFor(`!!document.querySelector('input.pc')`, "the class picker");
  await js(`(()=>{ const r=document.querySelector('input.pc'); if(r) r.checked=true; return true })()`);
  await scrollTo(`document.querySelector('input.pc')`);
  await sub(lines.found, hold(3200));
  await js(`document.querySelector('input.pc').closest('.panel').querySelector('.allow').click(), true`);

  await waitFor(`${lastPanel}.querySelectorAll('.rp').length > 0`, "the read consent");
  await scrollTo(`${lastPanel}.querySelector('.rp')`);
  await sub(lines.consent, hold(3400));
  await scrollTo(`[...${lastPanel}.querySelectorAll('.rp')].find(c=>c.dataset.id==='engram.version')`);
  await sub(lines.version, hold(3400));
  await js(`${lastPanel}.querySelector('.allow').click(), true`);

  await waitFor(`${lastPanel}.querySelector('.locp') && ${lastPanel}.textContent.includes('engram')`, "where is engram");
  await scrollTo(`${lastPanel}.querySelector('.locp')`);
  await sub(lines.where, hold(3200));
  const dir = process.env.ENGRAM_DIR;
  for (let i = 1; i <= dir.length; i += 2) {
    await js(`${lastPanel}.querySelector('.locp').value=${JSON.stringify(dir)}.slice(0,${i}), true`);
    await sleep(30);
  }
  await js(`${lastPanel}.querySelector('.locp').value=${JSON.stringify(dir)}, true`);
  await sleep(hold(600));
  await js(`${lastPanel}.querySelector('.allow').click(), true`);
  await waitFor(`[...document.querySelectorAll('#main > div.done h3')].some(h=>h.textContent.includes(t('loc_ok_h',{p:'engram'})))`, "engram found");
  await scrollTo(`[...document.querySelectorAll('#main > div.done')].pop()`);
  await sub(lines.read, hold(3200));
  if (await js(`!!(${lastPanel}.querySelector('.locp') && ${lastPanel}.textContent.includes('ollama'))`)) {
    await js(`${lastPanel}.querySelector('.deny').click(), true`);
  }

  await waitFor(`!!document.getElementById('qaok')`, "the questions");
  await js(`(()=>{ const q=document.getElementById('qaok').closest('.panel');
    const s=q.querySelector('.ai[data-id="engram.symptom"]'); if(s) s.value='search returns nothing, and it used to work';
    q.querySelector('[data-pick=file][data-for="ollama.log"]').click();
    q.querySelector('[data-file-for="ollama.log"] .logpath').value=${JSON.stringify(process.env.LOG_FILE)};
    return true })()`);
  await scrollTo(`document.querySelector('.ai[data-id="ollama.log"]')`);
  await sub(lines.log, hold(3000));
  await js(`document.querySelector('[data-load=file][data-for="ollama.log"]').click(), true`);
  await waitFor(`(document.querySelector('.ai[data-id="ollama.log"]')||{value:''}).value.includes('not found')`, "the log");
  await sleep(hold(1600));
  await js(`document.getElementById('qaok').click(), true`);

  // Nothing leaves before this panel: what goes to the operator, and why there.
  await waitFor(`!!document.getElementById('pubsend')`, "the send consent");
  await scrollTo(`document.querySelector('#pubsend p')`);
  await sub(lines.send, hold(3800));
  await js(`document.getElementById('pubsend').querySelector('.allow').click(), true`);
  await js(`window.__sub('')`);

  await waitFor(`[...document.querySelectorAll('#main h3')].some(h=>h.textContent===t('pub_found_h'))`, "the answer");
  await scrollTo(`[...document.querySelectorAll('#main h3')].find(h=>h.textContent===t('pub_found_h'))`);
  await sub(lines.answer, hold(3600));
  await waitFor(`!!document.querySelector('[data-att-proof=ok]')`, "the log proof", 20000);
  await scrollTo(`document.querySelector('[data-att-proof]')`);
  await sub(lines.proof, hold(3600));
  await scrollTo(`document.querySelector('.answer pre.md')`);
  await sub(lines.fix, hold(3000));

  await waitFor(`!!${lastPanel}.querySelector('button[data-o=resolved]')`, "did it help");
  await scrollTo(lastPanel);
  await sub(lines.helped, hold(2800));
  await js(`${lastPanel}.querySelector('button[data-o=resolved]').click(), true`);
  await js(`window.__sub('')`);

  await waitFor(`${lastPanel}.querySelector('.allow') && ${lastPanel}.textContent.includes(t('report_what'))`, "the report consent");
  await js(`${lastPanel}.querySelector('details').open=true, true`);
  await scrollTo(`${lastPanel}.querySelector('details')`);
  await sub(lines.report, hold(3600));
  await js(`${lastPanel}.querySelector('.allow').click(), true`);
  await js(`window.__sub('')`);

  await waitFor(`${lastPanel}.querySelectorAll('textarea.ft').length > 0`, "the free text");
  await scrollTo(`${lastPanel}.querySelector('textarea.ft')`);
  await sub(lines.anon, hold(4000));
  await scrollTo(`[...${lastPanel}.querySelectorAll('textarea.ft')].pop()`);
  await sub(lines.anon2, hold(3600));
  await js(`${lastPanel}.querySelector('.allow').click(), true`);
  await js(`window.__sub('')`);

  await waitFor(`[...document.querySelectorAll('#main h3')].some(h=>h.textContent===t('pub_receipt_h'))`, "the receipt");
  await scrollTo(lastPanel);
  await sub(lines.receipt, hold(3600));
  await sub(lines.outro, hold(3400));
  await js(`window.__sub('')`);
}

const USER_LINES = {
  intro: "PODSHL — support that reads the machine, not the ticket",
  ask: "Something is broken. Say what, and name the software.",
  found: "engram publishes answers — two files in its own repository",
  consent: "Every reading is shown first, and chosen item by item",
  version: "Which engram? The program is asked — not the person",
  where: "engram is not on the search path. Nothing is searched: the user is asked where it is",
  read: "Only engram --version runs. Only the number is kept: 1.2.2",
  log: "The log that matters can be loaded — and stays on this machine",
  send: "Before anything leaves: where it goes, and why there",
  answer: "The maintainer's own answer, matched by rules. No model involved",
  proof: "Checked here: the signed log vouches for exactly these files",
  fix: "The fix, as the maintainer wrote it",
  helped: "Did it work? Only the user can say — so the user is asked",
  report: "The report: what was measured, what was said, and nothing more",
  anon: "Free text is anonymised before anybody is asked to send it",
  anon2: "Addresses, user names, tokens and times — replaced, and counted",
  receipt: "Five independent people before a maintainer sees anything",
  outro: "The data never moves. The instructions cannot go stale.",
};

const USER_LINES_DE = {
  intro: "PODSHL — Support, der die Maschine liest, nicht das Ticket",
  ask: "Etwas geht nicht. Sag, was — und nenne die Software.",
  found: "engram veröffentlicht Antworten — zwei Dateien im eigenen Repository",
  consent: "Jede Auslesung wird zuerst gezeigt und einzeln ausgewählt",
  version: "Welches engram? Das Programm wird gefragt — nicht der Mensch",
  where: "engram liegt nicht im Suchpfad. Nichts wird durchsucht: Du wirst gefragt, wo es liegt",
  read: "Nur engram --version läuft. Nur die Nummer bleibt: 1.2.2",
  log: "Das entscheidende Log lässt sich laden — und bleibt auf diesem Rechner",
  send: "Bevor etwas den Rechner verlässt: wohin es geht, und warum dorthin",
  answer: "Die Antwort des Maintainers, nach Regeln gefunden — übersetzt von deinem eigenen Modell",
  proof: "Hier geprüft: Das signierte Log bürgt für genau diese Dateien",
  fix: "Befehle und engrams eigene Begriffe bleiben, wie engram sie schreibt",
  helped: "Hat es geholfen? Nur du kannst das sagen — also wirst du gefragt",
  report: "Der Bericht: was gemessen wurde, was du gesagt hast, und sonst nichts",
  anon: "Freitext wird anonymisiert, bevor du gefragt wirst, ob er gesendet wird",
  anon2: "Adressen, Benutzernamen, Tokens und Zeiten — ersetzt und gezählt",
  receipt: "Erst ab fünf unabhängigen Menschen sieht ein Maintainer etwas",
  outro: "Die Daten bleiben, wo sie sind. Die Anleitung kann nicht veralten.",
};

const SHORT_LINES = {
  intro: "Support that reads the machine — with permission",
  ask: "Name what is broken",
  found: "The project published the answers itself",
  consent: "Every reading: shown, then chosen",
  version: "The version comes from the program",
  where: "Not on the path? You are asked. Nothing is scanned",
  read: "engram 1.2.2 — measured, exactly",
  log: "Logs stay on your machine",
  send: "Nothing leaves until you say send",
  answer: "The maintainer's answer. No AI guessing",
  proof: "Verified against a public signed log",
  fix: "The fix, in their words",
  helped: "Did it help? You decide",
  report: "Only what you allow travels",
  anon: "Anonymised before you are asked",
  anon2: "IPs, names, tokens — replaced",
  receipt: "Counted only from five people up",
  outro: "PODSHL — the data never moves",
};

const SHORT_LINES_DE = {
  intro: "Support, der die Maschine liest — mit Erlaubnis",
  ask: "Sag, was nicht geht",
  found: "Das Projekt hat die Antworten selbst veröffentlicht",
  consent: "Jede Auslesung: gezeigt, dann gewählt",
  version: "Die Version kommt vom Programm",
  where: "Nicht im Pfad? Du wirst gefragt. Nichts wird durchsucht",
  read: "engram 1.2.2 — gemessen, exakt",
  log: "Logs bleiben auf deinem Rechner",
  send: "Nichts geht raus, bevor du sendest",
  answer: "Die Antwort des Maintainers. Kein KI-Raten",
  proof: "Geprüft gegen ein öffentliches, signiertes Log",
  fix: "Befehle bleiben Befehle — auch übersetzt",
  helped: "Hat es geholfen? Du entscheidest",
  report: "Nur was du erlaubst, wird gesendet",
  anon: "Anonymisiert, bevor du gefragt wirst",
  anon2: "IPs, Namen, Tokens — ersetzt",
  receipt: "Gezählt erst ab fünf Menschen",
  outro: "PODSHL — die Daten bleiben, wo sie sind",
};

// ------------------------------------------------------------------ the operator pages

const MAINTAINER_LINES = {
  en: {
    intro: "PODSHL for maintainers: static files, no server, no model",
    why: "Your users' problems usually never reach you. This is how they do",
    publish: "What a maintainer publishes: .podshl/agent.yaml and a few solutions",
    build: "Or build the files from a form — no YAML to learn",
    example: "What to read on the user's machine, and when an answer applies",
    check: "Checked by the same code that ingests them — and kept nowhere",
    load: "Already publishing? Load your files — what the form cannot edit is kept as written",
    register: "Registering proves control of the location — nothing else",
    signin: "The dashboard: private, free, about your own project only",
    triage: "263 recurring configurations — grouped by the file you would edit, work first",
    fork: "It helped some people and not others: the fact that tells them apart",
    paste: "…and the condition to paste into your own solution file",
    files: "What the mirror could not make of your files, in terms of what you wrote",
    lists: "Every list pages, filters and sorts — on your machine",
    log: "Every attestation is in a public, signed transparency log",
    outro: "Your users' problems, measured on their machines — reaching you",
  },
  de: {
    intro: "PODSHL für Maintainer: statische Dateien, kein Server, kein Modell",
    why: "Die Probleme deiner Nutzer erreichen dich meist nie. So erreichen sie dich",
    publish: "Was ein Maintainer veröffentlicht: .podshl/agent.yaml und ein paar Lösungen",
    build: "Oder die Dateien aus einem Formular bauen — ohne YAML zu lernen",
    example: "Was auf dem Rechner gelesen wird, und wann eine Antwort gilt",
    check: "Geprüft vom selben Code, der sie einliest — und nirgends gespeichert",
    load: "Schon veröffentlicht? Dateien laden — was das Formular nicht kennt, bleibt, wie es war",
    register: "Die Registrierung beweist die Kontrolle über den Ort — sonst nichts",
    signin: "Das Dashboard: privat, kostenlos, nur über dein eigenes Projekt",
    triage: "263 wiederkehrende Konfigurationen — nach der Datei gruppiert, die du ändern würdest",
    fork: "Es half manchen und anderen nicht: der Wert, der sie trennt",
    paste: "…und die Bedingung zum Einfügen in deine eigene Lösungsdatei",
    files: "Was der Mirror mit deinen Dateien nicht anfangen konnte — in deinen Begriffen",
    lists: "Jede Liste blättert, filtert und sortiert — auf deinem Rechner",
    log: "Jede Beglaubigung steht in einem öffentlichen, signierten Log",
    outro: "Die Probleme deiner Nutzer, gemessen auf ihren Rechnern — bei dir",
  },
};

async function maintainerStory(lines) {
  const go = async url => {
    await send("Page.navigate", { url });
    await sleep(1400);
    await js(`(window.__subBottom='40px', true)`);
  };
  const scrollToEl = async (expr, block = "center", ms = 1300) => {
    await fade();
    await js(`(()=>{ const e=${expr}; if(e) e.scrollIntoView({block:'${block}', behavior:'smooth'}); return !!e })()`);
    await sleep(ms);
  };
  const byId = id => `document.getElementById(${JSON.stringify(id)})`;

  await go(`${SERVER}/`);
  await sub(lines.intro, 3200);
  await sub(lines.why, 3400);

  await go(`${SERVER}/publish`);
  await sub(lines.publish, 3200);

  // The builder, filled with its own example and checked against the mirror.
  await go(`${SERVER}/publish/build`);
  await js(`(localStorage.removeItem('podshl.build.draft'), true)`);
  await go(`${SERVER}/publish/build`);
  await waitFor(`${byId("catalogue")}.options.length > 1`, "the vocabulary");
  await sub(lines.build, 2800);
  await js(`${byId("example")}.click(), true`);
  await scrollToEl(`document.querySelector('#probes .card')`, "start");
  await sub(lines.example, 3200);
  await js(`${byId("check")}.click(), true`);
  await waitFor(`/mirror would/.test(${byId("verdict")}.innerText)`, "the check");
  await scrollToEl(`${byId("verdict")}`);
  await sub(lines.check, 3400);

  // engram's real files, loaded as a maintainer would pick them.
  await scrollToEl(`${byId("load")}`);
  await js(`(()=>{ const dt=new DataTransfer();
    ${JSON.stringify(ENGRAM_FILES)}.forEach(f => dt.items.add(new File([f.text], f.name, {type:'text/plain'})));
    const i=${byId("load")}; i.files=dt.files; i.dispatchEvent(new Event('change')); return true })()`);
  await waitFor(`/^Loaded/.test(${byId("loaded")}.textContent)`, "the loaded files");
  await scrollToEl(`[...document.querySelectorAll('#probes .card')].find(c => [...c.querySelectorAll('input')].some(i => i.value === 'ollama.host'))`, "start");
  await sub(lines.load, 3800);
  await js(`(localStorage.removeItem('podshl.build.draft'), true)`);

  await go(`${SERVER}/register`);
  await sub(lines.register, 3000);

  // The dashboard of a project people use. The token is masked before it is
  // typed: a credential does not belong in a video, even a development one.
  await go(`${SERVER}/dashboard#${PROJECT_HOST}`);
  await js(`(()=>{ localStorage.clear(); ${byId("host")}.value=${JSON.stringify(PROJECT_HOST)};
    const t=${byId("token")}; t.type='password'; t.value=${JSON.stringify(process.env.PROJECT_TOKEN || "")}; return true })()`);
  await sub(lines.signin, 2800);
  await js(`${byId("open")}.click(), true`);
  await waitFor(`!${byId("view")}.hidden && document.querySelectorAll('#groups details.group').length > 0`, "the dashboard");
  await scrollToEl(`${byId("summary")}`, "start");
  await sub(lines.triage, 3800);

  await js(`(()=>{ document.querySelectorAll('#groups details.group').forEach(g => { g.open = true; }); return true })()`);
  await waitFor(`[...document.querySelectorAll('#groups details.group .panel.warn')].some(p => p.querySelector('pre'))`, "a fork");
  await js(`(()=>{ const keep=[...document.querySelectorAll('#groups details.group')].find(g => g.querySelector('.panel.warn pre'));
    document.querySelectorAll('#groups details.group').forEach(g => { if (g !== keep) g.open = false; }); return true })()`);
  const openFork = `[...document.querySelectorAll('#groups details.group')].find(d=>d.open)`;
  await scrollToEl(`${openFork}.querySelector('.panel.warn')`, "start");
  await sub(lines.fork, 3400);
  await scrollToEl(`${openFork}.querySelector('.panel.warn pre')`);
  await sub(lines.paste, 3200);

  await js(`${byId("tab-files")}.click(), true`);
  await scrollToEl(`document.querySelector('.tabs')`, "start");
  await sub(lines.files, 3600);

  await go(`${SERVER}/log`);
  await waitFor(`/All|newest/.test(${byId("count")}.textContent)`, "the log", 60000);
  await sub(lines.log, 3000);
  await scrollToEl(`document.querySelector('.plist-pages')`);
  await sub(lines.lists, 2800);
  await sub(lines.outro, 3200);
  await sub("", 600);
}

// ------------------------------------------------------------------ run

const LANG = (process.env.UI_LANG || "en").toLowerCase();
const suffix = LANG === "en" ? "" : `-${LANG}`;
const PROJECT_HOST = process.env.PROJECT_HOST || "your-project.example";
const ENGRAM_FILES = VIDEO === "maintainer" ? (() => {
  const dir = "examples/engram/.podshl";
  return [{ name: "agent.yaml", text: readFileSync(join(dir, "agent.yaml"), "utf8") }]
    .concat(readdirSync(join(dir, "solutions")).map(n => ({ name: n, text: readFileSync(join(dir, "solutions", n), "utf8") })));
})() : [];

if (VIDEO === "user") {
  await clientReady(LANG);
  await record({ width: 1280, height: 800, deviceScaleFactor: 1, mobile: false },
               () => clientStory(1, LANG === "de" ? USER_LINES_DE : USER_LINES));
  encode(`podshl-user${suffix}.mp4`, 1280, 800);
} else if (VIDEO === "short") {
  await clientReady(LANG);
  await js(`window.__subBottom='120px'; window.__subSize='30px'; true`);
  await record({ width: 540, height: 960, deviceScaleFactor: 2, mobile: false },
               () => clientStory(0.5, LANG === "de" ? SHORT_LINES_DE : SHORT_LINES));
  encode(`podshl-short${suffix}.mp4`, 1080, 1920);
} else if (VIDEO === "maintainer") {
  // The operator's pages exist in English only; the subtitles follow UI_LANG.
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  await record({ width: 1280, height: 800, deviceScaleFactor: 1, mobile: false },
               () => maintainerStory(MAINTAINER_LINES[LANG] || MAINTAINER_LINES.en));
  encode(`podshl-maintainer${suffix}.mp4`, 1280, 800);
} else {
  throw new Error(`VIDEO=${VIDEO} — one of user, maintainer, short`);
}
ws.close();

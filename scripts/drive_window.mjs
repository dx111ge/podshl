// Walks the real PODSHL window through the published path, and photographs it.
//
// The client has had contract tests on its window for a long time and nobody
// had looked at a panel past the first screen, because GUI automation with
// synthetic keystrokes collides with whoever is at the keyboard. This does not
// type at the OS level at all: it talks to the window's own web view over the
// DevTools protocol and works with the DOM. The window appears on screen while
// it runs; nothing is sent to it that a person could be interrupted by.
//
// The first walk found a report path that could not deliver, a consent screen
// in the wrong language, every inline style blocked by the window's own CSP and
// an answer that contradicted the machine it was given to — none of them
// visible to a check that reads files. Needs Node 22+ (built-in WebSocket).
//
// On Windows (WebView2):
//
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
//   $env:VS_ROOT = '..\var'; $env:VS_TRUST = '..\var\ans_stub.json'
//   $env:PODSHL_SERVER_URL = 'http://127.0.0.1:8725'
//   cd client-rs; cargo build; Start-Process .\target\debug\podshl-client.exe
//   $env:SHOTS = "$env:TEMP\podshl-shots"; $env:UI_LANG = 'de'
//   $env:ENGRAM_DIR = '<a folder containing engram.exe, not on PATH>'
//   $env:LOG_FILE = (Resolve-Path ..\examples\engram\harness\fixtures\ollama-server.log)
//   node ..\scripts\drive_window.mjs
//
// It needs the stack up with engram enrolled (examples/engram/README.md).
// `CLASS` and `SYMPTOM` choose the problem class and the answer given.
import { writeFileSync, mkdirSync } from "node:fs";

const PORT = process.env.CDP_PORT || "9333";
const LANGUAGE = process.env.UI_LANG || "en";
const OUT = process.env.SHOTS;
const ENGRAM_DIR = process.env.ENGRAM_DIR;
const LOG_FILE = process.env.LOG_FILE;
// `published` (the engram walk, the default), `vendor` (a vendor with a signed
// agent, the counterparty on :8721), `model` (a vendor with no agent and no
// published answer, so the person's own model leads — needs one set up), or
// `settings` (the model panel).
const SCENARIO = process.env.SCENARIO || "published";
if (!OUT || (SCENARIO === "published" && !LOG_FILE)) {
  console.error("SHOTS is required, and LOG_FILE for the published walk — see the header");
  process.exit(2);
}
// C3, at runtime: every panel that asks for consent must have the refusing
// button focused when it appears, so a stray Return can never grant. The
// contract test reads the source; this reads the window.
const focusChecks = [];
mkdirSync(OUT, { recursive: true });

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
const consoleLines = [];
ws.onmessage = ev => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  if (m.method === "Runtime.consoleAPICalled")
    consoleLines.push(m.params.type + ": " + m.params.args.map(a => a.value ?? a.description).join(" "));
  if (m.method === "Runtime.exceptionThrown")
    consoleLines.push("EXCEPTION: " + JSON.stringify(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text));
};
const send = (method, params = {}) => new Promise(res => {
  const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params }));
});
await send("Runtime.enable");
await send("Page.enable");

async function js(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error("page: " + JSON.stringify(r.result.exceptionDetails.exception?.description));
  return r.result?.result?.value;
}
const panels = () => js(`[...document.querySelectorAll('#main > div')].map(d =>
  ((d.querySelector('h3')||d.querySelector('.title')||{}).textContent||'').trim()).filter(Boolean)`);

async function waitFor(expr, what, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try { if (await js(expr)) return; } catch {}
    await sleep(150);
  }
  throw new Error(`timed out waiting for ${what}\npanels: ${JSON.stringify(await panels(), null, 1)}\nconsole:\n${consoleLines.join("\n")}`);
}

// German on a screen that is not German. The binary spoke German for a long
// time and nothing looked: every panel is read as it is photographed, and a
// line that looks German on an English, French or Spanish screen is reported.
// Text a person typed is theirs and is not checked.
const GERMAN = /[äöüÄÖÜß]|\b(nicht|keine?|wird|werden|oder|und|für|mit|eine[rn]?|ist|sind|Datei|Gerät|Schlüssel|Meldung|Befund|abgelehnt|gesperrt|lesen)\b/;
// And the other direction, which went unnoticed for exactly as long. The model
// answered in English on a German screen — the labels around it were German, so
// nothing looked wrong to a check that only hunts German — and a person reading
// their own language is the whole point of the four language files.
//
// Only function words, and at least three of them: a model's answer legitimately
// carries English product names, command lines and version strings, and a screen
// is not English because it says "NVIDIA GeForce". Panels a person typed into are
// skipped already.
const ENGLISH = /\b(the|is|are|was|were|and|or|not|with|from|this|that|your|you|should|would|could|which|there|because|driver|update|issue|problem|likely|suggests)\b/i;
// Words a screen language spells like an English function word. German "was"
// is "what", and a German sentence with three of them — "Nimm alles heraus, was
// dieses Gerät nicht verlassen soll. Was du siehst, ist genau das, was gesendet
// wird." — was reported as English on the German screen.
const ALSO_NATIVE = { de: new Set(["was"]) };
const germanLines = new Map();
const englishLines = new Map();
const brokenLines = new Map();
// A translated panel keeps the project's own words beside the translation,
// marked `lang="en"`, one click away behind "Original". Those are English on
// purpose and were reported as a language failure on every translated walk —
// 17 lines of them the first time one was walked in French. An untranslated
// panel carries no such mark (`biBlock` returns the original bare), so this
// cannot hide a translation that did not happen.
async function auditLanguage(where) {
  const lines = await js(`(()=>{ const out=[];
    const skip=el=>!el || el.closest("script,style,textarea,input,select,.q,code,pre,.v,[lang='en']");
    const walk=document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    let n; while((n=walk.nextNode())){ const s=n.textContent.trim();
      if(s && !skip(n.parentElement)) out.push(s); }
    for(const el of document.querySelectorAll("[title]")) if(el.title.trim()) out.push(el.title.trim());
    return out; })()`);
  for (const l of lines || []) {
    // A sentence with a hole in it: a placeholder nobody filled, or a value
    // that came through as null. The first French walk showed "read its
    // output: null null" where the tool and its arguments belonged.
    // `None` is Python's hole, and it reaches a screen the same way
    // `undefined` reaches one from the window: a value nobody had, formatted
    // into a sentence. "program version None" was shipping, in a sentence
    // that also claimed the answer had been resolved against that version.
    // Not flagged where the line *starts* with it: "None of it has left your
    // device" is a sentence rather than a hole.
    const nulled = /\bNone\b/.test(l) && !/^None\b/.test(l);
    if ((/\{\w+\}|\b(null|undefined)\b/.test(l) || nulled) && l.length > 12 && !brokenLines.has(l)) brokenLines.set(l, where);
    const words = l.split(/\s+/).filter(w => GERMAN.test(w));
    if (LANGUAGE !== "de" && (/[äöüÄÖÜß]/.test(l) || words.length >= 2)) {
      if (!germanLines.has(l)) germanLines.set(l, where);
    }
    const native = ALSO_NATIVE[LANGUAGE] || new Set();
    const english = l.split(/\s+/).filter(w => ENGLISH.test(w) &&
      !native.has(w.toLowerCase().replace(/[^\p{L}]/gu, "")));
    if (LANGUAGE !== "en" && english.length >= 3 && !englishLines.has(l)) {
      englishLines.set(l, where);
    }
  }
}

let shot = 0;
async function screenshot(name) {
  await sleep(350);
  const r = await send("Page.captureScreenshot", { format: "png" });
  const file = `${OUT}/${String(++shot).padStart(2, "0")}-${name}.png`;
  writeFileSync(file, Buffer.from(r.result.data, "base64"));
  console.log("  shot", file);
  await auditLanguage(name);
}
// A panel saying something went wrong is a failed walk, however quietly the
// walk reached its end. The first run of the language audit reported "OK"
// over a window whose very first step had thrown.
const failedPanels = () => js(`[...document.querySelectorAll("#main h3")].filter(h => h.textContent === t("err_h")).length`);

function reportLanguage() {
  console.log(`\nSentences with a hole in them: ${brokenLines.size ? "" : "none"}`);
  for (const [l, where] of brokenLines) console.log(`  ! [${where}] ${l.slice(0, 160)}`);
  let wrong = brokenLines.size;
  if (LANGUAGE !== "de") {
    console.log(`\nGerman on the ${LANGUAGE} screen: ${germanLines.size ? "" : "none"}`);
    for (const [l, where] of germanLines) console.log(`  ! [${where}] ${l.slice(0, 160)}`);
    wrong += germanLines.size;
  }
  if (LANGUAGE !== "en") {
    console.log(`\nEnglish on the ${LANGUAGE} screen: ${englishLines.size ? "" : "none"}`);
    for (const [l, where] of englishLines) console.log(`  ! [${where}] ${l.slice(0, 160)}`);
    wrong += englishLines.size;
  }
  return wrong;
}
const lastPanel = `[...document.querySelectorAll('#main > div')].pop()`;
// A question with choices arrives unanswered — no panel answers for the person
// looking at it — so the walk answers, as a careful user would: the first real
// option. Index 0 is "please choose". Called before any button is pressed.
const chooseAnswers = () => js(`(()=>{ const p=${lastPanel}; if(!p) return 0;
  const sel=[...p.querySelectorAll('select.field[data-id]')].filter(s=>s.options.length>1);
  sel.forEach(s=>{ if(!s.value) s.selectedIndex=1; });
  return sel.length })()`);
const step = s => console.log("·", s);

/* A walk that answers whatever the window asks, by fixed rules, until the
   window stops asking. Every panel is photographed; every consent panel has
   its focus checked before anything is clicked. The rules are what a careful
   user would do on a test machine: allow reading, answer questions with the
   first offered choice, send the diagnosis, allow a change (it is dry-run and
   undoable), report, and decline free text and index contributions. */
async function walkUntilQuiet(label) {
  const seen = new Set();
  let idle = 0, n = 0, spinning = 0;
  while (idle < 25 && n < 40) {
    await sleep(300);
    const info = await js(`(()=>{ const p=${lastPanel}; if(!p) return null;
      const h=(p.querySelector('h3')||p.querySelector('.title')||{}).textContent||'';
      const btns=[...p.querySelectorAll('button:not([disabled])')];
      return {h:h.trim(), key:h.trim()+'|'+btns.length+'|'+document.querySelectorAll('#main > div').length,
        decide:p.classList.contains('decide'), deny:!!p.querySelector('.deny:not([disabled])'),
        focusDeny: !!(document.activeElement && document.activeElement.classList.contains('deny') && p.contains(document.activeElement)),
        rp:!!p.querySelector('.rp'), qa:!!p.querySelector('#qaok:not([disabled])'),
        allow:!!p.querySelector('.allow:not([disabled])'), outcome:!!p.querySelector('button[data-o]:not([disabled])'),
        free:!!p.querySelector('textarea.ft'), contrib:p.textContent.includes(t('contrib_h')),
        loc:!!p.querySelector('.locp'), spin:!!p.querySelector('.spin'),
        hq:!!p.querySelector('#hqok:not([disabled])'), fu:!!p.querySelector('.fudone:not([disabled])'),
        rq:!!p.querySelector('#rqok:not([disabled])'),
        ch:!!p.querySelector('#chok:not([disabled])'),
        more:[...p.querySelectorAll('button')].some(b=>b.textContent===t('more_no') && !b.disabled)} })()`);
    // A spinner is the window working — a model thinking can take a minute —
    // and is waited out rather than counted as the window having gone quiet.
    if (info && info.spin) { if (++spinning > 600) break; continue; }
    if (!info || seen.has(info.key)) { idle++; continue; }
    seen.add(info.key); idle = 0; n++;
    await screenshot(`${label}-${String(n).padStart(2, "0")}`);
    console.log(`  panel: ${info.h}`);
    if (info.decide && info.deny) {
      focusChecks.push({ panel: info.h, ok: info.focusDeny });
      if (!info.focusDeny) console.log(`  ! the refusing button does not hold focus on "${info.h}"`);
    }
    if (info.ch) {
      // "What changed?" — asked by the window rather than by the model, first
      // and on every free-running diagnosis, because it is the question a
      // person can always answer and no tool ever can.
      await js(`(()=>{ const q=${lastPanel};
        q.querySelectorAll('.hq').forEach(i=>{ i.value=${JSON.stringify(process.env.CHANGED || "The graphics driver updated itself two days ago.")}; });
        q.querySelector('#chok').click(); return true })()`);
    } else if (info.hq) {
      // The model's own questions: what only a person knows. Answered plainly.
      // Scoped to the newest panel: every round's panel has its own `#hqok`,
      // and the document-wide lookup found the first, long since disabled.
      await js(`(()=>{ const q=${lastPanel};
        q.querySelectorAll('.hq').forEach(i=>{ i.value=${JSON.stringify(process.env.ANSWER || "It started after the last driver update; no overclocking.")}; });
        q.querySelector('#hqok').click(); return true })()`);
    } else if (info.more) {
      // Another round is the person's choice; the walk takes what it has.
      await js(`${lastPanel}.querySelector('.deny').click(), true`);
    } else if (info.fu) {
      await js(`${lastPanel}.querySelector('.fudone').click(), true`);
    } else if (info.qa) {
      // Questions the publisher authored, asked because a reading came back
      // empty. A select gets its first real option; a text question gets
      // `QUESTION` if one was given and is otherwise left blank, which is the
      // other half of the path — "I don't know" is an answer and travels as
      // `<id>.declined`. Log boxes are never filled: they are the one place a
      // walk could put somebody's real log on the wire.
      const typed = JSON.stringify(process.env.QUESTION || "");
      await js(`(()=>{ const q=document.getElementById('qaok').closest('.panel');
        q.querySelectorAll('select.ai').forEach(s=>{ if(s.options.length>1) s.selectedIndex=1; });
        const answer=${typed};
        if(answer) q.querySelectorAll('textarea.ai').forEach(a=>{
          if(!a.value.trim() && !a.closest('.qb').querySelector('[data-pick=file]')) a.value=answer; });
        document.getElementById('qaok').click(); return true })()`);
    } else if (info.rq) {
      await chooseAnswers();
      await js(`${lastPanel}.querySelector('#rqok').click(), true`);
    } else if (info.free && process.env.FREE_TEXT) {
      // The branch where a person's own words actually travel, taken only when
      // asked for. Declining is the default and stays the default: a walk that
      // sent free text by accident would be the one thing this panel exists to
      // make impossible. What is sent is whatever `FREE_TEXT` says — never a
      // real log, and the panel has already anonymised it, which is the point
      // of looking.
      const shown = await js(`[...${lastPanel}.querySelectorAll('textarea.ft')].map(t=>t.value).join("\\n")`);
      console.log("  free text, as the panel would send it:");
      for (const line of (shown || "").split("\n")) console.log("    " + line);
      await js(`${lastPanel}.querySelector('.allow').click(), true`);
    } else if (info.loc || info.free || info.contrib) {
      await js(`${lastPanel}.querySelector('.deny').click(), true`);
    } else if (info.outcome) {
      await js(`${lastPanel}.querySelector('button[data-o=resolved]').click(), true`);
    } else if (info.allow) {
      await chooseAnswers();
      await js(`${lastPanel}.querySelector('.allow').click(), true`);
    }
  }
}

try {
  // No `Emulation.setDeviceMetricsOverride`, however convenient a fixed
  // screenshot size would be. **WebView2 accepts it and will not undo it** —
  // `clearDeviceMetricsOverride`, a 0x0 override and `resetPageScaleFactor`
  // were all tried and all ignored — so a walk left the person's window
  // rendering at 900x1200 inside whatever size their window actually is:
  // white to the right of 900, everything below the window's height
  // unreachable, and no way back but killing the process. A tool that drives
  // somebody's own window may not put it in a state only a restart fixes.
  //
  // It also made the walk measure the wrong thing. The layout was checked at
  // 700x420 *through* the override, which is self-consistent and says nothing
  // about the real window — the very size the fix was about.
  step("boot");
  await waitFor(`document.getElementById('go') && document.getElementById('go').textContent.length > 0`, "boot");
  await js(`(()=>{ const s=document.getElementById('lang'); s.value='${LANGUAGE}'; s.onchange(); return true })()`);
  await waitFor(`document.getElementById('go').textContent === t('go')`, "the language");
  await waitFor(`window.__TAURI__.core.invoke('index_status').then(s => s.have && s.entries > 0)`, "index cached", 20000);

  if (SCENARIO === "vendor") {
    step("a vendor with a signed agent: the counterparty on :8721");
    await js(`(()=>{ document.getElementById('problem').value=${JSON.stringify(process.env.PROBLEM || "training is slow, bf16?")};
                     document.getElementById('vquery').value='http://127.0.0.1:8721';
                     document.getElementById('go').click(); return true })()`);
    await walkUntilQuiet("vendor");
  } else if (SCENARIO === "model") {
    step("nobody publishes for this: the person's own model leads");
    await js(`(()=>{ document.getElementById('problem').value=${JSON.stringify(process.env.PROBLEM || "My screen flickers since the last driver update")};
                     document.getElementById('vquery').value='http://127.0.0.1:8722';
                     document.getElementById('go').click(); return true })()`);
    await walkUntilQuiet("model");
  } else if (SCENARIO === "settings") {
    step("the model settings");
    await js(`document.getElementById('llmchip').click(), true`);
    await waitFor(`!!document.getElementById('llmpanel')`, "the settings panel");
    await screenshot("settings-open");
    await js(`(()=>{ const s=document.getElementById('lprov'); const o=[...s.options].find(o=>/ollama/i.test(o.value));
                     if(o){ s.value=o.value; s.onchange(); } return !!o })()`);
    await screenshot("settings-ollama");
    // Look, never save. "Load models" and "Test" both save the form first, and
    // the first version of this walk pressed one — which replaced the person's
    // real model settings with the form's half-filled contents. The panel is
    // photographed as it stands; nothing on it is pressed.
    console.log("  provider:", await js(`document.getElementById('lprov').value`),
                " endpoint:", await js(`document.getElementById('lep').value`));
  }
  if (SCENARIO !== "published") {
    const bad = focusChecks.filter(f => !f.ok);
    console.log(`\nfocus checks: ${focusChecks.length}, refusing button not focused on: ${bad.map(b => b.panel).join("; ") || "none"}`);
    console.log("\npanels:", JSON.stringify(await panels(), null, 1));
    console.log("\nconsole:\n" + consoleLines.filter(l => !l.includes("IPC custom protocol")).join("\n"));
    const german = reportLanguage();
    const broken = await failedPanels();
    if (bad.length || german || broken) process.exitCode = 1;
    console.log(broken ? "\nA PANEL SAYS SOMETHING WENT WRONG" : bad.length ? "\nFOCUS FAILURES"
      : german ? "\nLANGUAGE FAILURES" : "\nOK");
    ws.close();
    process.exit(process.exitCode || 0);
  }

  step("ask about engram");
  await js(`(()=>{ document.getElementById('problem').value='The chat in engram never answers';
                   document.getElementById('vquery').value='engram';
                   document.getElementById('go').click(); return true })()`);

  step("pick the problem class");
  await waitFor(`!!document.querySelector('input.pc')`, "the class picker");
  await js(`(()=>{ const want=${JSON.stringify(process.env.CLASS || "")};
                   const rs=[...document.querySelectorAll('input.pc')];
                   const r=(want && rs.find(x=>x.value===want)) || rs[0];
                   if(!r) return 'no class to pick';
                   r.checked=true; return r.value })()`);
  await screenshot("class-picker");
  await js(`(()=>{ const p=document.querySelector('input.pc').closest('.panel'); p.querySelector('.allow').click(); return true })()`);

  step("consent to the project's readings");
  await waitFor(`${lastPanel}.querySelectorAll('.rp').length > 0`, "the read consent");
  await screenshot("read-consent");
  const offered = await js(`[...${lastPanel}.querySelectorAll('.rp')].map(c=>c.dataset.id)`);
  console.log("  offered:", offered.join(", "));
  await js(`${lastPanel}.querySelector('.allow').click(), true`);

  await waitFor(`${lastPanel}.querySelector('.locp') && ${lastPanel}.textContent.includes('engram')`, "the engram location question");
  await screenshot("where-is-engram");
  if (ENGRAM_DIR) {
    step("engram is not on the PATH: say where it is");
    await js(`(()=>{ const p=${lastPanel}; p.querySelector('.locp').value=${JSON.stringify(ENGRAM_DIR)};
                     p.querySelector('.allow').click(); return true })()`);
    await waitFor(`[...document.querySelectorAll('#main > div.done h3')].some(h=>h.textContent.includes(t('loc_ok_h',{p:'engram'})))`, "engram's version");
    await screenshot("engram-found");
  } else {
    // No copy of engram on this machine, which is an ordinary situation and
    // not a reason to invent one: a program that prints a version string would
    // be a stand-in for the thing under test, and what is under test is the
    // client. Declining must cost nothing — the version is recorded as
    // declined and the diagnosis carries on without it.
    step("engram is not on this machine: decline, which must cost nothing");
    await js(`${lastPanel}.querySelector('.deny').click(), true`);
    await screenshot("engram-declined");
  }

  // Ollama is probably not installed here either; decline, which must cost nothing.
  if (await js(`!!(${lastPanel}.querySelector('.locp') && ${lastPanel}.textContent.includes('ollama'))`)) {
    step("ollama is not installed: decline");
    await js(`${lastPanel}.querySelector('.deny').click(), true`);
  }

  step("the questions");
  await waitFor(`!!document.getElementById('qaok')`, "the question panel");
  const asked = await js(`[...document.getElementById('qaok').closest('.panel').querySelectorAll('.ai')].map(i=>i.dataset.id+':'+i.tagName)`);
  console.log("  asked:", asked.join(", "));
  await js(`(()=>{ const q=document.getElementById('qaok').closest('.panel');
     const set=(id,v)=>{ const e=q.querySelector('.ai[data-id="'+id+'"]'); if(e) e.value=v; return !!e; };
     set('engram.symptom', ${JSON.stringify(process.env.SYMPTOM || "search returns nothing, and it used to work")});
     set('ollama.host','OLLAMA_HOST is not set and I am not running Ollama');
     set('error.text','Error: could not reach http://192.168.0.26:11434 — see C:\\\\Users\\\\jdoe\\\\engram.log');
     q.querySelector('[data-pick=file][data-for="ollama.log"]').click();
     q.querySelector('[data-file-for="ollama.log"] .logpath').value=${JSON.stringify(LOG_FILE)};
     q.querySelector('[data-load=file][data-for="ollama.log"]').click();
     return true })()`);
  await waitFor(`(document.querySelector('.ai[data-id="ollama.log"]')||{value:''}).value.includes('not found')`, "the loaded log");
  await screenshot("questions-with-log");
  await js(`document.getElementById('qaok').click(), true`);

  // The operator is asked, and asking is its own consent. This panel was added
  // to the published path and this walk was never taught about it, so the walk
  // sat in front of it until it timed out — and `HANDOVER` went on citing the
  // walk of 2026-09-11 as evidence for a path that had since grown a step.
  // What is listed here is the *anonymised* facts (`C4a`), which is the thing
  // worth photographing on this path: it is where a person's log excerpt and
  // their own words are offered to somebody else.
  step("consent to send to the operator");
  await waitFor(`!!document.getElementById('pubsend')`, "the send consent");
  await js(`document.getElementById('pubsend').querySelector('details').open=true, true`);
  await screenshot("send-consent");
  console.log("  told:", await js(`[...document.getElementById('pubsend').querySelectorAll('p.why')]
      .map(p=>p.textContent.trim()).join(' / ')`));
  await js(`document.getElementById('pubsend').querySelector('.allow').click(), true`);

  step("the answer");
  await waitFor(`[...document.querySelectorAll('#main h3')].some(h=>h.textContent===t('pub_found_h'))`, "their answer", 20000);
  await screenshot("their-answer");

  step("did it help?");
  await waitFor(`!!${lastPanel}.querySelector('button[data-o=unresolved]')`, "the outcome question");
  await js(`${lastPanel}.querySelector('button[data-o=unresolved]').click(), true`);

  step("consent to the report");
  await waitFor(`${lastPanel}.querySelector('.allow') && ${lastPanel}.textContent.includes(t('report_what'))`, "the report consent");
  await js(`${lastPanel}.querySelector('details').open=true, true`);
  await screenshot("report-consent");
  await js(`${lastPanel}.querySelector('.allow').click(), true`);

  step("the free text, anonymised");
  await waitFor(`${lastPanel}.querySelectorAll('textarea.ft').length > 0`, "the free-text consent");
  const ft = await js(`[...${lastPanel}.querySelectorAll('textarea.ft')].map(t=>t.dataset.id+'\\n'+t.value).join('\\n---\\n')`);
  console.log("  would send:\n" + ft.split("\n").map(l => "    | " + l).join("\n"));
  const notes = await js(`[...${lastPanel}.querySelectorAll('p.why')].map(p=>p.textContent.trim()).join(' / ')`);
  console.log("  told:", notes);
  await screenshot("free-text-anonymised");
  await js(`${lastPanel}.querySelector('.allow').click(), true`);

  step("the receipt");
  await waitFor(`[...document.querySelectorAll('#main h3')].some(h=>h.textContent===t('pub_receipt_h'))`, "the receipt", 20000);
  await screenshot("receipt");
  console.log("  receipt:", await js(`[...document.querySelectorAll('#main > div')].pop().textContent.trim()`));
  console.log("\npanels:", JSON.stringify(await panels(), null, 1));
  console.log("\nconsole:\n" + consoleLines.join("\n"));
  const broken = await failedPanels();
  if (reportLanguage() || broken) { process.exitCode = 1; console.log(broken ? "\nA PANEL SAYS SOMETHING WENT WRONG" : "\nLANGUAGE FAILURES"); }
  else console.log("\nOK");
} catch (e) {
  console.error("\nFAILED:", e.message);
  try { await screenshot("failure"); } catch {}
  process.exitCode = 1;
} finally {
  ws.close();
}

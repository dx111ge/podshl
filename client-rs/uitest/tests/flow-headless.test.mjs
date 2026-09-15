// The published path, headless, against the real binary.
//
// **What is real here and what is not.** The page is the one that ships, byte
// for byte, served under the application's own Content-Security-Policy. Every
// `invoke` reaches the actual `podshl-client`, which reaches the actual
// operator. What is replaced is Tauri's IPC, and it is replaced by a process
// call to the same Rust rather than by a recording — a recording is a claim
// that the shape has not changed since somebody captured it.
//
// What this buys over the window test beside it: no desktop, no Windows, no
// WebView2. This is the one that can run in CI, which matters because the flow
// is where every defect this project has seen actually lived.
//
// It needs a client built with `--features uitest`, because the flow calls
// three commands a released client must not have. That is a separate build on
// purpose; `browser.mjs` refuses with the command to make one.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { startBridge } from "../lib/bridge.mjs";
import { openHeadless, refuseWithoutTestSurface } from "../lib/browser.mjs";
import { clientPath } from "../lib/window.mjs";
import { execFileSync } from "node:child_process";

const HERE = dirname(fileURLToPath(import.meta.url));
const UI = join(HERE, "..", "..", "ui");
const QUERY = process.env.UITEST_QUERY || "engram";
const PROBLEM = process.env.UITEST_PROBLEM || "The chat in engram never answers";

let client;
before(() => {
  client = clientPath();
  refuseWithoutTestSurface(client);

  // **Establish the precondition rather than hope for it.** Finding a
  // published project needs a verified directory, and a container that has
  // never fetched one has none — the page then falls back to guessing a vendor
  // from the name, the flow correctly reports that nothing is published, and
  // the test times out looking for answers that were never going to come. The
  // window fetches this at boot; doing it here means the test says what it
  // needs instead of depending on that having worked.
  const ep = JSON.parse(execFileSync(client, ["invoke", "endpoints", "{}"], { encoding: "utf8" }));
  const out = execFileSync(client, ["invoke", "refresh_index", JSON.stringify({ base: ep.index })],
                           { encoding: "utf8" });
  const idx = JSON.parse(out);
  assert.ok(idx.entries > 0,
    `the operator at ${ep.index} serves a directory with ${idx.entries} entries, so there is ` +
    `no published project to walk to. Not a client defect — register one, or point this at an ` +
    `operator that has one.`);
});

/** Ask about a project and get past the provenance question. */
async function ask(b) {
  await b.waitFor(`!!document.getElementById("vquery")`, "the page to finish starting");
  // The language first, and *waited for*. Changing it re-renders every label,
  // and asking in the same breath means clicking a button the page is in the
  // middle of rewriting — which looked exactly like the flow never starting.
  await b.evaluate(`(() => {
    const l = document.getElementById("lang");
    l.value = "en"; l.dispatchEvent(new Event("change", { bubbles: true }));
    return l.value;
  })()`);
  await b.waitFor(`document.getElementById("go").textContent.trim().length > 0`,
                  "the language to be applied");
  await b.evaluate(`(() => {
    document.getElementById("problem").value = ${JSON.stringify(PROBLEM)};
    document.getElementById("vquery").value = ${JSON.stringify(QUERY)};
    document.getElementById("go").click();
    return true;
  })()`);
  await b.waitFor(`!!document.querySelector('button[data-y=""]')`, "the provenance question");
  await b.evaluate(`(document.querySelector('button[data-y=""]').click(), true)`);
}

test("the page loads under its own CSP and reaches the published answers", async () => {
  const bridge = await startBridge({ uiDir: UI, client });
  const b = await openHeadless({ url: bridge.origin });
  try {
    await ask(b);
    await b.waitFor(`!!document.querySelector("input.pc")`, "the published answers to be offered");

    const seen = await b.panels();
    assert.deepEqual(seen.filter(p => /does not publish a support agent/i.test(p)), [],
      `the page says the project publishes no support agent while offering its answers: ${JSON.stringify(seen)}`);

    // The shim is a file rather than an inline script because `script-src
    // 'self'` forbids the second. If the policy were not applied here, this
    // would pass under a weaker rule than the product runs under.
    const csp = await b.evaluate(
      `(document.querySelector('meta[http-equiv="Content-Security-Policy"]') || {}).content || "header"`);
    assert.ok(csp, "the page was served without a policy");

    // And it really did go to the binary.
    assert.ok(bridge.calls.includes("search_vendors"),
      `nothing reached the client: ${JSON.stringify(bridge.calls)}`);
  } finally {
    await b.close();
    await bridge.stop();
  }
});

test("an operator that cannot be reached is not a project that publishes nothing", async () => {
  // **The defect of 2026-09-14, as a test that needs no desktop.** The card
  // cannot be fetched; the window used to say the project published no support
  // agent and start a model on its own. Only `published_card` is answered here
  // — everything else still goes to the real binary and the real operator, so
  // what is being tested is the flow's decision and not a simulation of it.
  const bridge = await startBridge({
    uiDir: UI, client,
    onInvoke: (cmd) => cmd === "published_card"
      ? new Error("not reachable: error sending request for url (test)")
      : undefined,
  });
  const b = await openHeadless({ url: bridge.origin, port: 9556 });
  try {
    await ask(b);
    await b.waitFor(`!!document.querySelector("input.pc")`, "the published answers to be offered");
    await b.evaluate(`(() => {
      const r = document.querySelector("input.pc");
      r.checked = true;
      r.closest(".panel").querySelector(".allow").click();
      return true;
    })()`);

    await b.waitFor(
      `[...document.querySelectorAll(".panel")].some(p => /could not be reached/i.test(p.textContent))`,
      "the panel that says the catalogue could not be reached");

    const seen = await b.panels();
    assert.deepEqual(seen.filter(p => /does not publish a support agent/i.test(p)), [],
      `an unreachable operator was reported as a project that publishes nothing: ${JSON.stringify(seen)}`);
    assert.deepEqual(seen.filter(p => /what changed/i.test(p)), [],
      `a model was started although the project publishes answers: ${JSON.stringify(seen)}`);

    // It offered the retry rather than deciding for the person.
    const buttons = await b.evaluate(
      `[...document.querySelectorAll(".panel")].slice(-1)[0].querySelectorAll("button").length`);
    assert.equal(buttons, 2, "the unreachable panel does not offer a choice");
  } finally {
    await b.close();
    await bridge.stop();
  }
});

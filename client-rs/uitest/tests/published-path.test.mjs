// The two defects a person actually hit, as one walk through the real window.
//
// Both were reported on 2026-09-14 and neither could have been caught by
// anything this project had: `cargo test` covers the Rust, `ui_contract` reads
// the window's source, and text cannot fall into the wrong branch. The gates
// they went wrong at are decided in JavaScript, in a window nobody could drive.
//
//   1. a project that publishes answers must reach the published panel, and
//      must not be announced as publishing nothing
//   2. the answer shown must be the project's own text, matched against what
//      was read here, with no model writing it
//
// **One test, not two.** They are one walk: the second begins where the first
// ends, and splitting them made the second depend on the window state the first
// left behind — which is a shared mutable fixture wearing a test's clothes.
//
// This needs a client built for a reachable operator, because it walks the real
// one. It fails with an instruction rather than skipping: a suite that quietly
// tests nothing is worse than one that is red, a lesson this repository has
// already paid for twice.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { openWindow } from "../lib/window.mjs";

const QUERY = process.env.UITEST_QUERY || "engram";
const PROBLEM = process.env.UITEST_PROBLEM || "The chat in engram never answers";
const WANT_CLASS = process.env.UITEST_CLASS || "";

let w;
before(async () => { w = await openWindow({ port: Number(process.env.UITEST_PORT || 9444) }); });
after(async () => { if (w) await w.close(); });

test("a published project answers from its own files, and no model writes it", async () => {
  await w.waitFor(`!!document.getElementById("vquery")`, "the window to finish starting");

  // **Pin the language.** The window remembers the last one chosen, so a run
  // after somebody read it in German asserts against German — which is how the
  // first version of this file failed on its own English regex rather than on
  // the product.
  await w.evaluate(`(() => {
    const l = document.getElementById("lang");
    l.value = ${JSON.stringify(process.env.UITEST_LANG || "en")};
    l.dispatchEvent(new Event("change", { bubbles: true }));
    return l.value;
  })()`);

  await w.evaluate(`(() => {
    document.getElementById("problem").value = ${JSON.stringify(PROBLEM)};
    document.getElementById("vquery").value = ${JSON.stringify(QUERY)};
    document.getElementById("go").click();
    return true;
  })()`);

  // `confirmOrigin`, added after this window was first walked — the walk then
  // sat in front of it until it timed out and reported that the class picker
  // never came, which is true and says nothing. Declined rather than answered:
  // that reading is a separate consent with its own case.
  await w.waitFor(`!!document.querySelector('button[data-y=""]')`, "the provenance question");
  await w.evaluate(`(document.querySelector('button[data-y=""]').click(), true)`);

  // ---- the first defect -------------------------------------------------
  await w.waitFor(`!!document.querySelector("input.pc")`, "the published answers to be offered");

  const seen = await w.panels();
  assert.deepEqual(seen.filter(p => /does not publish a support agent/i.test(p)), [],
    `the window says the project publishes no support agent while offering its answers: ${JSON.stringify(seen)}`);
  await w.waitForLog(/ui noVendorPath .* published=true/,
    "the window to record reaching the published path");

  // ---- walk it to the end ------------------------------------------------
  const chosen = await w.evaluate(`(() => {
    const want = ${JSON.stringify(WANT_CLASS)};
    const rs = [...document.querySelectorAll("input.pc")];
    const r = (want && rs.find(x => x.value === want)) || rs[0];
    if (!r) return null;
    r.checked = true;
    r.closest(".panel").querySelector(".allow").click();
    return r.value;
  })()`);
  assert.ok(chosen, "there was no class to choose");

  await w.waitFor(`[...document.querySelectorAll(".panel")].some(p => p.querySelectorAll(".rp").length)`,
                  "the consent to read this machine");
  await w.evaluate(`([...document.querySelectorAll(".panel")].reverse()
    .find(p => p.querySelectorAll(".rp").length).querySelector(".allow").click(), true)`);

  // Whatever the project asks a person, answered with its first real choice.
  // What is asserted is where the answer came from, not which answer it is.
  await w.waitFor(`!!document.getElementById("qaok") || !!document.getElementById("pubsend")`,
                  "the project's questions, or the consent to send");
  if (await w.evaluate(`!!document.getElementById("qaok")`)) {
    await w.evaluate(`(() => {
      const p = document.getElementById("qaok").closest(".panel");
      p.querySelectorAll("select.ai").forEach(s => {
        if (!s.value) s.selectedIndex = Math.min(1, s.options.length - 1);
        s.dispatchEvent(new Event("change", { bubbles: true }));
      });
      document.getElementById("qaok").click();
      return true;
    })()`);
  }

  await w.waitFor(`!!document.getElementById("pubsend")`, "the consent to send to the operator");
  await w.evaluate(`(document.getElementById("pubsend").querySelector(".allow").click(), true)`);

  // ---- the second defect -------------------------------------------------
  await w.waitFor(`[...document.querySelectorAll(".panel")].some(p => p.querySelector(".answer"))`,
                  "the project's answer");

  // **Asserted on facts rather than on prose.** The answer reaches a reader in
  // their own language, translated by their own model, so matching a sentence
  // is matching a translation — an earlier version of this failed on its own
  // English regex against a correct German answer. What has to be true is that
  // the text came from the project: its name, and a commit it published.
  const answer = await w.evaluate(
    `[...document.querySelectorAll(".panel")].find(p => p.querySelector(".answer")).textContent`);
  assert.ok(answer.includes(QUERY),
    `the answer does not name the project it came from:\n${answer.slice(0, 300)}`);
  assert.match(answer, /\d+\.\d+/,
    `the answer does not name a commit the project published:\n${answer.slice(0, 300)}`);

  // One settled snapshot for both: the second assertion is a *negative* one,
  // and a negative read of a log still being written passes for the wrong
  // reason — the line it looks for may simply not have arrived yet. Waiting
  // for the positive line first is what makes the absence mean something.
  const log = await w.waitForLog(/ui publishedPath: answered from the project's own files/,
    "the window to record answering from the project's own files");
  assert.doesNotMatch(log, /falling through to the model/,
    `a model was reached on a path that had a published answer:\n${log}`);
});

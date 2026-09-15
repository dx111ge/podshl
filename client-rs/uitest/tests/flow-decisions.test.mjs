// The decisions, on their own, in milliseconds.
//
// The flow test beside this one walks a browser and a real operator and takes
// two seconds. It is the truth, and it is not where you want to be when you are
// asking "what should happen when the card cannot be fetched" — that question
// has an answer that needs no network, no page and no operator, and until
// `ui/flow.js` existed it could only be asked by driving all three.
//
// Every case below is a defect this project actually shipped.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInThisContext } from "node:vm";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));

/** The file the page loads, evaluated the way the page evaluates it.
 *
 *  Read and run rather than imported: it is a plain script because the page
 *  loads it under `script-src 'self'` with no bundler, and a test that imported
 *  a second, module-shaped copy would be testing something else. */
function loadFlow() {
  const src = readFileSync(join(HERE, "..", "..", "ui", "flow.js"), "utf8");
  // In this realm, not a fresh one. A separate `vm` context has its own
  // `Array`, so an empty array from in there is not deep-equal to an empty
  // array out here — two identical values that compare unequal, which is a
  // confusing half hour. The page runs it in one realm too.
  runInThisContext(src);
  return globalThis.Flow;
}

const Flow = loadFlow();

test("a hit's answers are read as they arrive, or not at all", () => {
  assert.deepEqual(Flow.publishedAnswers({ answers: ["a.b", "c.d"] }).classes, ["a.b", "c.d"]);
  assert.deepEqual(Flow.publishedAnswers({}).classes, []);
  assert.deepEqual(Flow.publishedAnswers(null).classes, []);
  // Whatever came off the wire, this must not throw in the middle of a flow.
  assert.deepEqual(Flow.publishedAnswers({ answers: "a.b" }).classes, []);
  assert.deepEqual(Flow.publishedAnswers({ answers: [1, "a.b", null] }).classes, ["a.b"]);
  // Labels are absent on every manifest published before they existed.
  assert.deepEqual(Flow.publishedAnswers({ answers: ["a"] }).labels, {});
  assert.deepEqual(
    Flow.publishedAnswers({ answers: ["a"], answer_labels: { a: "It will not start" } }).labels,
    { a: "It will not start" });
  assert.deepEqual(Flow.publishedAnswers({ answers: ["a"], answer_labels: ["x"] }).labels, {});
});

test("a project that publishes answers is offered them", () => {
  const plan = Flow.planBefore({ answers: ["engram.llm.model-not-pulled"] }, { have: true, entries: 1 });
  assert.equal(plan.kind, "offer-published");
  assert.equal(plan.sayNoAgent, undefined, "a project with answers is not announced as publishing none");
});

test("why nothing was found is said, when nothing was found", () => {
  // No directory at all is a different sentence from a directory that holds
  // this project with nothing in it. Both were once "does not publish a
  // support agent", which is about an Agent Card and says nothing about either.
  const none = Flow.planBefore({ answers: [] }, { have: false, entries: 0 });
  assert.equal(none.kind, "nothing-published");
  assert.equal(none.directory, "missing");

  const empty = Flow.planBefore({ answers: [] }, { have: true, entries: 7 });
  assert.equal(empty.directory, "holds-entries");
  assert.equal(empty.entries, 7);
});

test("an operator that could not be reached is not a refusal and not a verdict", () => {
  // **The defect of 2026-09-14.** Six outcomes came back as one bare `false`
  // and every one of them started a model.
  const unreachable = Flow.planAfter("unreachable", { published: true });
  assert.equal(unreachable.kind, "offer-retry");
  assert.notEqual(unreachable.kind, "to-model", "a timeout chose a model for somebody");
  assert.ok(!unreachable.sayNoAgent,
    "a project that publishes answers was announced as publishing none, because the network failed");
});

test("the person declining is a choice, and the model may follow it", () => {
  const declined = Flow.planAfter("declined", { published: true });
  assert.equal(declined.kind, "to-model");
  assert.equal(declined.because, "declined");
  // Accurate here: `discover` established there is no Agent Card before any of
  // this ran, and the published answers did not resolve it.
  assert.equal(declined.sayNoAgent, true);
});

test("an answer ends it, and nothing is said afterwards", () => {
  const done = Flow.planAfter("answered", { published: true });
  assert.equal(done.kind, "done");
  assert.ok(!done.sayNoAgent);
});

test("giving up starts nothing", () => {
  const stopped = Flow.planOnGivingUp();
  assert.equal(stopped.startModel, false);
  assert.equal(stopped.sayNoAgent, false);
});

test("every outcome the page can return is one this file knows", () => {
  // The page's own exits, read from it. A seventh added there without being
  // added here fails now rather than falling through to a model silently.
  const page = readFileSync(join(HERE, "..", "..", "ui", "index.html"), "utf8");
  const body = page.slice(page.indexOf("async function publishedPath("));
  const returned = [...body.slice(0, body.indexOf("\nasync function ", 10)).matchAll(/return "([a-z]+)"/g)]
    .map(m => m[1]);
  assert.ok(returned.length >= 6, `only found ${returned.length} outcomes in publishedPath`);
  for (const r of new Set(returned)) {
    assert.ok(Flow.OUTCOMES.includes(r), `publishedPath returns ${r}, which flow.js does not know`);
  }
  // And each one has a plan.
  for (const o of Flow.OUTCOMES) {
    assert.ok(["done", "offer-retry", "to-model"].includes(Flow.planAfter(o, {}).kind),
      `${o} has no plan`);
  }
});

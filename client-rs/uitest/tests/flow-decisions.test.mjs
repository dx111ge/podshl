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

test("a skipped question is an answer, and the endpoint is told so", () => {
  // **The defect that asked for ever.** `SPEC.md` makes "I don't know" a wire
  // fact. The panel recorded an answer and recorded nothing for a skip, so the
  // endpoint never learned the question had been put, and armed it again on
  // every round — on any card whose firmware carries no serial, that never ends.
  const q = [{ id: "fw.serial" }, { id: "fw.version" }];
  const r = Flow.readAnswers(q, { "fw.version": "3.2" });
  assert.deepEqual(r.facts, { "fw.serial.declined": true, "fw.version": "3.2" });
  assert.deepEqual(r.stated, ["fw.version"]);
  assert.equal(r.ok, true);
  // Whitespace is not an answer.
  assert.deepEqual(Flow.readAnswers([{ id: "a" }], { a: "   " }).facts, { "a.declined": true });
});

test("required means unanswered, not declined", () => {
  // The hand-off's own round. An unanswered required field must not open a case
  // in somebody's name — which could not happen while the choices arrived with
  // the first one selected, so it was never checked, and then the default went.
  const r = Flow.readAnswers([{ id: "who" }], {}, { required: true });
  assert.equal(r.ok, false);
  assert.deepEqual(r.invalid, ["who"]);
  assert.deepEqual(r.facts, {}, "an unanswered required field was recorded as declined");
});

test("a value that does not fit the publisher's pattern is refused, whole", () => {
  const q = [{ id: "serial", pattern: "[A-Z]{2}\\d+" }, { id: "note" }];
  const bad = Flow.readAnswers(q, { serial: "hello", note: "the fan is loud" });
  assert.equal(bad.ok, false);
  assert.deepEqual(bad.invalid, ["serial"]);
  // Nothing half-applied: the caller is told to refuse the round, and a person
  // is not left with some answers kept and no way to see which.
  assert.equal(bad.facts.note, "the fan is loud",
    "the good answer is still computed, for a caller that asks for it");

  const good = Flow.readAnswers(q, { serial: "AB12", note: "" });
  assert.equal(good.ok, true);
  assert.deepEqual(good.facts, { serial: "AB12", "note.declined": true });
});

test("a pattern is anchored, and a broken one constrains nothing", () => {
  // `\d+` means the whole value, not a digit somewhere in it. The two panels
  // were free to disagree about this while each had its own copy.
  assert.equal(Flow.matchesPattern("\\d+", "12"), true);
  assert.equal(Flow.matchesPattern("\\d+", "a12b"), false);
  // Already anchored by the publisher, and not anchored twice into nonsense.
  assert.equal(Flow.matchesPattern("^\\d+$", "12"), true);
  // Somebody else's typo does not block a person in a panel that cannot say why.
  assert.equal(Flow.matchesPattern("([unclosed", "anything"), true);
  // An empty answer is never pattern-checked; it is a decline or it is missing.
  assert.deepEqual(Flow.readAnswers([{ id: "a", pattern: "\\d+" }], { a: "" }).invalid, []);
});

test("what came off the wire cannot stop a round", () => {
  // `questions` is the publisher's, through the operator.
  assert.equal(Flow.readAnswers(null, {}).ok, true);
  assert.equal(Flow.readAnswers([null, 7, { nope: 1 }], {}).ok, true);
  assert.deepEqual(Flow.readAnswers([{ id: "a" }], null).facts, { "a.declined": true });
  assert.deepEqual(Flow.readAnswers([{ id: "a" }], { a: 12 }).facts, { "a.declined": true });
});

test("only a person's own words are editable on the way out", () => {
  // Asked for after somebody saw a real account name in a real panel. A reading
  // is what the machine said, and a box over it would make the report a fiction.
  const shown = { "os.version": "13.2", "serial.printed": "AB12", "note": "  " };
  const typed = new Set(["serial.printed", "note"]);
  assert.deepEqual(Flow.editable(shown, typed), ["serial.printed"],
    "a machine reading is offered for editing, or an empty answer is");
  // The window keeps its typed ids in two collections and hands both.
  assert.deepEqual(Flow.editable(shown, ["serial.printed"]), ["serial.printed"]);
  // Nothing is editable when nothing was typed, and nothing throws.
  assert.deepEqual(Flow.editable(shown, null), []);
  assert.deepEqual(Flow.editable(null, typed), []);
  // A declined marker is a boolean, not a person's words.
  assert.deepEqual(Flow.editable({ "a.declined": true }, new Set(["a.declined"])), []);
});

test("emptying a box withdraws the answer, it does not send an empty one", () => {
  const shown = { "serial.printed": "AB12", "os.version": "13.2" };
  const out = Flow.applyEdits(shown, { "serial.printed": "" });
  assert.equal("serial.printed" in out, false, "an empty string was sent as the answer");
  assert.equal(out["serial.printed.declined"], true);
  assert.equal(out["os.version"], "13.2", "a reading was disturbed by an edit elsewhere");
  // Whitespace is empty.
  assert.equal("a" in Flow.applyEdits({ a: "x" }, { a: "   " }), false);
  // A changed value is the changed value, trimmed.
  assert.equal(Flow.applyEdits({ a: "x" }, { a: " y " }).a, "y");
});

test("the edit box cannot put in something nobody was shown", () => {
  // The panel's promise is that what is sent is what was on screen. An edit
  // naming a key that is not in the snapshot is not an edit.
  const out = Flow.applyEdits({ a: "x" }, { b: "smuggled" });
  assert.deepEqual(out, { a: "x" });
  assert.equal("b.declined" in out, false, "an unknown key was withdrawn, which invents it");
});

test("the snapshot a person agreed to is not changed underneath them", () => {
  // `applyEdits` returns a new object, so there is no moment at which what
  // would be sent is half-edited, and the original stays as it was listed.
  const shown = Object.freeze({ a: "x", b: "y" });
  const out = Flow.applyEdits(shown, { a: "" });
  assert.notEqual(out, shown);
  assert.deepEqual(shown, { a: "x", b: "y" });
  assert.deepEqual(out, { b: "y", "a.declined": true });
});

test("a send with no panel in front of it does not happen quietly", () => {
  // C4 used to be true by construction, argued in a comment. This is the same
  // claim as something the running program knows.
  const c = Flow.consent();
  assert.throws(() => c.factsFor("operator"), /nothing has been agreed for operator/);
  assert.equal(c.granted("operator"), false);
  c.grant("operator", { a: "1" });
  assert.deepEqual(c.factsFor("operator"), { a: "1" });
});

test("agreeing that one party may see a value is not agreeing that another may", () => {
  // The operator that mirrors a project is a third party the person is not told
  // about until the panel that names it. The vendor's yes is not its yes.
  const c = Flow.consent();
  c.grant("vendor", { serial: "AB12" });
  assert.throws(() => c.factsFor("operator"), /operator/);
  assert.deepEqual(c.factsFor("vendor"), { serial: "AB12" });
});

test("what was agreed does not change underneath the agreement", () => {
  // The gate keeps a copy. A gate holding a reference to an object the page
  // goes on mutating is the two-reads-of-a-mutable-object problem in a new
  // place — which is the defect C4 exists because of.
  const c = Flow.consent();
  const shown = { a: "1" };
  c.grant("vendor", shown);
  shown.b = "arrived after the panel was drawn";
  assert.deepEqual(c.factsFor("vendor"), { a: "1" },
    "a fact that arrived after the panel was drawn was sent anyway");
  // It may be added, but only by showing it and granting again.
  c.grant("vendor", shown);
  assert.deepEqual(c.factsFor("vendor"), { a: "1", b: "arrived after the panel was drawn" });
});

test("a new question is a new incident, and the last one's yes is spent", () => {
  const c = Flow.consent();
  c.grant("vendor", { a: "1" });
  c.clear();
  assert.throws(() => c.factsFor("vendor"), /nothing has been agreed/);
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

/* ------------------------------------------------- report assembly */

test("a withheld reading is not offered back as a box to retype", () => {
  // `dropped` holds both kinds: a machine reading no policy could coarsen, and
  // a person's own words. Only the second may be offered. Reading `dropped`
  // here instead of `stated` would put a measurement in an editable box, which
  // is the one thing the reading panel exists to refuse.
  const report = {
    stated: { "log.excerpt": null },
    dropped: ["log.excerpt", "gpu.serial"],
  };
  const facts = { "log.excerpt": "CUDA error: out of memory", "gpu.serial": "0325918101234" };
  assert.deepEqual(Flow.withheldWords(report, facts), ["log.excerpt"]);
});

test("supplied-and-withheld is a null, and nothing else is", () => {
  const facts = { a: "words", b: "words", c: "words" };
  // Present with a value: it travelled, there is nothing to offer.
  assert.deepEqual(Flow.withheldWords({ stated: { a: "1.2.3" } }, facts), []);
  // An empty string travelled and was empty. That is not withheld.
  assert.deepEqual(Flow.withheldWords({ stated: { b: "" } }, facts), []);
  assert.deepEqual(Flow.withheldWords({ stated: { c: null } }, facts), ["c"]);
});

test("nothing held any more is not a box inviting somebody to write something new", () => {
  const report = { stated: { "log.excerpt": null, "what.i.tried": null } };
  assert.deepEqual(Flow.withheldWords(report, { "log.excerpt": "   " }), []);
  assert.deepEqual(Flow.withheldWords(report, {}), []);
  assert.deepEqual(Flow.withheldWords(report, { "log.excerpt": "a", "what.i.tried": "b" }),
                   ["log.excerpt", "what.i.tried"]);
});

test("a report off the wire cannot stop the free-text panel", () => {
  assert.deepEqual(Flow.withheldWords(null, null), []);
  assert.deepEqual(Flow.withheldWords({}, {}), []);
  assert.deepEqual(Flow.withheldWords({ stated: ["a"] }, { a: "x" }), []);
  assert.deepEqual(Flow.withheldWords({ stated: "a" }, { a: "x" }), []);
  // A fact that is not a string is not words, whatever it is.
  assert.deepEqual(Flow.withheldWords({ stated: { a: null } }, { a: true }), []);
  assert.deepEqual(Flow.withheldWords({ stated: { a: null } }, { a: 42 }), []);
});

test("an emptied box withdraws the words rather than attaching an empty answer", () => {
  const ids = ["log.excerpt", "what.i.tried"];
  assert.equal(
    Flow.consentedText(ids, { "log.excerpt": "out of memory", "what.i.tried": "  " }),
    "log.excerpt: out of memory",
    "an emptied box must not become an id with nothing after it");
});

test("emptying every box attaches nothing at all", () => {
  // Not an attachment holding no text -- that is a second consent recorded
  // against nothing. The caller sends the report it already had.
  assert.equal(Flow.consentedText(["a", "b"], { a: "", b: "   " }), "");
  assert.equal(Flow.consentedText([], { a: "x" }), "");
  assert.equal(Flow.consentedText(null, null), "");
});

test("the attachment reads in the order of the panel, with a blank line between", () => {
  const text = Flow.consentedText(["first", "second"], { second: "b", first: "a" });
  assert.equal(text, "first: a\n\nsecond: b");
  // An id with no box is skipped, not attached empty.
  assert.equal(Flow.consentedText(["first", "gone"], { first: "a" }), "first: a");
  // Whatever is in the box, it is trimmed before it is anybody's evidence.
  assert.equal(Flow.consentedText(["a"], { a: "  x  " }), "a: x");
  assert.equal(Flow.consentedText(["a"], { a: 42 }), "");
});

test("the footer comes off the end and goes back on, and the words come from the binary", () => {
  const FOOTER = "---\n*Assembled by PODSHL on my own machine.*\n";
  const md = "### What I measured\n\nx\n\n" + FOOTER;
  const off = Flow.footerToggle(md, FOOTER, false);
  assert.equal(off, "### What I measured\n\nx\n\n");
  assert.equal(Flow.footerToggle(off, FOOTER, true), md, "putting it back is not the same document");
  // Already off, asked for off again: not two footers' worth of stripping.
  assert.equal(Flow.footerToggle(off, FOOTER, false), off);
  assert.equal(Flow.footerToggle(md, FOOTER, true), md);
});

test("toggling the footer does not throw away what the person typed", () => {
  const FOOTER = "---\n*Assembled by PODSHL on my own machine.*\n";
  const edited = "### What I measured\n\nx\n\nAnd my own note.\n\n" + FOOTER;
  const off = Flow.footerToggle(edited, FOOTER, false);
  assert.ok(off.includes("And my own note."));
  assert.equal(Flow.footerToggle(off, FOOTER, true), edited);
});

test("a footer the binary did not send removes nothing and claims nothing", () => {
  // The failure the old regex was heading for: reword the footer in `issue.rs`
  // and the page's own copy matches nothing, so unchecking the box takes
  // nothing out while the label says it did. Here there is no pattern to go
  // stale -- with no footer given there is nothing to add or remove, and the
  // text is returned as it stands rather than guessed at.
  const md = "### What I measured\n\nx\n";
  assert.equal(Flow.footerToggle(md, "", false), md);
  assert.equal(Flow.footerToggle(md, undefined, true), md);
  assert.equal(Flow.footerToggle(md, null, false), md);
  assert.equal(Flow.footerToggle(undefined, "f", false), "");
  // A footer that is not at the end is not this document's footer.
  assert.equal(Flow.footerToggle("a---b", "---", false), "a---b");
});

/* ------------------------------------------- the glossary, early enough */

test("the terms arrive with the labels they are for", () => {
  // The whole point: at the moment the first question is translated, the card
  // has not been fetched and the index entry is all there is.
  const hit = { answers: ["engram.llm.model-not-pulled"],
                answer_labels: { "engram.llm.model-not-pulled": "Search returns nothing" },
                glossary_keep: ["brain", ".brain"] };
  assert.deepEqual(Flow.publishedGlossary(hit), ["brain", ".brain"]);
  assert.deepEqual(Flow.publishedAnswers(hit).classes, ["engram.llm.model-not-pulled"]);
});

test("an index built before the glossary existed is not an error", () => {
  // Every entry served by an operator that has not been updated. No terms is
  // yesterday's behaviour, which is a translation with nothing kept -- not a
  // failure, and not something to refuse the question over.
  assert.deepEqual(Flow.publishedGlossary({ answers: ["a"], answer_labels: { a: "x" } }), []);
  assert.deepEqual(Flow.publishedGlossary({}), []);
  assert.deepEqual(Flow.publishedGlossary(null), []);
});

test("a term that could not be a word is not sent as one", () => {
  // Ingest already refuses these; this arrives over a wire, so it is checked
  // again. An empty string as a term to keep would substitute against every
  // position in the text -- the placeholder pass would eat the whole sentence.
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: ["", "  ", "123", "4.2"] }), []);
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: ["brain", "", "123"] }), ["brain"]);
  // Trimmed, and the same term twice is one term.
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: ["  brain  ", "brain"] }), ["brain"]);
  // A term with a letter in it stays, whatever else it has.
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: [".brain", "my.brain"] }),
                   [".brain", "my.brain"]);
  // Non-Latin letters are letters.
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: ["\u30d6\u30ec\u30a4\u30f3"] }), ["\u30d6\u30ec\u30a4\u30f3"]);
});

test("whatever came off the wire cannot stop the first question", () => {
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: "brain" }), []);
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: { keep: ["brain"] } }), []);
  assert.deepEqual(Flow.publishedGlossary({ glossary_keep: [1, null, "brain", {}] }), ["brain"]);
});

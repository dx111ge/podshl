// The decisions, with no page around them.
//
// **Why this file exists.** `index.html` is 2200 lines of JavaScript in one
// block, and every defect this project has seen lived in them: a project that
// publishes answers handed to a model, an operator that could not be reached
// reported as a project that publishes nothing. Neither is a rendering fault.
// Both are decisions — *when an answer counts as found, when a model may run* —
// taken in the middle of code that also builds panels, and therefore only
// checkable by driving a window.
//
// What is here is the deciding, and nothing else. No DOM, no `await`, no
// transport: given what came back, what should happen next. That makes each one
// a test that runs in a millisecond instead of a browser, and it makes the page
// code say what it is doing rather than work it out inline.
//
// **This is a first slice, not the whole of it.** The consent ordering, the
// question rounds and the report assembly are still in the page. They belong
// here too, one at a time, each move covered by the headless flow test that
// already walks the real thing.
//
// A plain script rather than a module, on purpose: the page loads it under
// `script-src 'self'` with no bundler and no build step, and the tests read it
// and evaluate it. Nothing in the chain needs tooling that could drift from
// what ships.

globalThis.Flow = (function () {
  "use strict";

  /** What a search hit publishes: the classes, and the sentence for each.
   *
   *  `answers` is the identifiers a rule matches on; `answer_labels` is the
   *  maintainer's sentence for them, absent on every manifest published before
   *  that field existed. Both are read defensively because they come off the
   *  wire — and the shape was doubted for a whole day in 2026-09-14's
   *  diagnosis, wrongly, which is a reason to state it here once. */
  function publishedAnswers(hit) {
    const classes = Array.isArray(hit && hit.answers) ? hit.answers.filter(c => typeof c === "string") : [];
    const raw = hit && hit.answer_labels;
    const labels = raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
    return { classes, labels };
  }

  /** What to do before the published path runs.
   *
   *  "Does not publish a support agent" is about an Agent Card and nothing
   *  else, and it was being said for every other reason as well — including
   *  the two that are ours: no verified directory to search, or a directory
   *  that holds this project without any answers in it. A person then reads
   *  that a project publishes nothing while it publishes three answers. */
  function planBefore(hit, indexStatus) {
    const { classes } = publishedAnswers(hit);
    if (classes.length) return { kind: "offer-published", classes };
    const have = !!(indexStatus && indexStatus.have);
    return {
      kind: "nothing-published",
      directory: have ? "holds-entries" : "missing",
      entries: have ? (indexStatus.entries || 0) : 0,
      sayNoAgent: true,
    };
  }

  /** What to do with the outcome the published path returned.
   *
   *  **It used to return one bare `false` for six different things**, and the
   *  caller read every one of them as permission to start a model. So an
   *  operator that did not answer was indistinguishable from a person saying
   *  "none of these", and a network failure chose a model for somebody.
   *
   *  `retried` exists so the caller cannot loop for ever on its own: the
   *  decision to ask again is the person's, and this says only that asking is
   *  what is owed. */
  function planAfter(outcome, context) {
    const ctx = context || {};
    if (outcome === "answered") return { kind: "done" };
    if (outcome === "unreachable") return { kind: "offer-retry", why: ctx.why || "" };
    return {
      kind: "to-model",
      // Accurate rather than suppressed: `discover` established that there is
      // no Agent Card before any of this ran. What must not happen is saying it
      // about a failure to reach the operator, which is the case above.
      sayNoAgent: !!ctx.published,
      because: outcome,
    };
  }

  /** What is owed when somebody declines to try again. */
  function planOnGivingUp() {
    return { kind: "stopped", sayNoAgent: false, startModel: false };
  }

  /** The six outcomes the published path may report. Named here so a seventh
   *  cannot be added in the page without this file knowing about it. */
  const OUTCOMES = ["answered", "declined", "unreachable", "reads", "nofinding", "none"];

  return { publishedAnswers, planBefore, planAfter, planOnGivingUp, OUTCOMES };
})();

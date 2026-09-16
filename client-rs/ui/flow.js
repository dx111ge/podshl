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
// **The four moves are made.** The consent ordering, the question rounds, the
// consent edits and the report assembly are all here now. Report assembly was
// the smallest of them and the answer to "is there a decision left in the page
// at all" was yes, three: which withheld facts still have words to offer, what
// an emptied free-text box means, and what the footer checkbox does. Each move
// was covered by the headless flow test that walks the real thing.
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

  /** The project's own terms, at the moment the first question is asked.
   *
   *  **The card is too late and that is not a bug in the card.** A published
   *  project's glossary lives in its Agent Card, the card is fetched under a
   *  consent the person gives *after* choosing which problem they have, and the
   *  sentence they choose from is publisher text that gets translated. So the
   *  one translation a person is guaranteed to read is the one translated with
   *  no glossary at all, and `LG8` measured what that costs: engram's `brain`
   *  lost in eighteen runs of eighteen across German, French and Spanish.
   *
   *  The index entry answers it. The terms are the publisher's own published
   *  words, the index is public and already fetched and already signed, and it
   *  is where `answer_labels` comes from — so the words and the glossary for
   *  them arrive together, under one consent, or rather under none, because
   *  reading a directory is not asking a project anything.
   *
   *  Read off the wire as defensively as the labels beside it, and absent from
   *  every index built before this existed: no terms is the old behaviour, not
   *  an error. */
  function publishedGlossary(hit) {
    const raw = hit && hit.glossary_keep;
    if (!Array.isArray(raw)) return [];
    const out = [];
    for (const t of raw) {
      if (typeof t !== "string") continue;
      const term = t.trim();
      // A term with no letter in it cannot be a word a translation would
      // change, and ingest already refuses one. Checked again here because
      // this arrives over a wire, and an empty string as a "term to keep"
      // would be a substitution against every position in the text.
      if (!term || !/\p{L}/u.test(term)) continue;
      if (!out.includes(term)) out.push(term);
    }
    return out;
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
    // **"None of these fit" is not a request for a guess.** It is a statement
    // about the classes this project published, and the window used to read it
    // as permission: the person said no, and a model they never asked for began
    // interrogating them about a project it had never been told the name of.
    // Found on the first Omarchy desktop this ran on, by somebody saying none
    // fit and getting nonsense.
    //
    // The better exit already existed and was only offered after the published
    // path *answered*: the issue report — everything asked and everything read,
    // anonymised, as markdown for the project's own tracker. That is the honest
    // answer to "nothing published covers this", and a model that knows nothing
    // about the project is not.
    //
    // So this asks instead of assuming. **And `nofinding` asks too**, which was
    // decided the other way an hour earlier and corrected by reading the log of
    // the same desktop: nobody rejected anything there, the person picked a
    // class, the readings were taken and the publisher's rules said nothing —
    // and a model started anyway.
    //
    // That case is worse rather than milder. A publisher who writes `escalate`
    // has said in their own manifest what happens when nothing matches —
    // engram's reads *"Nothing published matches these readings, and it may be
    // a defect rather than a setup problem"* — and starting a model there does
    // not fill a gap, it overrides an instruction.
    //
    // `none` is left alone: a hit with no published answers at all has nobody
    // to defer to, so a model is the only thing there is.
    //
    // **Both of them are reported, under one word: `uncovered`.** Not as a
    // by-product of something else — asking leaves no trace at all, because
    // `/diagnose` runs in a read-only transaction — so unless this says so, a
    // maintainer never learns that anybody reached the end of their published
    // answers and found nothing. Nobody files an issue titled "your support
    // page did not have my problem on it"; they close the window.
    //
    // `declined` is reported as well as `nofinding`, and it is the stronger of
    // the two: rules producing no statement is a gap between rules, while a
    // person looking at their own machine and saying "none of these is what I
    // am seeing" is a judgement. Both say the same thing to the person who has
    // to act on it — *write one down* — so both carry the same word.
    if (outcome === "declined" || outcome === "nofinding") {
      return { kind: "offer-elsewhere", because: outcome, tell: "uncovered" };
    }
    return {
      kind: "to-model",
      // Accurate rather than suppressed: `discover` established that there is
      // no Agent Card before any of this ran. What must not happen is saying it
      // about a failure to reach the operator, which is the case above.
      sayNoAgent: !!ctx.published,
      because: outcome,
    };
  }

  /** A publisher's `pattern`, anchored the way the client anchors every one.
   *
   *  Anchored here rather than trusted: a pattern written `\d+` matches a digit
   *  *somewhere* in a value, which is not what a publisher who wrote `\d+`
   *  means, and the hand-off and the questions were free to disagree about it
   *  because each had its own copy of this line.
   *
   *  An expression that does not compile constrains nothing. The alternative is
   *  blocking a person on somebody else's typo, in a panel that cannot tell
   *  them what is wrong with it. */
  function matchesPattern(pattern, v) {
    let re;
    try { re = new RegExp("^(?:" + String(pattern).replace(/^\^|\$$/g, "") + ")$"); }
    catch (_) { return true; }
    return re.test(v);
  }

  /** What a round of answers becomes.
   *
   *  Two panels ask questions — the publisher's open questions, and the
   *  hand-off's required ones — and they differ in exactly one thing, which is
   *  what an empty box means. Everything else about them was written twice and
   *  went wrong separately:
   *
   *  * **An empty answer is an answer.** `SPEC.md` makes "I don't know" a wire
   *    fact, `<probe id>.declined`. The questions panel recorded an answer and
   *    recorded *nothing* for a skip, so the endpoint was never told and asked
   *    again — for ever, on any card whose firmware carries no serial.
   *  * **Required means required.** In the hand-off an empty box is not a
   *    decline, it is unanswered, and an unanswered required field must not open
   *    a case in somebody's name. That could not happen while the choices
   *    arrived with the first one selected, so it was never checked, and then
   *    the default was removed.
   *  * **A pattern is checked before anything is built**, not after a report was
   *    assembled around the value.
   *
   *  **Three panels, not two.** The need round — the one that puts the
   *  endpoint's `need` to a person — wrote this rule a third time and checked
   *  no patterns at all, so a value the vendor had said would not do went out
   *  anyway and came back as another round of the same question. It could not
   *  be converted with the other two because it had nowhere to say no: it
   *  resolved the moment somebody clicked. It now keeps the panel open, like
   *  the other two, and that is the last of the three.
   *
   *  Nothing is applied when anything is invalid: the round is refused whole, so
   *  a person is not left with half their answers recorded and no way to see
   *  which half. `stated` is which of these a person said rather than a machine
   *  measured — the hand-off does not track that and ignores it. */
  function readAnswers(questions, values, options) {
    const required = !!(options && options.required);
    const asked = Array.isArray(questions) ? questions : [];
    const given = (values && typeof values === "object") ? values : {};
    const facts = {};
    const stated = [];
    const invalid = [];

    for (const q of asked) {
      if (!q || typeof q.id !== "string") continue;
      const raw = given[q.id];
      const v = typeof raw === "string" ? raw.trim() : "";
      if (!v) {
        if (required) invalid.push(q.id);
        else facts[q.id + ".declined"] = true;
        continue;
      }
      if (q.pattern && !matchesPattern(q.pattern, v)) { invalid.push(q.id); continue; }
      facts[q.id] = v;
      stated.push(q.id);
    }
    return { facts, stated, invalid, ok: invalid.length === 0 };
  }

  /** What a person may still change on the way out.
   *
   *  Their own words, and only those. A reading is what the machine said, and a
   *  box that let somebody edit it would make the report a fiction — the vendor
   *  would be answering about a machine that does not exist. Asked for after
   *  somebody saw a real account name in a real panel: *can I make that `xxx`
   *  instead?*
   *
   *  `typed` is the ids the person supplied rather than the machine — the
   *  window keeps two sets of them and hands both. */
  function editable(shown, typed) {
    const values = (shown && typeof shown === "object") ? shown : {};
    const was = typed instanceof Set ? (k => typed.has(k))
      : Array.isArray(typed) ? (k => typed.includes(k))
      : () => false;
    return Object.keys(values).filter(
      k => was(k) && typeof values[k] === "string" && values[k].trim());
  }

  /** What changing one of those boxes means.
   *
   *  **Emptying one is not a blank answer, it is a withdrawn one**, and that is
   *  a wire fact like any other: the value goes and `<id>.declined` takes its
   *  place. An empty string says the person answered "", which the endpoint has
   *  no way to read as "they took it back" — and is not what they did.
   *
   *  **Only what was shown can be changed.** An edit naming something that is
   *  not in the snapshot is ignored rather than added: the panel's whole promise
   *  is that what is sent is what was on screen, and a fact nobody saw must not
   *  get in through the edit box.
   *
   *  Returns the snapshot rather than changing it, so the caller rebinds and
   *  there is never a moment where what would be sent is half-edited. */
  function applyEdits(shown, edits) {
    const out = Object.assign({}, shown);
    for (const k of Object.keys(edits || {})) {
      if (!(k in out)) continue;
      const raw = edits[k];
      const v = typeof raw === "string" ? raw.trim() : "";
      if (v) out[k] = v;
      else { delete out[k]; out[k + ".declined"] = true; }
    }
    return out;
  }

  /** What has been agreed to leave this machine, and to whom.
   *
   *  **C4 — what is shown before sending is what is sent — was true by
   *  construction.** The panel builds a snapshot, the request sends that
   *  snapshot, and the argument that nothing else could get out was a comment:
   *  *there is nothing left that could send it*. That is a fact about today's
   *  code rather than a property of it, and it is exactly the kind that stops
   *  being true in a patch that looks unrelated — which is what the comment
   *  itself says about the version before it.
   *
   *  This makes it something the running program knows. A send asks the gate
   *  for its facts, and the gate has them only because a panel put that exact
   *  snapshot in front of somebody and they said yes. A send added later
   *  without a panel in front of it does not quietly send the wrong thing: it
   *  throws, naming the destination nobody agreed to.
   *
   *  **Per destination.** Agreeing that the vendor may see a value is not
   *  agreeing that the operator may. They are different parties and one of them
   *  — the operator that mirrors a project — is a third party the person was
   *  not told about until the panel that names it.
   *
   *  **The snapshot is copied.** A gate holding a reference to an object the
   *  page goes on mutating is the two-reads-of-a-mutable-object problem in a
   *  new place. So anything added after consent has to be shown and granted
   *  again — which is what the `need` rounds do, and why they may add to it.
   *
   *  **Not the local model.** Its consent is the reading panel, which names the
   *  model as the thing that will see the values; there is no second send to
   *  gate, and pretending there is one would make this look like more than it
   *  is. */
  function consent() {
    const granted = new Map();
    return {
      /** A panel showed `facts` and named `to`, and the person said yes. */
      grant(to, facts) {
        granted.set(to, Object.assign({}, facts));
        return granted.get(to);
      },
      /** The facts for a destination, or a refusal to invent any. */
      factsFor(to) {
        if (!granted.has(to)) {
          throw new Error(
            `nothing has been agreed for ${to}. Something is about to send facts ` +
            `that were never put in front of anybody — a panel is missing, or it ` +
            `is after the send instead of before it.`);
        }
        return granted.get(to);
      },
      granted: to => granted.has(to),
      /** A new question is a new incident, and last one's agreement is spent. */
      clear() { granted.clear(); },
    };
  }

  /** Which withheld facts there are still words to offer.
   *
   *  `preview_report` has already decided what travels. What it hands back says
   *  so in two places, and only one of them is the right one to read here:
   *
   *  * `dropped` is everything held back, **including machine readings** that no
   *    policy could coarsen safely. Offering a person a box to retype one of
   *    those is the free-text panel doing the exact thing the reading panel
   *    refuses — putting a measurement in a box somebody can rewrite, so the
   *    recipient answers about a machine that does not exist.
   *  * `stated[id] === null` is the narrow one: *a person supplied this, and it
   *    was withheld*. `report.rs` writes that null deliberately — the key is
   *    present so the recipient knows the answer rested on something supplied,
   *    the value is absent because typed text can contain anything.
   *
   *  So it is `stated`, and the null is load-bearing rather than incidental: a
   *  fact whose value is an empty string is one that travelled and was empty,
   *  which is not the same thing and must not be offered.
   *
   *  And there must still be words. The report is a snapshot taken earlier; if
   *  nothing is held for an id any more, an empty box that says "this was
   *  withheld" invites somebody to type something new into a panel whose whole
   *  promise is that it shows what already exists. */
  function withheldWords(report, facts) {
    const stated = (report && report.stated && typeof report.stated === "object"
                    && !Array.isArray(report.stated)) ? report.stated : {};
    const held = (facts && typeof facts === "object") ? facts : {};
    return Object.keys(stated).filter(id => {
      if (stated[id] !== null) return false;
      const v = held[id];
      return typeof v === "string" && v.trim() !== "";
    });
  }

  /** What the free-text panel attaches, out of what is in its boxes.
   *
   *  **An emptied box is a withdrawal**, the same rule `applyEdits` keeps for
   *  readings — nothing is attached for it. It does not become `id:` with
   *  nothing after it, which would tell the recipient a person answered and
   *  said nothing, and it does not become `<id>.declined` either: the fact is
   *  already in the report as supplied-and-withheld, and that is what it stays.
   *
   *  **Empty all of them and nothing is attached at all.** The caller gets `""`
   *  and sends the report it already had, rather than a consented-text
   *  attachment holding no text — which is a second consent recorded against
   *  nothing.
   *
   *  The ids lead, so the order is the order of the panel rather than of
   *  whatever object the values arrived in, and an id with no box is skipped
   *  rather than attached empty. Each block is `id: words`, and the blank line
   *  between them is what makes a pasted log readable at the other end.
   *
   *  This was a chain of `map` and `filter` in the page that tested emptiness by
   *  asking whether the assembled line ended in a colon and a space. It was
   *  right. It was right for a reason nobody could see without running it. */
  function consentedText(ids, values) {
    const given = (values && typeof values === "object") ? values : {};
    const out = [];
    for (const id of Array.isArray(ids) ? ids : []) {
      if (typeof id !== "string") continue;
      const raw = given[id];
      const v = typeof raw === "string" ? raw.trim() : "";
      if (!v) continue;
      out.push(id + ": " + v);
    }
    return out.join("\n\n");
  }

  /** What the box holds when the footer checkbox changes.
   *
   *  The footer says how the text was made — assembled locally, read with
   *  permission, anonymised first — on a document going into a public issue
   *  tracker under the person's own name. A checkbox that claims to remove it
   *  and does not is a lie about disclosure, which is why this is here and not
   *  left as two lines beside a `<textarea>`.
   *
   *  **The words come from the binary.** `issue_report` returns them next to the
   *  markdown it put them in. The page used to carry its own regex for the same
   *  sentence, written out twice, and the failure that arrangement was heading
   *  for is silent in the direction that matters: reword the footer in
   *  `issue.rs`, and the pattern matches nothing, unchecking the box removes
   *  nothing, and the label goes on saying it was taken out.
   *
   *  **Off the end of what is there now, not off the original.** The text is
   *  editable and the person may have changed it before touching the checkbox;
   *  rebuilding from the generated markdown would throw their edits away.
   *
   *  A suffix, exactly — `the_footer_is_exactly_a_suffix` in `issue.rs` is that
   *  half of the bargain. With nothing to add or take away this returns what it
   *  was given rather than guessing. */
  function footerToggle(current, footer, keep) {
    const text = typeof current === "string" ? current : "";
    const foot = typeof footer === "string" ? footer : "";
    if (!foot) return text;
    const body = text.endsWith(foot) ? text.slice(0, -foot.length) : text;
    return keep ? body + foot : body;
  }

  /** What is owed when somebody declines to try again. */
  function planOnGivingUp() {
    return { kind: "stopped", sayNoAgent: false, startModel: false };
  }

  /** The six outcomes the published path may report. Named here so a seventh
   *  cannot be added in the page without this file knowing about it. */
  const OUTCOMES = ["answered", "declined", "unreachable", "reads", "nofinding", "none"];

  /** Every plan `planAfter` can return, for the same reason. A page that grew a
   *  branch this file does not know about is a decision back in the page. */
  const PLANS = ["done", "offer-retry", "offer-elsewhere", "to-model"];

  return { publishedAnswers, publishedGlossary, planBefore, planAfter,
           planOnGivingUp, matchesPattern, readAnswers, editable, applyEdits,
           consent, withheldWords, consentedText, footerToggle, OUTCOMES, PLANS };
})();

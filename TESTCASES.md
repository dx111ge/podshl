# Test cases

    mise run services    # the four counterparty services
    mise run client-test # the client suite, against the shipped binary
    mise run testcases   # every row marked `auto`, by id

**The client cases live here; the server's live in
[TESTCASES-SERVER.md](TESTCASES-SERVER.md).** That split is the one
`PUBLISHING.md` asks for — client cases go public with the client, `SV*` do not
— made now rather than under time pressure on publication day. The runner reads
both files.

Every case states what *should* happen, because a suite that only records what
the code does today cannot say when the code became wrong. Coverage is marked
honestly: **`auto` runs unattended, `manual` needs a person at the window,
`open` is a known gap and counts as failing.**

**The runner reads this table.** It parses every `auto` row and fails if one has
no implementation — so the document cannot drift back into claiming coverage it
does not have. That check has caught a row documented and untested (`J2`), and
six cases quietly documented *twice*, saying slightly different things in two
places: it compares sets, so duplicates were invisible to it until it counted
them separately.

Rows tagged `[rust]` in the runner delegate to `cargo test`; they live on that
side of the wire, and one command still answers for the whole list.

**What no row covers:** the Rust client end to end through its own window.
Driving a GUI with synthetic keystrokes is the weakest verification available
and it collides with anyone actually using the machine. `cargo run -- demo`
drives the shipped binary through the whole flow without one, which narrows that
gap rather than closing it.

Everything on the client side is now checked against **the binary that ships**.
It was previously checked against a second implementation in Python, which had
quietly drifted from this one — the two action vocabularies disagreed, and the
case guarding that boundary could not see it because it asserted only an id
common to both.

## Discovery

| # | Case | Expected | Cover |
|---|---|---|---|
| D1 | Vendor publishes a valid signed Agent Card | Verified; legal entity and LEI shown before anything is read | auto |
| **D2** | **Vendor publishes no Agent Card (404)** | **"Keine Anbindung" — explicitly *not* a trust failure. No skill, no signature, no reporting path. The agent offers to solve it locally and states that the vendor will never learn it happened** | auto |
| D3 | Vendor host unreachable | Same class as D2, different reason text | auto |
| D4 | Well-known URI returns non-JSON | Treated as D2, not as an error | auto |
| D5 | Well-known URI returns HTTP 500 | Treated as D2 with the status named | auto |
| D6 | Card carries no signature | Refused as untrusted | auto |
| D7 | Card body altered after signing | Refused as untrusted | auto |
| D8 | No out-of-band key for the domain | Refused — a self-asserted card is never accepted | auto |
| D9 | Card signed by a different vendor's key | Refused | auto |
| D10 | Protected header declares an alg other than EdDSA | Refused | auto |
| D11 | Small-order public key offered | Refused **as a key**, when it is read, naming what is wrong. `verify_strict` would have refused every signature under it anyway — but as "signature does not verify", which reads as a tampered card rather than as a key nobody holds a secret for. The identity, order-two and order-four points are all offered, and the vendor's real key must still pass | auto |

## Skill selection

| # | Case | Expected | Cover |
|---|---|---|---|
| S1 | Problem matches a published skill | Skill descriptor returned with its probes | auto |
| S2 | Problem matches nothing | Routed to a human — a shrug is not an outcome | auto |
| **S3** | **A requirement the client cannot check** (`cuda`, `product`) | **Not a refusal, and this is deliberate. Whether a *product* is present is the vendor's question, answered by its own read instructions returning a value or not — probing hardware here would be the client guessing at a domain it does not own, and it would be a read taken before anyone consented to one** | auto |
| S4 | Skill declares another OS | Refused before probing | auto |

## Diagnostic rounds and the named vendor

| # | Case | Expected | Cover |
|---|---|---|---|
| S5 | A round answer carries both reads and questions | Both parsed — the model must be able to ask a person, not only read | auto |
| S6 | The model names an id that does not exist | Never becomes a read; the catalogue is the authority | auto |
| S7 | An id appears inside a *question* | Not treated as a request to read it | auto |
| S8 | The model reports completion | Recognised — but it is a suggestion, and only the user ends the loop | auto |
| **V1** | **The user names a vendor the reading contradicts** ("Intel", an AMD card is installed) | **Flagged with both values and the offer to switch. The claim is a claim; the reading is not — and a diagnosis against the wrong vendor is worthless in both paths** | auto |
| V2 | The reading agrees with the named vendor | Silent — no false alarm | auto |
| V3 | Nothing was read yet | No mismatch is claimed | auto |

## Method, where nobody publishes one

On the published path a maintainer has already decided which fact separates
which problem, and the client walks their tree. Here there is none — so the
client supplies the *method* and the model supplies the domain. That division
is the line this must not cross: a client carrying domain knowledge would be
competing with the publishers it exists to carry, and would be wrong more
often than they are. What it carries is the shape of a diagnosis, the way
`spec/SPEC.md` fixes the shape of a remedy without writing one.

| # | Case | Expected | Cover |
|---|---|---|---|
| **MD1** | **Each round carries one dimension of the method, in order** | **What changed, then the boundary — what else the user would expect to be affected and is not — then when, then how much. Which dimension is the *client's* decision, not the model's: telling a model all three at once produces three shallow questions in one breath, and a model asked to remember where it is in a method does not. Past the ladder the rounds stop rather than inventing a fourth dimension, and the model is told not to re-ask what the window already asked** | auto |
| **MD2** | **An answer is split into its sections, and graded by what it rests on** | **Cause, why not elsewhere, what it rests on, what to do, what would disprove it. The ids in `RESTS ON` are matched against what is actually known rather than taken from the text — the rule `parse_round` applies to read ids — and an answer resting on a value the person *typed* is graded `rests_on_supplied`, exactly as `SV71` grades a finding on the published path. It may be right and it is not evidence: the person could have been mistaken. A model that ignored the format is carried whole rather than shredded, so a reader can see that it did** | auto |
| **MD5** | **Every prompt names the language, and says it last** | **Found by walking the window in German: the labels were German and the model's answer was English. "the language with the code 'de'" is an abstraction a 4B model resolves about as often as it ignores, and it sat before a wall of English keywords — recency is most of what a small model has, so the last thing it read was English. The language is named plainly now and the instruction repeated after the keywords, in every prompt that produces text a person reads. Not a guarantee and not claimed as one: a model that will not follow an instruction cannot be made to. What is checked is that the client asks properly** | auto |
| **MD4** | **A short id is not dragged in by a longer one** | **Found by walking the window rather than by reading the code: the model wrote `RESTS ON: os.version` and the basis came back naming `os` too, because the match was `contains`. The answer claimed to rest on a fact nobody had named. The read path had the same flaw and a worse consequence — a model asking for `os.version` would have had `os` read as well, a reading nobody requested appearing on a consent panel. One rule now, shared by both parsers** | auto |
| **MD3** | **The answer must explain what it does not affect** | **The step models skip, and the one that separates a diagnosis from a plausible sentence: a cause that cannot say why the comparable case is unaffected has not been tested against anything and reads exactly like one that has. Abstaining is offered in the same breath, because a model with nowhere to say "not enough" guesses. The section keywords stay English so they can be parsed; everything in them is in the user's language** | auto |

## Read catalogue and platform

| # | Case | Expected | Cover |
|---|---|---|---|
| N1 | The catalogue on this machine | Offers only what is actually runnable here — otherwise the model picks it, the user consents, and nothing comes back | auto |
| N2 | A missing or unlisted tool | Refused with a stated reason, never a silent empty result | auto |
| N3 | A denied path (`.ssh`, credentials, wallets) | Refused, and **consent cannot unlock it** | auto |
| N4 | The free baseline | Carries OS and architecture, read nothing to get them | auto |

## Report identity

| # | Case | Expected | Cover |
|---|---|---|---|
| ID1 | Two vendors receive reports from the same client | Different pseudonyms — neither can link this client to the other | auto |
| ID2 | The same vendor within one epoch | The same pseudonym, or rate limiting and blocking cannot work | auto |
| ID3 | The epoch | A year-month, so a vendor's view of a client expires on its own | auto |

## Language served by the vendor

| # | Case | Expected | Cover |
|---|---|---|---|
| **LG1** | The vendor has the language the client asked for | It serves that, and nothing is translated — there is nothing to compare against, so no second tab appears | auto |
| **LG2** | It does not have that language | Falls back to English, **which every vendor owes**. One extra language is a small burden and it guarantees every user something readable without pushing safety-relevant text through a machine | auto |
| **LG3** | A skill exists only in the vendor's own language | Refused loudly. Serving a language the user may not read, silently, is the failure this obligation exists to prevent | auto |
| LG4 | The response | States which language was served and which was asked for — that decides whether the client must translate | auto |
| **LG6** | **The finding** | **In the language the diagnosis asked for, or English — the rule the skill already followed. `diagnose` carried no language, so a vendor could not know it, and the walk through the window got a German finding on an English screen** | auto |
| **V4** | **A vendor that is not a chip brand, and a reading that names one** | **No contradiction. "Intel" against an AMD card is one; a card maker, a typed address or a software project against an NVIDIA chip is not — the user named whom they are asking. The walk through the window was told "that does not add up" about the counterparty's own demo, and the switch it offered did nothing: the run went on sending readings to the vendor the person had just rejected. A switch now ends the run** | auto |
| LG5 | Local translation | Reached only when the vendor lacks the language, marked as machine translation, with the original rendered alongside and one click away from every consent screen | auto |
| **LG7** | **A published project's words, in a language it did not write** | **Translated by the reader's own model where one is set up — the questions before they are asked, the answer once it is found — marked as machine translation, with the original one click away. Never the `choices`: the answer that is recorded has to be the publisher's own word. A project owes English and nothing more, so the window was in German and every question in it in English. Without a model nothing changes** | auto |
| **LG8** | **A published project's own term — a feature, a file format — in a translation** | **Reaches the reader as the project wrote it. The project lists it under `glossary.keep`; the client replaces every occurrence with a placeholder before the text goes to the reader's model and puts the original back after, so the model is never shown a word to translate. Measured against `gemma3:4b` and `qwen2.5:7b` in German, French and Spanish: engram's *brain* lost in 18 runs of 18 without it — *cerveau*, *Gehirn* — and in none with it. Telling the model to keep the term was tried first and worked by luck of phrasing. A placeholder the model drops anyway is found by count and said beside the translation. **Since 2026-09-16 this reaches the first sentence a person reads, which was the one place it did not.** `class_labels` is translated before the card is fetched — the question comes before the consent that fetches it — so the glossary could not arrive in time and that sentence alone was translated with nothing kept. It rides on the index entry now, beside the labels it is for: the same published words in the same public signed document, so nothing is disclosed and no extra fetch is made. `SV107` checks it reaches the entry, and the window contract pins that the page reads it — it used to pin `translateKeeping(src, [])`, which pinned the defect** | auto |
| **LG9** | **A command or code in a project's answer, in a translation** | **Reaches the reader byte for byte. Fenced blocks, indented blocks and inline spans are hidden from the reader's model behind placeholders, as a project's own terms are, and put back after; a placeholder the model drops names the command beside the translation. Measured first: asked to keep "commands and code unchanged", `gemma3:4b` rewrote engram's indented `engram reindex my.brain` as `reindexer mon.brain` in every French and Spanish run of six. With the code hidden, six live runs on two models in three languages lost none** | auto |

## Traversal

| # | Case | Expected | Cover |
|---|---|---|---|
| E1 | The version tree | Walked, and the named keys read out of each manifest found | auto |
| E2 | A traversal that escapes its root or enters a denied name | Refused before it runs | auto |

## Which machine the answer is about

| # | Case | Expected | Cover |
|---|---|---|---|
| **EN1** | **A project that runs in a virtualenv** | **Its Python is read from `.venv/pyvenv.cfg`, which states the version as a plain key — so the answer is about the environment the project uses rather than the interpreter on this process's PATH, and **nothing is executed** to get it. The directory is granted for the incident and withdrawn with it** | auto |
| EN2 | Running inside a container | Answerable, and read from files. If the user's code is containerised, host readings are about the wrong machine; if the client is, the whole diagnosis is | auto |
| **EN3** | **The host interpreter and the venv disagree** | **A question, never a resolution — neither is obviously the one meant, so the user is asked. Nobody is told a version that is not theirs. Agreement is silent, a differing patch level is not a disagreement, and one reading alone cannot contradict anything** | auto |
| **EN4** | **A new question** | **Ends the last incident first: the project directory and every program location the user gave are withdrawn before anything else runs, and what was read or answered is cleared. `set_project_root` was documented and tested as per incident while nothing but the user emptying a text field ever cleared it, so a directory granted for one diagnosis stayed readable for every diagnosis after it** | auto |
| EN5 | `VS_EXTRA_ROOT` | Read only by a development build. It was read unconditionally, so one environment variable set by a launcher or a wrapper widened every publisher's reach on a shipped binary, with no consent screen involved | auto |

## A program's own version

A version a person has to type is a claim. engram's manifest asked *"which
release are you running?"* with five choices, so every report carried a version
somebody picked from memory — while the program could simply say it.
`program_version` asks it. It is the first read op that starts a program a
publisher chose, so it is bounded on every side: a name and never a path, a flag
from a closed list, a deny list, the system directories refused, a time limit,
and only the version token kept.

| # | Case | Expected | Cover |
|---|---|---|---|
| **PV1** | **A program on the search path** | **Asked for its version, and only the number comes back. Whatever else it prints — a build path, a user name compiled in, a banner — stays on the machine. The consent screen names the resolved file, because which file runs is the thing the user can check** | auto |
| **PV2** | **A program that is not on the search path** | **Asked about, never searched for. A client that walked the disk looking for a program would be reading far beyond what it was allowed to, so the person says where it is — the file or its folder. The file must *be* that program; pointing at `rustc` does not answer a question about `engram`. The location outranks the search path and goes with the incident. A relative entry on the search path — `.` or an empty one — is not searched: it is wherever the client happened to start, and a file sitting there is not what anybody installed** | auto |
| **PV3** | **A program that is not a program's own version** | **Refused whatever the user clicks: a shell, a command runner, a privilege, power or disk tool, a launcher or script host, a path instead of a name, a flag of the publisher's own, `-v` (which half of all programs read as *verbose*), a non-ASCII name, and anything inside the operating system's own directories — which is where the programs that ignore an argument and open a window live** | auto |
| PV4 | What programs actually print | The version is found in engram's usage banner, `Python 3.12.14`, `v20.1.0`, pip's `from …` line, git's `.windows.1` suffix and Docker's `, build …`. A run that failed is believed only on a line naming the program, because an error message is full of numbers that are not versions | auto |
| PV5 | A program that does not answer | Stopped at the limit rather than waited on. The consent screen promised a version, not an open-ended run | auto |
| **PV7** | **A tool whose output is a list** | **Shaped to what its reading promises, by a fixed filter in the client: `pci.devices` is the graphics and network chips, without bus addresses; `mac.displays` is the chipset, vendor, Metal support and resolution. It was the first line, which emptied both — the host bridge, and the heading "Graphics/Displays:" — and returning everything was never the fix: a whole PCI inventory is the fingerprint this vocabulary refuses `pip list` for** | auto |
| **PV9** | **A card reporting `0` for its serial has no serial** | **`nvidia-smi --query-gpu=serial` prints `0` on a consumer RTX card — the card saying it carries none in firmware. It arrived as the string "0", so `warranty.rma.precheck`'s `serial.printed` question never fired and a precheck went to the vendor reading "serial number 0": a return authorised against a number that identifies nothing. `[N/A]` was handled; `0` is the same statement in another dialect. Per field rather than a general rule — a temperature or a fan speed of zero is a reading. Found by walking the window on a real RTX 5070, which is the only way it could have been: every fixture had a serial or an empty string** | auto |
| PV6 | A Docker image | One image however Docker spells it — `docker.io/library/postgres` is `postgres` — and a publisher names a repository, never a tag: the tag is what is being read. Nothing is executed inside a container | auto |

## Log excerpts

The single most useful thing a maintainer can receive is the error, or the lines
of a log around it — and the one thing no policy can coarsen. Free text was
already withheld by default and sent only under its own consent. What was
missing is the middle: getting the lines at all, and taking the person out of
them before anybody is asked to send them.

| # | Case | Expected | Cover |
|---|---|---|---|
| LX1 | A log file the user names | Loaded from its end, bounded, for display only — and the deny list still holds: "paste your `.env`" is not a request this client helps anybody make. A file is never searched for | auto |
| **LX2** | **What has the shape of an identifier** | **Replaced before the text is shown for consent, and counted per kind so the person is told what happened: account and host names wherever they occur, home directories, IPv4 and IPv6 outside loopback, e-mail, credentials in a URL, `password=`-style values, bearer tokens, JWTs, UUIDs, MAC addresses, long tokens — and timestamps, because a report carries no time on purpose and one paste of a log would undo that** | auto |
| LX3 | What is diagnosis rather than identity | Survives: loopback, `0.0.0.0`, ports, version numbers, `file:line:column`, `std::io::Error` | auto |
| LX4 | A container as a source | An image name and nothing else — anything that could be a command or carries a tag is refused before Docker is asked | auto |
| LX5 | How much | Loaded: the last 400 lines. Sent: at most 120 lines and 8,000 characters per answer and 16 KiB per report, refused by the client *and* the operator. The bounds a publisher reads in `reads.json` are the ones applied | auto |
| **LX7** | **This machine's own account and host** | **Found from the environment rather than handed in, and removed by the entry the window actually calls. Every other case here supplies the names and so proves the replacing, never the finding — `own_names()` reads `USERNAME`, `USER`, `LOGNAME`, `COMPUTERNAME`, `HOSTNAME` and `/etc/hostname`, it is the half that decides whether a real person's account leaves their machine, and it had no test at all. Nothing is printed on failure but a count: a message naming the account would put it in the log of whoever ran the suite, which is what this exists to prevent** | auto |
| **LX6** | **The order of it** | **Anonymised before it is offered, not after it is agreed to; attached only after the person said yes; and what they see — still editable — is exactly what is sent** | auto |

## The published path

The branch the open-source offer rests on was walked for the first time against
engram's real files, end to end, and three things were wrong that no case could
see because each case checked one step.

| # | Case | Expected | Cover |
|---|---|---|---|
| **PB1** | **A project that published files** | **Its own `collect` is read — under the same per-item consent as a vendor's skill — before the operator is asked anything. It used to go to `/diagnose` with nothing but the platform, and the tree's first `need` then asked a person to type their operating system** | auto |
| PB2 | A `need` naming a readable fact | Read, not asked. A declined reading is recorded as declined, so the operator falls back rather than asking again | auto |
| **PB3** | **The report** | **Goes to the operator, keyed by subject and pseudonym, with the same two maps as every report. It went through the vendor path — A2A to an agent the project never ran, built from a skill the published path does not have — so every fact was dropped and the send failed. The one branch whose promise is "the maintainer finally hears about it" could not deliver one report** | auto |
| **PB4** | **Whether it worked** | **Asked, after the person had a chance to try — "it worked", "it did not", or nothing reported. It was sent as `resolved` the moment the answer appeared, and that label is the column a maintainer's dashboard is read for. Declining the report returns before anything is sent** | auto |
| **PB5** | **Questions a project declared for a person** | **Asked, with the publisher's own `choices`. The question panel was built from the read plan, which carries machine probes only: a probe declared `human` was never asked on either path, and `choices` were never offered — so every answer became free text, which never travels** | auto |
| PB6 | The report shape against the running operator | Accepted — the client's shape and the server's route agree with each other rather than with a fixture | auto |
| **AT1** | **Who stands behind a published answer** | **Said with the answer: that control of the location was confirmed and on which day — or that it was last confirmed more than two weeks ago — the log entry anybody can check, and the project's own word if it has deprecated itself. The client knew only a vendor's key, verified or untrusted, so the one thing a person weighing a stranger's advice can use — how recently anybody stood behind it — was never shown. A deprecated or stale project's answer is still shown; its age is not a verdict** | auto |
| **AT2** | **"Anybody can check"** | **This client does. The signed tree head verifies against the log key pinned out of band; the entry, hashed as an RFC 6962 leaf, folds with its inclusion proof — for exactly that head's size — to the head's root; and the entry attests the same content hash and commit the mirror is serving. The window says which it was, and a failed proof is shown rather than hidden. Proved against the running operator for the first, middle and last entry, and refused for an entry about other files** | auto |
| **LC1** | **A head is held against the last one this device accepted** | **One session proving a log self-consistent proves very little: a server showing this machine one tree and everybody else another is consistent with itself every time it is asked. The client remembers the size and root it last accepted and refuses a head that does not extend it — and the three ways that can go wrong are three different sentences, because "rewritten", "shrunk" and "this is a different log" are not the same news. The head is remembered only after it has been proved to extend the remembered one; remembering first would let one bad answer erase the evidence that would have caught it** | auto |
| **LC2** | **The operator's own consistency proofs verify in the client** | **Over HTTP against the running server, for every size the log has passed through rather than a sample — the ragged right edge is where this arithmetic goes wrong. The old root is rebuilt in the client from the entries the log serves, and checked against the signed head, so a server that served a convenient root could not be the reason this passes. Both roots are recomputed from the proof, which is the check a naive monitor forgets: an inclusion proof says an entry is in *some* tree** | auto |
| PB7 | The published answer itself | Rendered as the Markdown it is written in. It was shown with blank lines turned into breaks, so the command a solution exists to give — an indented block — ran into one line with the next command, and emphasis arrived as asterisks. Every character is escaped first and only fixed tags are inserted, so nothing a publisher writes becomes markup | auto |
| **W11** | **An inline `style` attribute in the window** | **None. Tauri puts a nonce into `style-src`, and a browser that sees a nonce ignores `'unsafe-inline'` — so every one of the 33 was blocked, and every answer box, list and question past the first screen rendered at the browser's default width. Found by photographing the running window; the first screen's layout comes from the stylesheet and was fine, which is why nobody saw it** | auto |
| W12 | A vendor-chosen id inside an attribute | Escaped, quotes included. The window's one escaper covered `&<>` only and is used inside `data-id="…"`, so a `"` in a skill's probe id ended the attribute. The CSP blocking inline handlers is the second line, not the first | auto |
| **W13** | **A sentence shown with no values to fill in carries none** | **`t("key")` fills nothing in, so a sentence behind such a call that still carries `{n}` reaches a person with the word `undefined` in it — which is where it was found, in the report panel of the real window. The count in that sentence is one the server withholds **on purpose**: below the floor it does not return `reporters`, because "you are the third" is a count about other people's machines and counting up to the floor one report at a time is how the floor gets read from outside. The sentence could never have been filled in, so the text was the half that was wrong. Checked in every language, because a translator who adds a placeholder English does not have makes the same hole** | auto |
| **W14** | **The parts of the window the engine draws follow its theme** | **The palette follows `prefers-color-scheme`; the scrollbar, the caret, select popups and the overscroll edge are the engine's, and it assumes light unless `color-scheme` says otherwise. So the dark theme shipped with a white scrollbar down its right-hand side — invisible to every check that reads the file, and the first thing a person looking at the window says about it. One declaration rather than `::-webkit-scrollbar` rules, because it is the same answer for every control the engine draws** | auto |
| **W16** | **A command that grades an answer is told what was typed** | **`typed` is optional on the Rust side, because a caller with nothing to say about provenance should not be made to lie — and optional is exactly how the follow-up came to omit it, so an answer resting on a value the person supplied was graded as a measurement. That is the one mis-grading the distinction exists to prevent, in the place it matters most: the follow-up is where they have just typed something. The list of commands is derived from `main.rs`, so a third one is covered the day it is written** | auto |
| **W15** | **The conversation scrolls, and the footer stays** | **`.app` is a full-height flex column: a bar, the conversation, the footer the next question is typed into. A flex item's automatic minimum size is its *content*, so the middle one never shrank below the panels inside it — the column grew past the window, `overflow-y` had nothing to scroll because the box was already as tall as its content, and the footer was pushed off the bottom of a window that does not scroll. It looks fine until the conversation is taller than the window, which is every window after a few panels, so a screenshot of the first screen does not show it** | auto |
| **I4** | **The consent text** | **In the user's language. The sentence under each reading — the one text the user actually decides on — came from the binary in German, as did the risk level and the reason a reading was refused, on an English screen. The sentence is now a message from the language files (`I5`) that the window says again in the user's language, and a refusal carries a kind the window names** | auto |

The gap-report cases `GR1`, `GR1a`, `GR2` and `GR3` used to sit here, against
the catch-all. They are server behaviour and now live in
[TESTCASES-SERVER.md](TESTCASES-SERVER.md); the catch-all is gone.

## The third outcome: "I still need X"

Where two problems look alike, the endpoint asks for the fact that separates
them rather than estimating which one this is. A switch is deterministic; a
similarity threshold is wrong in both directions at once. The probes come back
through the same round loop the client already runs with the local model.

| # | Case | Expected | Cover |
|---|---|---|---|
| **ND1** | A decisive fact is missing | **Acquired, not refused** — the endpoint says what it needs and why | auto |
| **ND2** | The user answers "I don't know" | Falls back to the parent node with a destination. **Never asked again** — not everyone knows, and a dead end here is a design failure | auto |
| ND3 | A probe the user declined | Recorded as declined, so the endpoint stops asking and falls back | auto |
| ND4 | The client | Loops on `need` rather than answering once, and records a skip as declined | auto |
| ND5 | The round | Offers both "I don't know" and "cancel" — a loop the user cannot leave is a trap | auto |
| **ND7** | **A question left empty is recorded as declined** | **"Don't know must never dead-end" is a wire convention in `SPEC.md`: `<probe id>.declined`. The need loop had always written it; the panel that *arms* a question — where a machine probe read nothing and the publisher's `when_missing` question takes over — kept an answer and recorded nothing at all for a skip. So the endpoint was never told, and asked again. That is the root of the loop `ND6` stops, and two other bugs sat on top of it: a vendor reading `serial.declined` for a probe called `serial.printed`, and the case for it sending the same wrong key — green for as long as it existed, because the test and the code agreed with each other and both disagreed with the wire** | auto |
| **ND6** | **A round that cannot make progress ends the loop** | **`SPEC.md` is normative — an endpoint receiving `<id>.declined` must not ask again for that fact — and in the same breath says an endpoint returning `need` indefinitely "will loop until the user stops it". That leaves the person to notice, and the way they notice is by clicking through the same question a dozen times. A round whose every probe is already answered or already declined cannot make progress whatever the endpoint intended, and that is decidable here without judging the vendor's reasoning. Walked 37 rounds in the real window before anybody saw it, and only because a card with no firmware serial finally took the branch** | auto |

## The hand-off to a person

The vendor defines it; the client bounds it. Where the case goes is the
vendor's routing decision; what it needs arrives as ordinary probes, so a
contact address is consented and validated like any other value instead of
being a column baked into every client; and the return path is chosen from a
list the client can actually honour.

| # | Case | Expected | Cover |
|---|---|---|---|
| **H1** | The vendor's escalation declaration | Carries a routing target, the fields it needs as probes, and the channels it can answer on — none of it hardcoded here | auto |
| H2 | A vendor offers a channel the client cannot honour | Filtered out before the user is asked to pick it | auto |
| H3 | A vendor invents a channel | Never offered — the same rule as the action vocabulary | auto |
| **H4** | A vendor that will not reply | `none` is a legitimate promise. **Silence is not** — leaving someone waiting for an answer that was never coming is what this list prevents | auto |
| H5 | A completed hand-off | Returns a case reference, states the return path, and passes the routing target through untouched | auto |
| H6 | The no-reply case | Says so explicitly in the receipt | auto |
| H7 | An unsupported channel reaching the command | Refused, naming what is possible | auto |
| H8 | A malformed required field (an address that is not one) | Caught locally, **before** a case is opened — not after an RMA is raised | auto |

## Reachability

| # | Case | Expected | Cover |
|---|---|---|---|
| **W1** | Every registered command | Reachable from the interface. A command nobody calls is a claim nobody honours — this caught four, including the applicability gate | auto |
| W2 | The applicability check | Actually applied, so "this advice is not for your machine" can be reached at all | auto |
| **W10** | **Where the client talks to** | **Configuration, read from the environment in Rust, and read *first* at boot. Both endpoints were `const` literals pinned to loopback — right for a checkout, and a released binary that can only ever address the machine it runs on. Verified by running the window against a real server, a dead port and no setting at all, and watching the catalogue cache appear, not appear, and appear** | auto |

## Languages

| # | Case | Expected | Cover |
|---|---|---|---|
| I1 | Every language table | Defines the same keys — a missing translation falls back silently and looks like a design choice. Checked by **loading the files**, not by matching them with a regex that could only see the first key on a line | auto |
| I2 | Every language | Names itself for the picker | auto |
| I3 | Locale detection | The client reports a usable code from the OS | auto |
| **I5** | **Everything the binary says to a person** | **Is an `m_*` sentence in `ui/i18n/en.json`, which the binary reads when it is built; the source holds codes and every language lives in the language files. The window recognises a message by its English template, lifts out the values and says it again in the chosen language — errors, the dry run, the reasons a reading or a location was refused, a vendor's standing. They were German literals in the source, shown as they came on an English screen. Every code has a sentence, no sentence is kept for a code nothing says, every sentence can be recognised again, and a refusal is sorted by which message it is rather than by the words inside it** | auto |
| **PR1** | **Where this machine says it got the software** | **A name belongs to nobody — 1872 repositories on GitHub carry `engram` — so the list shows candidates and the person recognises theirs. This is the one fact that helps them and that an impostor cannot forge, because it is not on their side of the wire: the machine's own package database records where each program came from, written by the distribution. Measured here: `hyprland` → `github.com/hyprwm/Hyprland`, `curl` → `curl.se`. **No publisher can request it and none can switch it off** — that is why it is the client's own question and not a vocabulary entry. Still a reading, so it is asked for first and refusing costs nothing; it answers nothing far more often than not, and a disagreement is said as one, because a fork or a repackaged version produces one honestly | auto |
| **PR2** | **Every text the window names exists in every language** | **`i18n.rs`: a missing key falls back to English silently, and in a consent dialogue that is the wrong failure — the person is told in a language they may not read what is about to be read from their machine. Two cases checked a handful of keys by hand and everything added since was covered by nobody. Literal keys only, and the first version matched the tail of `split(` and `format(` and reported that English was missing the word `div`, which is how it became clear it was reading identifiers** | auto |
| **IS1** | **A diagnosis has an exit that is not a report to the operator** | **It ended with one offer, a pseudonymous report, which is the wrong shape for the case the open-source branch rests on: the published answers did not cover somebody's problem, they now hold more about it than they could have assembled in an hour, and there was nowhere to put it. "A good bug report in two minutes" was the promise and it did not exist. Markdown for an issue, shown before it is copied and editable, saying what the anonymiser removed — **and the copy button copies the box rather than the generated text**, because copying the original after somebody edited it hands over something they did not read, from a panel whose whole argument is that they did. The footer naming PODSHL can be switched off, and since 2026-09-16 that is worth something: the page used to strip it by matching a copy of its own wording, so rewording the sentence in `issue.rs` would have left unchecking the box removing nothing while the label said it did. `issue_report` now returns the footer beside the markdown, `the_footer_is_exactly_a_suffix` holds the relationship the page relies on, and `Flow.footerToggle` takes it off the end of what is in the box rather than rebuilding from the generated text, so an edit survives the checkbox** | auto |
| **IS2** | **Nothing reaches the issue that a report would have held back** | **An issue tracker is more public than a report and keeps it for ever, under the person's own name — so every string goes through the same anonymiser, the words they typed themselves included, before assembly rather than after: anonymising the finished document would mean one mistake publishes everything rather than one field. Measured and supplied stay in separate sections, because only one of them is evidence about a maintainer's rule. A model's answer is marked **Unchecked** above the text rather than below it, since it reads exactly like a published one. And a value carrying a pipe or a newline cannot end its row and take the rest of itself out of the document** | auto |
| **IS3** | **A version the machine never reported is named before it is copied** | **Measured on a released client against a local model: the answer said *"revert to driver 610.86"* while the machine had reported `610.57`, and nothing had read or been told `610.86`. `rests_on` could not catch it — that says which facts a *match* turned on, and an invented number is in the prose. Marked above the answer, not below, because a maintainer who reads the number first has already started looking, and returned to the panel so it is said before anybody scrolls. **Only where there is something to compare against**: with no version-shaped reading it stays quiet, because a published answer naming "driver 555 or newer" is correct and firing there is noise on top of a right answer. A bare integer is a port or a count, never a version. And it is not a verdict — what it says is that the number was not measured here. Not caught, and written down rather than attempted badly: *"other applications work correctly"*, a claim about facts nobody stated | auto |
| **OM1** | **The desktop's own agent is the model, with its tools denied** | **Omarchy keeps the default agent's *name* in `~/.config/omarchy/defaults/agent`, so on a machine somebody already set up there is no model to configure, no second key and no second place to get wrong. Offered only where this desktop names one, it is installed, and somebody has measured how to call it without its tools — an option that appears and then fails is worse than one that never appears, and here the failure would be a person choosing "no setup needed" and getting nothing. Walked live: probe, then one real completion through the same `solve()` the diagnosis uses, which abstained correctly because no problem was described** | auto |
| **OM2** | **Two switches that read like a fence and are not** | **Measured 2026-09-14 with a marker file. `--allowed-tools ""` **read it** — an empty allow-list means *no restriction*, not *nothing allowed*. `--permission-mode manual --permission-prompts none` refused a file outside the working directory and **read one inside it**: default-deny is a directory boundary, not a tool fence, and it is the one that would have shipped, because it refuses convincingly until the file happens to be underfoot. Only `--disallowed-tools` refused in both places. Two more things the runs showed: the agent handed the job to two sub-agents unprompted, and with every file tool denied it still reported the working directory's git status — the fence stops tool calls, not the context it is started in, so it is started in an empty directory of its own. Only Claude Code is offered; codex was tried and its read-only sandbox bounds what commands *write* rather than what is read** | auto |
| **OM3** | **The agent is only looked for on an Omarchy desktop** | **Without this the only guard was that `~/.config/omarchy/defaults/agent` happens not to exist — a fact about one file rather than a statement about the system, and on Windows or macOS `dirs::config_dir()` would look for that path under `%APPDATA%` or `~/Library/Application Support`. The installation directory is asked first and on purpose: **`OMARCHY_PATH` is set in a login shell and is not in the systemd user environment** — measured — so a client started from a desktop entry or from the bar does not inherit it, which is how this would have worked in every terminal test and for nobody else. Proved by running the detection with an empty environment: no `OMARCHY_PATH`, no `DESKTOP_SESSION`, and the agent is still found, because a directory on disk survives a bare environment. The case also holds the order — the desktop is identified before the file is read** | auto |
| **CL1** | **The client is built for an operator, not for loopback** | **A client built without `PODSHL_BUILD_SERVER_URL` and `PODSHL_BUILD_LOG_KEY` is not a broken client, it is a *plausible* one: it starts, draws its window, and quietly cannot verify the published directory — so every published project falls through to the model, which then asks what the project is, having never been told. `option_env!` is read at compile time and cargo does not rebuild when only an environment variable changed, so this is one `cargo build` away at any moment. `scripts/build_client.sh` builds it and then checks the binary rather than trusting the build; this asks the binary itself** | auto |
| **CL2** | **A published project is found, as owner/repo** | **Through the built client: `engram` returns `dx111ge/engram` with its three problem classes — without them the window skips the published path entirely. The forge's own name reaches no published row** | auto |
| **CL3** | **The published card is fetched for a repository** | **The half missed when the operator learned about repositories: `/mirror/{host}` began refusing a bare forge host — correctly, since `github.com` is shared — and the client kept sending the host. Every repository anchor fell through to the model. The identity is sent now, and a bare forge host is still refused** | auto |
| **CL4** | **The whole published path answers, from the project** | **Search, card, diagnosis, with no model anywhere in it. The answer names the fix the project published and says it rested on something a person typed. The same class on a machine the rule does not cover answers **no statement**, and somebody else's repository on the same forge reaches none of it** | auto |
| **I6** | **The demo vendor's own words** | **Come from its content files, in the language the client asked for — receipts, reply notes, refusals — or in English where it has no other. They were German literals in the source, on every screen; none of them is written in the source any more** | auto |

## Probes

| # | Case | Expected | Cover |
|---|---|---|---|
| P1 | All machine probes readable | Collected; nothing transmitted yet | auto |
| P2 | Machine probe returns nothing (consumer card has no serial) | Arms the matching human probe rather than failing | auto |
| P3 | Human probe answer violates the declared pattern | Rejected locally, asked again — never after an RMA is opened. The hand-off always checked this; the question panel never did, until 2026-09-11 **Since 2026-09-15 all three panels that ask a person a question go through one rule, `Flow.readAnswers`. The need round was the last, and had checked no pattern at all: it resolved the moment somebody clicked, so it had nowhere to say no.**| auto |
| **P4** | **Human probe with choices** | **Answer constrained to the list — and the list arrives with none of it chosen. The first half was always true: the question is a `<select>` built from exactly `choices`, so nothing else can come back. The second was not. Three of the four asked with the first choice selected, so a person clicking through stated a fact about their machine, in their name, that they never chose. It is the rule the refusing button already follows: a panel may not answer for the person looking at it. One site had the empty option and the others had grown without it, which is how a rule kept in four places goes. A required field left unanswered no longer opens a case either — it could not happen while a choice was pre-selected, so nothing checked it** | auto |
| P5 | macOS refuses a probe via TCC after in-app consent | Divergence surfaced to the user, not shown as a read failure | open |

## Consent

| # | Case | Expected | Cover |
|---|---|---|---|
| C1 | Transmission declined | Nothing sent, nothing changed, incident closed honestly | auto |
| C2 | Execution declined | Nothing on the machine changes | auto |
| **C3** | **Consent panel focus** | **The *refusing* button holds focus, so a stray Return can never grant. Two checks, and the pair is the point: the source is held to it here, which is weak on its own because a panel could take focus back; and `scripts/drive_window.mjs` checks the live `document.activeElement` on **every** consent panel it meets, which is strong and needs a desktop. Every walk on 2026-09-12 reported "refusing button not focused on: none"** | auto |
| **C4a** | **The panel shows the values *as they will be sent*** | **It showed `FACTS` — what the machine read and what the person typed, raw — while the request anonymised on the way out. So a Windows account name sat on screen under a sentence promising "only this goes — nothing about you as a person". Both halves were true and the pair was a lie: the account name was not going, and nothing said so. That is worse than a leak for a product whose claim is that you can see what leaves — a person reading their own user name there has every reason to conclude the promise is false and stop, and one did. The panel renders the anonymised facts now, through the same client-side command the request uses rather than a second copy of the rules in JavaScript, and it says what was taken out** | auto |
| **C4b** | **A person can change their own words before they go** | **Asked for after seeing a real account name in a real panel: *can I make that `xxx` instead?* Only what they typed is editable — a reading is what the machine said, and editing it would make the report a fiction — and emptying a box withdraws that answer as `<id>.declined` rather than sending an empty string** **2026-09-15: the rules moved to `Flow.editable` and `Flow.applyEdits`, and gained one neither copy had — an edit naming something the panel did not show is ignored rather than added, so the edit box cannot introduce a fact nobody saw.**| auto |
| **C4** | **What is shown before sending** | **Exactly the fields that would travel, in the user's language — and now by construction rather than by coincidence. The panel was built by reading `FACTS` and the request read `FACTS` again: two reads of a mutable object with a person's decision in between. Nothing moved it, so nothing was wrong; but "nothing currently moves it" is a fact about today's code, not a property, and it is the kind that stops being true in a patch that looks unrelated. The panel's own snapshot is what travels, so a fact arriving after the panel is drawn cannot be sent without being shown — there is nothing left that could send it** **2026-09-15: no longer by construction either. `Flow.consent()` is a gate — a panel that showed a snapshot and got a yes grants it, naming who for, and a send asks the gate for its facts. A send with no panel in front of it throws instead of sending. Per destination, over a copy, and cleared with the incident.**| auto |

## Action vocabulary

| # | Case | Expected | Cover |
|---|---|---|---|
| A1 | Known action, valid parameters | Dry-run shown, then executed on consent | auto |
| A2 | Action id the client does not implement | Refused, naming the client's own vocabulary | auto |
| A3 | Path traversal in a parameter | Refused | auto |
| A4 | Shell metacharacters in a parameter | Refused | auto |
| A5 | Unexpected extra parameter | Refused | auto |
| A6 | Required parameter missing | Refused | auto |
| A7 | Mutating action | Leaves a `.bak` rollback copy | auto |
| A8 | Undo | Restores the file from `.bak` | auto |
| A9 | Undo with no backup present | Clean refusal, not a crash | auto |
| **A10** | **Dry-run text vs actual effect** | **Must match — a dry-run that under-reports defeats the whole consent design, and does so silently. Asserted inside A12, against the change it describes** | auto |
| **A11** | **A sibling of the sandbox root** (`../var-evil/x.toml`) | **Refused. The bound is on path components, not on a string prefix — a prefix comparison accepts this, and `A3` misses it because `../../etc/passwd` fails the extension pattern before any path check runs** | auto |
| **A12** | **The agent fixes it, and the fix holds** | **The arc nobody had written down. A1 to A11 each check one step and every one of them passes on a machine where the product does not work at all. This walks it: a broken configuration file, the sentence the user is shown before anything happens, the change, the check that the change is real and that unrelated lines survived, the backup that existed before it was needed, an undo that restores the file byte for byte, and the report that says `resolved` — which is a claim about the world, made here by the only path that changed the world and changed it back** | auto |

## Remedy

| # | Case | Expected | Cover |
|---|---|---|---|
| R1 | Remedy signature valid | Acted on | auto |
| R2 | Remedy signature invalid | Refused; never acted on | auto |
| R3 | Vendor abstains | No action; routed to a human with the reason | auto |
| R4 | Vendor finding contradicts its own published documentation | Both shown side by side | auto |
| **R6** | **A finding never asserts a fact the endpoint did not have** | **The advisory skill's answer says the path was resolved against the *installed* version rather than against the manual, which is its entire reason to exist — and with no version read it formatted Python's `None` into that sentence and asserted it anyway. Shipping, and invisible: every fixture supplied a version, and the read is a file that exists only where the program is installed, so the branch was taken on every machine that is not a customer's and on none that any test ran on. The rule is general, so the case checks the shape rather than the sentence: no generator may put `None`, `null` or `undefined` in front of a person, in a finding, its evidence or an abstention — each of them reads as the program admitting it lost track rather than as an answer** | auto |

## Reporting

| # | Case | Expected | Cover |
|---|---|---|---|
| T1 | Report declined | Nothing sent | auto |
| T2 | Report on a known issue | Receipt names the fix version and the count | auto |
| **T7** | **The whole report path, end to end** | **Built, stamped with the pseudonym, validated against the published schema, sent, and a receipt comes back. Nothing exercised this before: the validation on the way out could have refused every report this client produces and no test would have noticed** | auto |
| T8 | A report that does not match the schema | Never leaves. This client must not be the one that sends a vendor something the specification says is not a report | auto |
| T3 | Rare combination | Counted but held below the threshold, and the user is told why | auto |
| **T3a** | **The same client reporting the same combination five times** | **Still held — the threshold counts distinct pseudonyms, never submissions. The client sends `pseudonym` and `epoch`, and the operator server has counted them since the catch-all was retired (`SV13`, `GR1a`). A vendor's own report service did not: `vendor/reports.py` counted posts, so on that branch one person crossed the threshold alone. It keeps keyed digests of pseudonyms now, and a report with no pseudonym is receipted but not counted — otherwise leaving the field out would be the way across** | auto |
| T4 | Serial number and free text | Never travel | auto |
| **W3** | **The window's script has a syntax error** | **Caught. The window is a JavaScript program and only a parser can say it is one — every other check on that file reads it as text and passes happily on a file that cannot run. The failure is total: no handler binds, no label is filled, and the user gets the layout drawn with every word missing** | auto |
| **W4** | **A page's script has a syntax error** | **Caught, by the same parser `W3` runs over the window. These pages fail the same way and for a second reason too: a Content-Security-Policy hash that stopped matching blocks the script silently** | auto |
| **W5** | **A page served from a directory rather than a route** | **Refused. `SV21` and `SV67` prove no path produces another vendor's figures by enumerating the routes; a mount collapses its whole subtree into one and a dropped-in file becomes a public URL with no review. Middleware is refused too — CORS here would make the claim token a cross-origin credential** | auto |
| **W6** | **A page assigning a log entry to `innerHTML`** | **Refused. Anyone may start a claim on any host, so log entries and index rows carry values a stranger chose; writing them as markup makes the viewer a stored-XSS sink on the operator's own origin** | auto |
| W7 | A page fetching something it was not meant to | Refused — each page's reachable URLs are an allow-list, so a page cannot quietly become a new disclosure route, and no public page names the operator's own listener | auto |
| **W8** | **A public page showing a per-project figure** | **Impossible. The pages carry no count and the data behind them carries none either, and the participant list is ordered by name — an ordering is a ranking wearing different clothes** | auto |
| W9 | A public page linking an internal document | Refused. `PUBLISHING.md` puts those on the internal side, and a public page citing one publishes it | auto |
| **P8** | **Nothing published names an internal document or this machine** | **`W9` checks the served pages; this checks the tree they ship inside, which is where it went wrong twice in one day — four published files ended a paragraph with a reference the public repository does not contain, and the maintainer's own home path sat in three more. `PUBLISHING.md` says to scan every time rather than once, and a checklist item nobody can run is a wish, so this is the item. Walked rather than asked of git: the first version called `git ls-files`, which answers "dubious ownership" in the container and exits 128, so the list was empty and the case passed over nothing — it is now asserted that the walk finds more than a hundred files before any of them is read** | auto |
| **W20** | **A maintainer loads the files they already publish into the builder** | **They come back meaning exactly what they meant. engram's files and both worked examples are posted to `/validate`, which returns ingest's own parse; the builder's `Emit.fromParsed` turns that into the form and `Emit.files` writes it back; the second parse equals the first key for key, with the same trees. What the form has no field for — a reading that also asks a question, a condition it would write differently, another language, more actions — is kept verbatim and said beside it. The originals must carry no warnings: the first import found the Python example answering a class its manifest never declared. Shown able to fail by dropping the kept fields** | auto |
| **W19** | **The files the builder on `/publish/build` writes** | **Taken by the mirror. The writer is a block with no page in it, run in node on real drafts — readings from the vocabulary, a question with choices, conditions with operators and "one of", a version `1.10` YAML would read as a number, a glossary, an escalation, and the example the page offers — and what it writes is sent to `/validate`, which runs the ingest checks (`SV108`). Every scalar is written as a JSON string, so YAML's typing never changes what a maintainer typed** | auto |
| **W18** | **A list longer than a screen, on any page** | **Paged with numbered pages, filtered in the browser over what it already holds, and ordered the ways that list allows — one shared component, `list.js`, on the dashboard, `/projects` and `/log`. It replaced a "Show 20 more" button (eleven presses at 263 configurations) and, on `/log`, one fixed page of the first thousand entries shown as the log: at 2,501 the newest 1,501 were missing. `/projects` still orders by host and nothing else, because any other order is a ranking. The file is served as JavaScript with `nosniff`, a page that loads it permits `'self'` and no other page does, and every JSON answer carries `nosniff` so none can be loaded as a script in its place** | auto |
| **W17** | **A maintainer's token refused by the dashboard** | **Said as a refusal of the host and token, in an announced panel directly under the button, with both fields marked and the cursor back in the token — what to check, that a revoked or expired token is refused the same way, and where a new one comes from. It was one grey line reading "this dashboard is private to whoever controls the domain", under two paragraphs, and a maintainer who had pasted the token without its first characters read it as the page not working. It still does not say which kind of refusal it was (`SV78`). A server error and an unreachable server are said as such, not as a credential problem. And a token is remembered only once it has worked — it was stored before it was tried** | auto |
| **RS1** | **A Rust test that no case names** | **Runs anyway. The cases above name 83 of the client's tests and there are 31 more — canonicalisation, the ledger, the window contract — that are implementation detail rather than behaviour this document should promise. Running only the named ones let one of them assert the wrong thing for two commits while this suite reported green. A case may go unnamed; it may not go unrun** | auto |
| **T11** | **A publisher asks a free-text question — "paste the error"** | **The answer is withheld by *default*, not forever. It is shown to the user verbatim and editable, the recipient is named, the refusing button holds focus, and only then does it travel — with `granted`, `destination` and a year-month `granted_at`, enforced by a CHECK on the receiving side. An error message copied by hand is the most useful thing a publisher can get and the one thing no policy can generalise, so refusing it outright would be the wrong kind of safe. **Which facts get a box is `stated[id] === null`, never `dropped`**: the report holds back machine readings too, and offering somebody an editable box to retype a measurement is what the reading panel exists to refuse. Emptying a box withdraws the words, and emptying every box attaches nothing rather than recording a second consent against no text — `Flow.withheldWords` and `Flow.consentedText`** | auto |
| **T10** | **A probe carrying both a `read` and a question** | **The reading is tried first and the question asked only if it came back empty — one fact, written once, arriving either way, and the report says which. `choices` on that question are what make the answered form travel at all: bounded answers may, free text never does, so an authored question is the difference between a fact the publisher receives and one they never see** | auto |
| **T9** | **A value a person typed, and one the machine read** | **They travel in separate maps. `observed` carries measurements and nothing else, so a recipient that ignores provenance loses facts rather than mistaking a claim for a measurement; `stated` carries what the person supplied. A key is in exactly one. Kept apart rather than flagged in place, because the failure to design against is a consumer that does not look** | auto |
| T5 | Version numbers | Of other software, coarsened (`610.57.04` → `610.57.x`): a driver build says more about the machine than about the problem. A program's **own** version, read by `program_version` or `container_image_version`, travels exactly — `1.2.2`, because its last component is the fix the publisher shipped | auto |
| T6 | Report payload | Carries no timestamp and no identifier | auto |

## Aggregation

| # | Case | Expected | Cover |
|---|---|---|---|
| G1 | Fewer than five contributors | No index published, and it says so | auto |
| G2 | At the threshold | Median, spread and contributor count published | auto |
| G3 | Ballot stuffing (30 % forged extremes) | Median unmoved; the spread exposes the attempt | auto |
| G4 | Vendor that never acts on reports | Report button withheld, with the reason | auto |
| G5 | Contributions batched across vendors | Must be impossible — the set of vendors is a profile of installed software | auto |
| **G6** | **A vendor's standing is checked before the button is offered** | **Consulted first, not after. Asking a user to spend effort on a vendor that has never once responded teaches them the channel is worthless — and that damage is shared by every other vendor** | auto |
| G7 | Contributing to the index | Its own decision, asked separately, with a preview that sends nothing and one request per vendor | auto |
| G8 | Acknowledgement versus action | Only a state that changed something counts; an auto-reply must not score as responsiveness | auto |
| G9 | Too few reports to judge | Says so, rather than quoting a percentage computed from three observations | auto |

## Conformance to the published vocabulary

The action and read vocabularies live in `spec/vocabulary/`, and both sides are
held to that one file rather than to each other. They had drifted: the two
implementations opened with the same sentence about being *"the boundary that
bounds generation"* and listed different actions, while `A2` asserted only that
one id common to both appeared in a refusal — so nothing could see it.

| # | Case | Expected | Cover |
|---|---|---|---|
| **SP1** | **A proposed action outside the vocabulary** | **Refused at the gate, naming what is permitted — including a missing parameter, an undeclared one, and one that does not match its pattern** | auto |
| SP2 | A read instruction outside the vocabulary | Refused: an unknown op, a tool off the allow list, an argument outside the tool's pattern, an unnamed root, a traversing glob, a denied path | auto |
| SP3 | The vendor's own skills | Conform — the CI half of the gate, which fails a bad skill before it is served | auto |
| SP4 | The client's action vocabulary vs the spec | Identical, or the file a vendor implements against has stopped describing the client `[rust]` | auto |
| SP5 | The client's read vocabulary vs the spec | Identical: ops, tool patterns, deny list, limits and catalogue | auto |
| SP7 | The client's wire structs vs `spec/schema/` | Identical field sets — a vendor implementing against the published document must not produce something this client silently ignores | auto |
| SP8 | A field a vendor added after this client was built | Ignored, never a refusal. Extending must not break a client that predates the extension | auto |
| SP9 | A signed remedy that does not match the published shape | Refused, naming the vendor — not carried into the window to fail there as a missing key mid-consent | auto |
| SP6 | The spec's own normative claims | Hold against the code — the integer rule, every action described, the `.declined` convention, the closed set of reply channels. Prose that nothing checks is prose that drifts | auto |

## Interop and canonicalisation

| # | Case | Expected | Cover |
|---|---|---|---|
| J1 | Rust verifies a card signed by the Python vendor | Passes — the failure mode otherwise looks like an attack | auto |
| J2 | Rust rejects the same card after one byte changes | Refused | auto |
| J3 | Object keys sort by UTF-16 code unit | Not UTF-8 byte order | auto |
| J4 | Solidus is not escaped | `/` stays `/` | auto |
| J5 | Control characters use short escapes | `\t`, `\n` | auto |
| J6 | Floats | Refused rather than approximated | auto |

## Platform

| # | Case | Expected | Cover |
|---|---|---|---|
| L1 | `doctor` on Linux | Reports the tools actually present | auto |
| **L6** | **A reader that goes away** (`demo \| head`) | **A quiet SIGPIPE, the way every ordinary Unix program ends. Not a panic — which the release profile's `panic = "abort"` turns into SIGABRT and a multi-megabyte core file, once per invocation, while the output still looks right and the pipeline still exits 0** | auto |
| L7 | An unknown subcommand | Refused, naming what is possible | auto |
| L8 | `--version` | The client names itself and its version. It asks other programs exactly that, and should be able to answer it | auto |
| L9 | The Windows installer | Installs for the current user with no administrator, and "delete application data" removes `%APPDATA%\podshl`, where the client's data actually is — not only the folder named after the bundle identifier | auto |
| L10 | A release build's defaults | Reads no log key and no vendor directory relative to where it was started (`../var/` exists in debug builds only), and keeps its state in the config directory rather than the working directory | auto |
| **L2** | **Wayland** | **The client sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` in its own process before the window is created, and leaves any value the person set. Walked 2026-09-14 on Omarchy 4.0.2 (Hyprland 0.56.2, webkit2gtk 2.52.6, NVIDIA 610.57.04), natively rather than under Xvfb: without it the process exits with `Gdk-Message: Error 71 (Protocol error)` and **no window is ever created** — and because the desktop entry is `Terminal=false`, the person clicks an icon and nothing happens, with no message anywhere. With it, a 946×1030 window rendering the start panel. Not decided by driver version: `spec/example-desktop/` publishes `< 555` as the boundary and 610 fails, so the boundary is not known, and a wrong guess fails invisibly. `manual` because the part that matters needs a Wayland session; `dmabuf_renderer_setting` is unit-tested so the override cannot be quietly dropped. Every screenshot this project ever took ran under Xvfb, which is X11 and cannot show this** | manual |
| **L3** | **Windows branch** | **Builds, runs and passes the whole suite on Windows, and every path has been walked through the real window: published (including a log excerpt read from a file), vendor across all three skills in four languages, the model path against Ollama, the hand-off to a person, free text, and the settings panel. Bundled since 2026-09-13: `scripts/build_windows_installer.ps1` builds a per-user NSIS setup with the operator's address and log key compiled in. That setup was installed silently (no administrator; nothing under HKLM), the installed program started from an unrelated directory with no `VS_*` or `PODSHL_*` in its environment walked the published path to a verified answer, and the silent uninstall removed the program, both shortcuts and the uninstall entry. `manual` because it needs a Windows desktop and is walked per release. **Not signed** — SmartScreen names no publisher; a certificate is a decision, not work** | manual |
| **UI1** | **The flow decided, and the flow walked** | **Three layers outside this table, in `client-rs/uitest/`, run by `scripts/ci.sh` rather than by `run_testcases.py` — so they are listed here as `manual` and are not claimed as `auto`, which this document's own drift check would otherwise report as missing. The *decisions* (`ui/flow.js`) are tested with no page and no network in milliseconds; the *shipped page* is served to a headless browser under the application's own CSP with `invoke` bridged to the real binary; the *real window* is driven over WebView2's DevTools protocol, which is Windows only and is the one a person still runs by hand. Every case in the first two is a defect this project shipped** | manual |
| L4 | macOS branch | Unverified | open |
| **L5** | **Windows elevation** | **Nothing in the vocabulary needs it, and the case is now that check rather than a missing binary. `actions.json` publishes three actions — `report_only`, `set_config_key`, `restore_backup` — and the last two write configuration files under roots the user granted, which is unprivileged by construction. So the elevated helper would be a privilege-escalation binary with nothing to do, shipped by a product whose argument is bounded effect. The rule stands and is worth keeping written down: if an action ever needs privilege it goes through a **separate** binary, because an application able to elevate itself in-process cannot honestly claim bounded effect. This case fails the day the vocabulary gains such an action and the helper does not exist** | auto |

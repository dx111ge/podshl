# Contributing

Most of this repository is developed elsewhere and published here, so a pull
request against it cannot simply be merged — the working copy is not reachable
from outside, and the next publication copies over what is here. **Language
files are the exception**, and they are the exception on purpose.

## A language

**One file, one line, one pull request.** That is the whole of it, and the
product's argument depends on it: a support client that speaks one language is
not a horizontal layer, so the localisation matrix is meant to collapse onto the
client rather than onto everybody it talks to.

1. Copy `client-rs/ui/i18n/en.json` to `client-rs/ui/i18n/<code>.json`.
   English is the source of truth — every other table answers to it.
2. Translate the values. Leave the keys alone.
3. Add one line to `client-rs/ui/i18n/languages.json`: your code, and the name
   your language calls itself. *Deutsch*, not *German* — a picker that offers
   "German" to somebody who reads only German has missed the point.
4. Open a pull request with those two files.

The publication script refuses to overwrite a language file that exists here and
not in the working copy, so a merged contribution cannot be quietly deleted by
the next release.

### What the checks will hold you to

* **Every key, or none.** A missing key falls back to English silently, which in
  a consent dialogue reads as a design choice rather than a gap. `I1` fails the
  build rather than shipping that.
* **Every `{placeholder}` that English declares.** A translation that drops
  `{n}` renders a sentence with a hole where the number should be.
* **`_name` is your language's own name for itself**, and must match what you put
  in the index (`I7`).
* **`_one` and `_other`** are two separate keys where a count changes the wording.
  "1 things" was once written into every language as "thing(s)", which is a form
  no language has. Use as many as your language needs — if yours has more than
  two, say so in the pull request and the keys will be added.

Do not translate `no_language`. It is the sentence shown to somebody whose
language this client does *not* have, it is deliberately read from the English
table, and a translated copy is never used.

### What no check can do

`I1` enforces completeness, not correctness. **Nothing here can tell a good
translation from a confident wrong one**, and several of these sentences are the
text a person decides on before letting this program read their machine. That is
why a language wants somebody who speaks it, and why a pull request from one is
worth more than any amount of tooling.

If you maintain a language, say so in the pull request. Changes to that file can
then be routed to you.

## Anything else

Open an issue. Code changes are carried into the working copy by hand, so a
patch or a clear description is more useful than a branch — and a defect report
is more useful than either.

## Security

Do not open an issue for a security finding. `SECURITY.md` says where it goes.

# Reporting a security finding

Mail **sven.andreas@gmail.com**. Nothing else is required — no form, no
account, no coordinated-disclosure agreement signed in advance.

If the operator server is running, `/.well-known/security.txt` on it carries the
same contact in RFC 9116 form, with an expiry. If that expiry has passed, the
file is out of date and this document is the current answer.

You will get an acknowledgement within **72 hours**. If you do not, assume the
mail did not arrive rather than that it was ignored, and try again.

## What this project is asking you to look at

The client **reads a stranger's machine and changes things on it**, under
instructions fetched over the network from a third party. That is the shape of
the thing, stated plainly, and it is why findings here matter more than the
project's size suggests. The parts worth your attention:

| | |
|---|---|
| **The read and action vocabularies** | Both live in the client (`spec/vocabulary/`). A publisher chooses an operation; a publisher must never be able to introduce one. Anything that gets an operation executed which is not in those files is the most serious class of finding here |
| **`program_version`** | The one read that starts a program a publisher named. Bounded by a bare name, a closed set of flags, a deny list of shells, launchers and power, privilege and disk tools, a refusal of anything in the operating system's own directories, a time limit, and a version token as the only output. A way to get a program started with another argument, a program the user did not point at, or any of its other output off the machine, is a finding. So is a program on the search path that does real work when handed `--version` and is not on the deny list — tell us its name |
| **Log excerpts and the anonymiser** | A container's output or a file the user names is loaded for display only; what the user keeps is anonymised before they are asked. The anonymiser is a floor, stated as one: something identifying that it misses is worth a report with the shape of it, and a way to make *loaded* text leave without the free-text consent is a serious one |
| **Argument construction** | A publisher supplies parameters to a tested operation. A parameter that escapes into a shell, a path that escapes its root, an argument that turns a read into a write |
| **The consent path** | Read, transmit and change are three separate consents; the refusing button holds focus; every change is dry-run first and reversible. A way to get any of those skipped, defaulted, or raced |
| **What travels** | `observed` is measured, `stated` was supplied by a person, free text travels only through an explicit consent naming the recipient. Anything identifying that leaves anyway — a path with a username in it, a hostname, a serial |
| **Where readings go to be matched** | For a published project the readings go, as read, to the operator's `POST /diagnose`; where nobody published, to the model provider the user chose. Both behind a transmit consent that names the recipient and shows the values, and the operator uses them for one walk and does not store them. A way to get readings sent without that consent, to a recipient other than the one named, or to have them retained, is a finding |
| **The mirror as an instruction channel** | `/mirror/<host>` is unsigned JSON, and through it the operator — or whoever controls the operator — hands the client a read plan and proposed changes. That is bounded by the vocabularies and by consent, not by a signature: an operator cannot make the client do anything the vocabularies do not name or the user did not agree to. A way past either bound through the mirror is a finding; that the channel is unsigned is known |
| **The model key** | An API key goes only to the host its provider preset names. A way to make the client present it elsewhere — through `llm.json`, a redirect, or a preset — is a finding |
| **The transparency log** | An inclusion or consistency proof that verifies against a tree the entry is not in, a signed tree head that can be forked, an append that is not append-only |
| **The anchor challenge** | Control of the URL is the whole credential. A way to attest a domain you do not control, or to un-enrol one you do not |
| **The pseudonym** | Per subject, per epoch, HMAC over a salt that is deliberately not in the database. A way to link one reporter across two projects or two months |
| **The k-threshold** | Nothing surfaces below five distinct reporters. A way to read anything below that floor, including by making one reporter look like five |

Reports about the specification are as welcome as reports about the code. A
protocol that cannot be implemented safely is a defect in the protocol.

## What is already known and is not a finding

* **The transparency log is public and fetched whole.** There is no per-domain
  lookup, deliberately — one would tell the operator which software everyone
  runs. That the log is enumerable is the design, not a leak.
* **The published half of a challenge is not a secret.** It has to be readable
  at a public URL; that is what makes it proof of control. The *other* half is —
  a claim is a digest you publish and a preimage you keep, and verification
  requires both. Reading the file is not claiming the host, and a report that it
  can be read is not a finding.
* **The demo's `PODSHL_ALLOW_LOOPBACK`** exists so the local counterparty can be
  reached by the crawler. It defaults to off and `GET /` reports when it is on.
  Finding it enabled in a *production* deployment is a finding; finding the
  switch is not.
* **Any operator can see what the operator can see.** The honest answer to "what
  can the operator do" is in `LICENSING.md` and on `/security`. That the
  operator holds a mirror of public files is not a disclosure.
* **Free pseudonyms can cross the k floor.** A pseudonym costs nothing and a
  client can reset it at will, so one person with patience can look like five.
  Known; the mitigation — a cost to minting one — is not built. A *cheaper*
  way than that is still a finding.
* **Rate limiting is the reverse proxy's.** The server does none of its own,
  by design; `deploy/README.md` says what the proxy must do. A flood against a
  server with no proxy in front of it is a misdeployment, not a finding.
* **The proxy's access log sees `/dashboard/<host>` and `/mirror/<host>`.** A
  line naming which host was claimed, or which software a client asked about,
  is in whatever log the proxy writes unless it is told to redact those two
  segments. `deploy/README.md` shows how; a deployment that does not is a
  disclosure to whoever reads that log, and known.
* **Log key rotation is by hand.** It is a `log_policy` entry, and nothing
  automates writing one. That is a gap in operations, not in the protocol.
* **The client's DNS lookup is only as good as the resolver path.** `VS_TRUST`
  defaults to resolving a vendor's key through the system resolver, and the
  client does not validate DNSSEC itself; a resolver that lies, or a network
  that answers for it, answers the key lookup too. The signed card and the log
  are what stand behind that; a poisoned answer that survives them is a
  finding.
* **`llm.json` is plain text and user-writable.** It holds the provider's base
  URL and model name under the user's config directory; the key is in the
  credential store and goes only to the preset's host. Editing one's own
  endpoint is not a finding. A way for anything other than the user to write
  it is.
* **A takedown notice is read by a person before anything happens.** `POST
  /notice` records it as pending; the anchor changes only when someone acts
  from the ops view. That a notice does not take effect on its own is the
  design, and the delay is not a finding. The route requires
  `notifier.statement_of_good_faith = true`; that it refuses without one is
  also the design, and it is not an authentication check — filing is open to
  strangers by necessity.

## Found here already, and what each one was

Listed because the shape of what has already gone wrong is the most useful thing
anyone looking can be given, and because a project that only publishes its clean
bill is not saying anything. Each has a case in `TESTCASES-SERVER.md` or in the
client's own suite that performs the attack rather than describing it.

| | |
|---|---|
| **A public file was treated as a credential** | `verify` took no authentication and checked only that the challenge file was *there* — and maintainers are told to leave it published. Any passer-by could take a project's dashboard, lock out the maintainer and withdraw the project. A claim now has a published half and a kept half. `SV86` |
| **A registry read was arbitrary code execution** | Both parameters were interpolated into a PowerShell `-Command` string, unescaped, behind a consent screen that said "read a registry value". No command string is built any more |
| **A read could leave its granted root** | Twice: `..` was not seen by a prefix test, and neither was a symlink. Both sides of the check are resolved now, and `..` is refused outright. `SV84` |
| **A name could answer twice** | The host was resolved to be validated and resolved again to connect. The connection is pinned to the address that was checked. `SV87` |
| **A percent-encoded `..` passed both containment checks** | One read raw text and the other compared raw text, while the host that serves the file decodes. `SV88` |
| **The deny list did not name the common words** | `password`, `api_key` and `private_key` went through, and so did any homoglyph spelling of a word that was listed |
| **One bad source took down the whole crawl** | And came back at the head of the next one. Each source is isolated now. `SV90` |
| **A deleted solution went on being served** | Removing the file is how a maintainer withdraws a harmful remedy, and it did nothing. `SV89` |
| **A client's pseudonym was unstable** | Two concurrent first uses minted two identities, and a changed pseudonym is a *new reporter* to the counter — one person could cross the floor of five alone |
| **The client hung for ever on a stalling host** | Discovery had no timeout at all, so an endpoint that accepts and never answers froze it permanently. Not remote code execution, but it is a denial of service any vendor could inflict on every user pointed at them, and "nobody there" is a state this design promises to *state* |
| **A manifest could ask for shell history** | `.bash_history`, `.python_history` and PowerShell's history were on no list, so the gate accepted a read of every command somebody had typed. `_history` is on the deny list now |
| **A granted project directory outlived its diagnosis** | Nothing cleared it except the user emptying a text field, so a directory granted once stayed readable for every diagnosis after. A new question ends the last incident now. `EN4` |
| **A development root was honoured by release builds** | `VS_EXTRA_ROOT` widened the read allow-list from one environment variable on a shipped binary. Only a development build reads it now. `EN5` |
| **The test suite changed the client's identity** | It deleted the installed client's secret to test first use, so one person reporting before and after a test run counted as two reporters — toward the same floor this page lists as worth attacking |
| **A reading that contradicted a rule got that rule's answer** | Where one solution was decided only by a question, the fallback for "I would rather not say" was also returned for a value that matched nothing. Not a disclosure, but a confident wrong answer is what the whole design refuses. `SV95` |

## Please do not

* Test against the public operator server if you can reproduce it locally.
  `docker compose up` gives you the whole thing, including its database, and a
  finding is reproducible or it is nothing. If you must, keep it to your own
  domain and your own anchors.
* Run anything that degrades the service for other people, or that touches
  another project's anchor, mirror or figures.
* File a notice-and-action complaint to report a vulnerability. That path
  exists for takedowns, it is public, and it is the wrong door.

## What happens next

There is no bug bounty. This is one person and there is no money in it, and
saying so is more useful than a page implying otherwise.

What there is instead:

* An acknowledgement within 72 hours, and a real answer — including "this is
  not a finding, and here is why" — rather than silence.
* Credit in the fix, under whatever name you want, or none.
* **A public record you can check.** If a finding leads to a change in what the
  operator holds, what the client may do, or what the log says, that change is
  itself in the log or in the repository. You should not have to take the fix on
  trust any more than anything else here.
* Disclose on your own timeline. **90 days is the default and it is not a
  condition of anything** — you are not obliged to wait, and a finding held past
  a fix helps nobody. If a fix needs longer than 90 days I will say so and say
  why, and that is a request rather than a demand.

## Supported versions

There is one: the current `main`. There are no releases old enough to have a
support window yet, and pretending otherwise would be a table with nothing in
it.

# podshl — the counterparty

**Not a client.** There used to be one here, duplicating `client-rs/`, and two
thirds of the suite pointed at it: green, plausible, and measuring an
implementation nobody installs. It is gone.

What remains is the other side of the wire — a vendor's own endpoint, the
neutral index, a website with no agent at all — plus the gate that holds a
document to `spec/vocabulary/`. The Rust client cannot be its own counterparty,
which is why this exists.

    mise run services   # vendor :8721 · plain :8722 · index :8723 · OSS project :8727
    mise run testcases  # the counterparty and conformance suite
    mise run demo       # five acts, driven by the client that ships

## What is real

* **Signed Agent Card** — detached JWS (RFC 7515) over the card's JCS-canonical
  form (RFC 8785), served at `/.well-known/agent-card.json` (RFC 8615), with the
  Legal Entity Identifier in the protected header. `jcs.py` is a real RFC 8785
  implementation: UTF-16 key ordering, no solidus escaping, floats refused
  rather than approximated.
* **Out-of-band trust** is the client's job, in `client-rs/src/trust.rs`. What
  this side does is publish the key material out of band in the first place —
  `var/ans_stub.json` stands in for the DNS lookup locally.
* **The conformance gate.** `spec_gate.py` validates a proposed action or read
  instruction against `spec/vocabulary/` — the same file the client is held to,
  rather than a second copy of the rules. It refuses an unknown action, a
  parameter that does not match its anchored pattern, an undeclared parameter, a
  tool off the allow list and a denied path. This is the ingest gate of
  `SERVER.md` in miniature.
* **Human probes.** A probe gated on `when_missing` fires only when the machine
  could not supply the fact — the serial case is not contrived: consumer cards
  really do report nothing, so the printed sticker is the only source.
* **Live contradiction of static documentation.** Measured on real hardware,
  not fixtures: `torch.bf16_reported=True` while `torch.bf16_native=False` on
  sm_75. The vendor's own KB page says the opposite.
* **Abstention with a destination.** Refusals, missing facts and unmatched
  problems all route to a human, carrying the record.
* **The no-A2A case, kept distinct from a verification failure.** `NoVendorAgent`
  means "there is nobody there"; `VerificationError` means "do not trust this".
  They lead to opposite behaviour, so they are never conflated. Act 1 of the demo
  runs against a plain website — which is what every vendor looks like today.
* **The report channel.** Received here; built and generalised on the client.
  A rare constellation is counted and held back, and the receipt is the entire
  reward. **The threshold counts distinct pseudonyms**, not submissions — the
  same client reporting five times counts once. `T3a` holds it here and `GR1a`
  on the server, both `auto`.
* **Two-way measurement.** `aggregate.py` and the index service turn per-client
  experience into a median, submitted one vendor per request so the set of
  vendors a client deals with is never disclosed. The client half is
  `client-rs/src/ledger.rs`; a vendor below 15 % does not get the button
  offered.

## What is not

* The generation step is rules (`GENERATOR_ID = "rules-…"`). A pinned model
  plugs in at the same interface under the same vocabulary constraint; nothing
  in the design depends on it being a model today.
* **`catchall/` is gone.** It was the last of the old design still running, and
  `SERVER.md` named both of its behaviours as things the server must not do — an
  unauthenticated gap report, and a free-text problem class. The client now
  reports to `server/`, which refuses a report carrying no pseudonym and derives
  the class from the signature. `aggregate.py` stays: `index_service/` still
  uses it for the responsiveness median, which is a different thing entirely.
* One JSON-RPC method (`SendMessage`) — not A2A conformance.
* The vendor and its LEI are illustrative.

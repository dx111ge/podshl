-- A takedown was reversible by an unauthenticated POST, and nothing said so.
--
-- `takedown.receive` sets `anchor.status = 'unknown'`, and the public statement
-- of reasons says exactly that: "the anchor is now `unknown` — the state of a
-- project that never registered". But `anchor.status` is *control liveness*, and
-- the only thing that writes it on a good probe is `sweep.record`, which sets
-- `status = 'live'` unconditionally. `POST /claim/{host}/verify` probes and
-- calls `sweep.record`.
--
-- So: anyone at all could POST to that route for a host whose challenge file was
-- still published — which, for a project that had been participating, it was —
-- and the anchor went straight back to `live`. No token, no header, no account.
-- The mirror stayed withheld and the attestation stayed withdrawn, so nothing
-- was served that should not have been; what broke is that the operator's own
-- public statement about what a takedown does became false, silently, through a
-- route with no authentication on it. For a `court_order` that is not a
-- cosmetic problem.
--
-- The fix is a fact the liveness machinery cannot overwrite, because liveness
-- and enrolment are two different state machines and this is the bug you get
-- when one column carries both. `taken_down_at` is set by notice-and-action and
-- is never cleared by any automatic path — reinstatement is a human reversing a
-- decision a human made, and it should look like one.
--
-- Self-withdrawal deliberately does NOT set this. `POST /claim/{host}/withdraw`
-- tells the maintainer "come back whenever you like", and a project that left is
-- not a project that was reported. Keeping those apart is the same distinction
-- `revoked_by_holder` and `superseded` already draw for tokens.
ALTER TABLE anchor ADD COLUMN taken_down_at    timestamptz;
ALTER TABLE anchor ADD COLUMN taken_down_seq   bigint;

COMMENT ON COLUMN anchor.taken_down_at IS
  'Set by notice-and-action only. While it is set, no probe may return this anchor to `live` — enrolment is not liveness, and a takedown must not be undone by a stranger republishing a file.';
COMMENT ON COLUMN anchor.taken_down_seq IS
  'The log sequence of the statement of reasons. The affected party is owed it, so the row points at it rather than at nothing.';

ALTER TABLE anchor ADD CONSTRAINT a_takedown_names_its_statement CHECK (
    (taken_down_at IS NULL AND taken_down_seq IS NULL)
    OR (taken_down_at IS NOT NULL AND taken_down_seq IS NOT NULL)
);

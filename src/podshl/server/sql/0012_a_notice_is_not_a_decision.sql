-- Four things that were true of the code and not of the schema.

-- 1. A notice acted on itself.
--
-- `POST /notice` takes no authentication — it cannot, a notice is filed by a
-- stranger — and it performed the takedown in the same request: mirror
-- withheld, attestation withdrawn, `taken_down_at` set. That made the
-- notice-and-action path a free, remote, unauthenticated un-enrolment of any
-- mirrored project, which is the weapon SERVER.md says the path must not be.
-- "Removing on notice is the requirement" is about a *person* removing on
-- notice; the route's job is to record the notice and hand it to one.
--
-- A notice is now `pending` until somebody on the operator's own listener acts
-- on it, and a decision can be reversed — `reinstated` — by the same person. A
-- takedown is still logged with a public reason, and so is its reversal.
ALTER TABLE notice DROP CONSTRAINT notice_action_check;
ALTER TABLE notice ADD CONSTRAINT notice_action_is_a_decision
    CHECK (action IN ('pending', 'degraded', 'refused', 'reinstated'));
ALTER TABLE notice ADD COLUMN reinstated_at  timestamptz;
ALTER TABLE notice ADD COLUMN reinstated_seq bigint;

COMMENT ON COLUMN notice.acted_at IS
    'When a person decided. NULL while the notice is pending; a notice that acts on itself is a weapon.';

-- 2. A served head could stop verifying under the key that signed it.
--
-- `sth.issue` rebuilt the head body from the row on every read, with the
-- *current* key's id in it. After a key rotation every stored head would be
-- served with a body the old signature never covered. The body that was signed
-- is stored beside the signature, and that is what is served.
ALTER TABLE sth ADD COLUMN body jsonb;

-- 3. A solution had no validators of its own.
--
-- The manifest carried an ETag and the solutions did not, so a 304 on the
-- manifest meant "unchanged" for the whole source — while a solution file edited
-- in place, with the manifest untouched, was never fetched again. Each solution
-- now remembers what the origin told it, and a 304 on the manifest is followed
-- by a conditional GET per solution.
ALTER TABLE solution ADD COLUMN etag          text;
ALTER TABLE solution ADD COLUMN last_modified text;

-- 4. One pending claim per host was a slot a griefer could hold.
--
-- `claim_start` refused a second caller for an hour while one claim stood, so
-- anybody could keep a maintainer out of their own registration by starting a
-- claim every fifty-nine minutes. A claim is a nonce hash and nothing else, so
-- there is no reason two cannot stand at once: each caller holds their own
-- preimage, `verify` finds the claim by the hash of what is presented, and a
-- successful proof retires every claim on that anchor. Nobody can lock a slot
-- because there is no slot.
CREATE TABLE claim_pending (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    anchor_id  bigint NOT NULL REFERENCES anchor(id) ON DELETE CASCADE,
    -- sha256 of the secret half. The hex digest is what the claimant
    -- publishes; the preimage is what they present. Never the value itself.
    nonce_hash bytea NOT NULL CHECK (octet_length(nonce_hash) = 32),
    issued_at  timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX claim_pending_nonce ON claim_pending (nonce_hash);
CREATE INDEX claim_pending_by_anchor ON claim_pending (anchor_id, issued_at DESC);

COMMENT ON TABLE claim_pending IS
    'Claims in progress. Several may stand for one anchor; verify finds one by the hash of the presented proof, and a success clears them all.';

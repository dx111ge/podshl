-- A token could be issued and never withdrawn, and a pending challenge could be
-- overwritten by a stranger. Both were invisible while claiming was two curl
-- calls nobody had been told about. A register page makes them everyone's.

-- 1. Tokens accumulated forever.
--
-- `claim_verify` INSERTed a new row on every success. There is no UPDATE and no
-- DELETE on this table anywhere in the codebase, so every token ever issued
-- stayed valid for its full 365 days: the live set only ever grew, and nothing
-- could shrink it. `revoked_at` has existed since 0001 and `_claimed_anchor`
-- honours it — nothing ever wrote it.
--
-- Re-proving control now supersedes what came before, so a leaked token is a
-- one-action problem rather than a year-long one. `superseded` and
-- `revoked_by_holder` are kept apart for the same reason a takedown degrades
-- rather than revokes: "revoked" reads as misconduct, and rotating your own
-- token is not misconduct.
ALTER TABLE dashboard_claim ADD COLUMN revoked_reason text;

-- Written with IS NOT NULL on both sides on purpose. A CHECK passes when it
-- evaluates to NULL, which is how the consent constraint in 0005 shipped
-- decorative — strongest exactly where the data was weakest.
ALTER TABLE dashboard_claim ADD CONSTRAINT revocation_states_a_reason CHECK (
    (revoked_at IS NULL AND revoked_reason IS NULL)
    OR (revoked_at IS NOT NULL
        AND revoked_reason IS NOT NULL
        AND revoked_reason IN ('superseded', 'revoked_by_holder'))
);

-- 2. A stranger could reset a challenge somebody was in the middle of.
--
-- `claim_start` rotated the nonce on every call with no authentication, so a
-- maintainer who published nonce A could have it silently replaced by nonce B
-- between publishing and verifying — and the verify failure told them their
-- file was wrong, when the truth was that we had changed the answer. Not an
-- authentication bypass: converting it still needs control of the host. A free,
-- remote denial of service on the one flow a new maintainer walks.
--
-- The nonce is not a secret — it must be published at a public URL — so
-- returning the existing one to an anonymous caller costs nothing, and makes
-- "come back tomorrow and finish it" work.
ALTER TABLE anchor ADD COLUMN challenge_issued_at timestamptz;

COMMENT ON COLUMN anchor.challenge_issued_at IS
  'When the current challenge nonce was minted. A live nonce is not rotated, so a stranger cannot reset a claim somebody is in the middle of.';

-- A published file proves that somebody controls this host. It does not prove
-- that the person asking is that somebody.
--
-- `POST /claim/{host}/verify` took no authentication and fetched the challenge
-- file: if the file was there, it minted a token for whoever asked and revoked
-- every token issued before. The challenge nonce is public by design and
-- `POST /claim/{host}` handed it to anonymous callers on purpose, and the
-- documentation tells maintainers to leave the file published forever so that
-- re-verification keeps working.
--
-- So for every project that followed the instructions, any stranger could take
-- the dashboard, lock the maintainer out of it, and withdraw the project.
-- `0001_core.sql` reasoned that "converting a nonce into a claim still needs
-- control of the host". It needs the host to be *serving* a file, which is a
-- fact anyone can ask us to check.
--
-- The fix separates the two things the one nonce was doing:
--
--   * `challenge_token` stays what it was — the value published at the
--     well-known path, checked forever by the sweep. Public, and it should be:
--     liveness is not a credential.
--   * `claim_nonce_hash` is new, and it is a *secret* half issued only to the
--     caller that started a claim. The value published during a claim is its
--     hash; the value that must be presented to verify is its preimage. A
--     stranger can read the published half off the public URL and still cannot
--     produce the half that was never published.
--
-- Stored as a hash rather than as the value, for the same reason
-- `dashboard_claim` stores one: a row that can be read is a row that can be
-- taken, and a base backup outlives the claim.

ALTER TABLE anchor ADD COLUMN claim_nonce_hash bytea;
ALTER TABLE anchor ADD COLUMN claim_issued_at  timestamptz;

COMMENT ON COLUMN anchor.claim_nonce_hash IS
    'sha256 of the secret half of an in-progress claim. Its hex digest is what '
    'the claimant publishes; the preimage is what they must present to verify. '
    'Never disclosed, and cleared once the claim is spent.';

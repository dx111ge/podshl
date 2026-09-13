-- `0012` moved a claim off `anchor` and into `claim_pending`, and left the
-- columns it had moved out of standing.
--
-- A column nothing reads is worse than no column. `0010` put the claim on
-- `anchor`; `0012` took the reading of it away, and `SV86` went on seeding
-- `anchor.claim_nonce_hash` and went on passing — against code that had
-- stopped looking at it. The next person to write a query would have found
-- two plausible places to ask and no way to tell which one the server
-- believes. There is one place now.
--
-- Separate from `0012` because `0012` has run: a migration that changes after
-- it is applied leaves two databases with the same version number and
-- different schemas, and nothing later can tell. The runner refuses it, which
-- is the rule doing its job rather than an inconvenience.
ALTER TABLE anchor DROP COLUMN claim_nonce_hash;
ALTER TABLE anchor DROP COLUMN claim_issued_at;

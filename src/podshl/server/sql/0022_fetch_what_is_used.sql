-- Fetch what is used (docs/INGEST-REDESIGN.md).
--
-- Every enrolled source used to be fetched on a timer whether anybody ever
-- asked about it or not: N + 2 requests a pass, at 100,000 projects about a
-- million a day, nearly all of them a `304` about a project nobody looked up.
-- From here a source is fetched on a timer only while it is *hot* -- used in
-- the last fourteen days -- and a *cold* one is checked when it is next asked
-- about, before anything is served from it. Anchors are checked weekly on
-- their own, because `stale` at fourteen days is a promise about the anchor
-- and not about how popular its project is.
--
-- Hot and cold are derived from `last_used`, never stored: a stored flag is a
-- second copy of a date, and the two would disagree the first time a job that
-- flips it did not run.

-- The day a source was last used, and nothing else: no time of day, no count,
-- nobody's address. Every existing source starts hot, so nothing changes on the
-- day this lands and the quiet ones cool over two weeks.
ALTER TABLE source ADD COLUMN last_used date NOT NULL DEFAULT current_date;

-- The last completed check of the files. What was fetched before this
-- migration counts, so an operator that updates does not start by treating
-- every project as never checked.
ALTER TABLE source ADD COLUMN last_checked timestamptz;
UPDATE source SET last_checked = last_fetched WHERE consecutive_silence = 0;

-- The last check that asked about every file, not only the manifest. A digest
-- in the manifest lets a check stop at a `304`; a digest can also go stale by
-- hand-editing a solution, so a hot source is still read file by file daily.
ALTER TABLE source ADD COLUMN last_full_check timestamptz;
UPDATE source SET last_full_check = last_checked;

-- The back-off after a failed on-demand check: nothing asks the forge again
-- before this, and the count doubles the wait up to an hour. A success resets
-- both.
ALTER TABLE source ADD COLUMN check_backoff_until timestamptz;
ALTER TABLE source ADD COLUMN check_failures int NOT NULL DEFAULT 0;

COMMENT ON COLUMN source.last_used IS
    'The last day a request needed this source''s content (the mirror card or a '
    'diagnosis) or its maintainer enrolled it. Written at most once a day, in its '
    'own transaction, never from inside a query. Hot = within 14 days.';
COMMENT ON COLUMN source.last_checked IS
    'The last check of this source''s files that completed, changed or not.';
COMMENT ON COLUMN source.last_full_check IS
    'The last check that asked about every solution file, whatever the manifest '
    'digests said.';
COMMENT ON COLUMN source.check_backoff_until IS
    'No on-demand check before this, after a failed one. NULL once one succeeds.';

-- The timer's question is now "which hot sources are due", and the weekly
-- anchor check's is "who has not been checked for a week".
DROP INDEX IF EXISTS source_due;
CREATE INDEX source_due ON source (next_fetch_at, last_used) WHERE mirror_state = 'serving';
CREATE INDEX anchor_recheck ON anchor (last_checked NULLS FIRST)
    WHERE verified_at IS NOT NULL AND taken_down_at IS NULL AND status <> 'unknown';

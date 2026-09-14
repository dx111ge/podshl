-- The columns and rules for the repository anchor `0018` made possible.
--
-- Split from `0018` only because PostgreSQL will not let a new enum value be
-- used in the transaction that adds it. Read the two together -- `0018` carries
-- the reasoning for why an anchor needs two URLs at all.


ALTER TABLE anchor ADD COLUMN probe_prefix text;

COMMENT ON COLUMN anchor.probe_prefix IS
    'Where the challenge and the published files are actually fetched from, when '
    'that is not `value` itself. NULL for a domain anchor, where the two are the '
    'same place. Set for a repository, where the forge serves file contents under '
    'a different host than the one a person recognises.';

-- The same rule `value` already lives under, and the same narrow loopback
-- carve-out `0006` made for it, for the same reason: the local counterparty has
-- to be ingestable or the suite tests nothing. A mirror served over a channel
-- anyone can rewrite is not provenance, whatever the commit says.
ALTER TABLE anchor ADD CONSTRAINT anchor_probe_prefix_is_https CHECK (
    probe_prefix IS NULL
    OR probe_prefix LIKE 'https://%'
    OR probe_prefix LIKE 'http://127.0.0.1:%'
    OR probe_prefix LIKE 'http://127.0.0.1/'
);

-- `anchor_url_is_https` was written when 'url' was the only kind that carried
-- one. A repository anchor carries a URL in exactly the same sense and must be
-- held to exactly the same rule; without this it would be the one kind that
-- could name a plain-HTTP location, which is the sort of hole a new enum value
-- opens silently.
ALTER TABLE anchor DROP CONSTRAINT anchor_url_is_https;

ALTER TABLE anchor ADD CONSTRAINT anchor_url_is_https CHECK (
    kind NOT IN ('url', 'repo')
    OR value LIKE 'https://%'
    OR value LIKE 'http://127.0.0.1:%'
    OR value LIKE 'http://127.0.0.1/'
);

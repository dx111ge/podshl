-- Two reasons ingest gave, and gave to nobody (`SV105`).
--
-- `rebuild_all` does not fail a source for a class whose decision tree will not
-- derive; it returns why, "so it can reach the person who can act on it". And a
-- refused manifest or solution keeps the previous version served, returning
-- why. Both reasons went into a return value the worker discards. A maintainer
-- whose trees never derived saw "no decision tree could be derived" and nothing
-- else; one whose change was refused saw the change not arrive.
--
-- Both are about the project's own files and are shown only on its own
-- dashboard. `last_refusal` is cleared once a version is accepted, so it never
-- describes a problem that has been fixed.
ALTER TABLE source ADD COLUMN last_refusal text;
ALTER TABLE source ADD COLUMN classes_without_a_tree jsonb NOT NULL DEFAULT '{}';

COMMENT ON COLUMN source.last_refusal IS
    'Why the most recently fetched version was refused, while the previous one keeps being served. NULL once a version is accepted.';
COMMENT ON COLUMN source.classes_without_a_tree IS
    'Problem class to the reason its decision tree did not derive, from the last accepted version.';

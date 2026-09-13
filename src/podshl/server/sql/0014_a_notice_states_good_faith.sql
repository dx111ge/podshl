-- The form asked for it and the route threw it away.
--
-- `notice.html` has always put `statement_of_good_faith: true` in the notifier
-- object, and `POST /notice` accepted a notice with or without it. So the one
-- thing the DSA asks a notifier to assert — that they believe what they are
-- saying — was collected by a checkbox and discarded by the server, which is
-- worse than not asking: it looks like a safeguard and is not one.
--
-- Filing is free, remote and unauthenticated, and it has to be: a notice comes
-- from a stranger. What can be asked of a stranger is that they say, in the
-- request, that they mean it. That does not stop anybody, and it is not meant
-- to — it makes a careless notice a statement somebody made rather than a
-- button somebody pressed, and it is the record the operator needs when the
-- decision is questioned later.
--
-- `NULL` is not "no statement", it is "filed before one was required". Nothing
-- backfills those to true: writing a statement nobody made is the opposite of
-- keeping a record. `false` is refused outright, so a notice can never carry a
-- denial of the thing it had to assert.
ALTER TABLE notice ADD COLUMN good_faith_stated boolean;
ALTER TABLE notice ADD CONSTRAINT notice_never_records_a_denied_statement
    CHECK (good_faith_stated IS NOT false);

COMMENT ON COLUMN notice.good_faith_stated IS
    'The notifier''s statement that they believe the notice is accurate. Required since 0014; NULL on notices filed before it was, and never backfilled.';

-- A value a person supplied is not a value something measured, and the two must
-- not be able to wear the same name.
--
-- `observed` used to carry both, so a maintainer reading a report could not tell
-- a reading from an answer. That matters exactly where it costs most: a solution
-- that matched on a measurement and failed is a defect in their rule and worth
-- their time; one that matched on a value somebody typed and failed may be
-- nothing of the sort. Arriving identical, the second wastes the first's credit.
--
-- Separate column rather than a flag inside `observed`, for the same reason the
-- wire format splits them: a query that does not know about provenance reads
-- `observed` and gets only measurements, which is wrong-but-safe rather than
-- confidently wrong.
ALTER TABLE observation ADD COLUMN stated jsonb NOT NULL DEFAULT '{}'::jsonb;

-- Rows written before the split carry readings and answers mixed together in
-- `observed`, and nothing can separate them after the fact. They stay as they
-- are: an empty `stated` here means "not recorded", not "nothing was stated",
-- and inventing a value would be worse than the gap.
COMMENT ON COLUMN observation.stated IS
  'What a person supplied. Empty on rows written before this column existed, where it means unrecorded rather than none.';

-- Four things that could only ever be filled by the suite that tested them.
--
-- `observation.tried_link_id` was meant to say which solution a report was
-- about, so "worked for some and not others" could be a join. No report ever
-- filled it: the protocol carries `decided_on`, never a solution id, and
-- `POST /report` never passed one. Every row that had one was written by
-- `SV15` or `SV32` directly.
--
-- `mixed_outcome_signal` joined on that column, so it was empty everywhere
-- but the development database. `cluster_partition` was written by
-- `repartition.save`, which nothing called, and it recorded a switch *inside*
-- a cluster — where no switch can do anything, because a cluster's signature is
-- every fact its reports carried, so they agree on every value. The GIN index
-- on `observed` served only those in-cluster reads, and it is on the hottest
-- write path this server has.
--
-- What replaced them is not a column: which answer an outcome is about is
-- found by walking the project's own solutions against the configuration, and
-- where an answer helped some configurations and not others is computed from
-- the rows the dashboard already shows (`repartition.forks`, `SV32`). Nothing
-- is stored, because a tree is derived from `answers.when` and a partition kept
-- here would be a second document drifting from it.
DROP VIEW IF EXISTS mixed_outcome_signal;
DROP TABLE IF EXISTS cluster_partition;
DROP INDEX IF EXISTS observation_repartition;
DROP INDEX IF EXISTS observation_by_link;
ALTER TABLE observation DROP COLUMN IF EXISTS tried_link_id;

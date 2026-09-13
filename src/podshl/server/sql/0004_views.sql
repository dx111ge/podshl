-- The operator's own view, and the only two figures that are ever public.

-- SV23. A materialised view rather than a query, for two reasons: the ranking
-- never touches the request path, and it becomes one object that can be
-- permissioned and audited. The k-threshold is inside the HAVING, so it holds
-- even for the operator's own curiosity.
--
-- "Private to the public is not invisible to the operator" — the rule was that
-- no per-vendor defect list is ever *published*. This is the outreach list, and
-- it prioritises itself: the domain with the most accumulated observations has
-- the most users in pain, is where a report lands hardest, and is therefore
-- both the best sales call and the most deserving of one.
CREATE MATERIALIZED VIEW outreach_rank AS
SELECT c.subject_host              AS host,
       count(*)                    AS clusters,
       sum(c.peak_epoch_reporters) AS reporters,
       sum(c.reports_total)        AS reports,
       max(c.last_epoch)           AS last_epoch
FROM   cluster c
WHERE  c.subject_kind = 'domain'
  AND  NOT EXISTS (
         SELECT 1 FROM anchor a
         JOIN attestation t ON t.anchor_id = a.id AND t.withdrawn_at IS NULL
         WHERE a.host = c.subject_host)
GROUP BY c.subject_host
HAVING max(c.peak_epoch_reporters) >= 5
WITH NO DATA;
CREATE UNIQUE INDEX outreach_rank_host ON outreach_rank (host);
CREATE INDEX outreach_rank_ranked ON outreach_rank (reporters DESC);

-- "A cluster with a solution whose reports say it worked for some and not
-- others is two problems." The outcome label exists precisely because the
-- report comes after the attempt, and this is where it points at the place a
-- distinction is missing.
CREATE VIEW mixed_outcome_signal AS
SELECT l.cluster_id, l.source_id, l.solution_id, o.epoch,
       count(*) FILTER (WHERE o.outcome = 'resolved')   AS worked,
       count(*) FILTER (WHERE o.outcome = 'unresolved') AS did_not,
       count(DISTINCT o.seen_key)                       AS reporters
FROM   observation o
JOIN   link l ON l.id = o.tried_link_id
GROUP BY l.cluster_id, l.source_id, l.solution_id, o.epoch
HAVING count(*) FILTER (WHERE o.outcome = 'resolved')   >= 1
   AND count(*) FILTER (WHERE o.outcome = 'unresolved') >= 1;

-- SV25. Exactly two integers, and there is no parameter that could narrow them
-- because there is no parameter. It shows the size of the gap without naming
-- anybody, which is the only version of this number that helps rather than
-- threatens.
CREATE VIEW ecosystem_totals AS
SELECT (SELECT count(DISTINCT subject_host) FROM cluster WHERE subject_kind = 'domain')
         AS products_observed,
       (SELECT count(*) FROM attestation WHERE withdrawn_at IS NULL)
         AS products_with_an_agent;

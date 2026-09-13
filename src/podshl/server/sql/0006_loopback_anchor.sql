-- The schema and the fetcher disagreed about loopback.
--
-- `anchor_url_is_https` refused anything that was not https, while
-- `ingest/fetch.py` already carved out `http://127.0.0.1` so the local
-- counterparty could be ingested at all. Two rules for the same question, and
-- the one that fired first won — which is how a carve-out becomes invisible.
--
-- The carve-out is narrow on purpose: loopback only, and loopback is not
-- reachable by anybody who is not already on the machine. Everything else stays
-- https, because a mirror served over a channel anyone can rewrite is not
-- provenance, whatever the commit says.

ALTER TABLE anchor DROP CONSTRAINT anchor_url_is_https;

ALTER TABLE anchor ADD CONSTRAINT anchor_url_is_https CHECK (
    kind <> 'url'
    OR value LIKE 'https://%'
    OR value LIKE 'http://127.0.0.1:%'
    OR value LIKE 'http://127.0.0.1/'
);

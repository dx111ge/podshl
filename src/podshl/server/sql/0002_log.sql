-- The transparency log: the only thing we ask anyone to trust.
--
-- Two reasons it is a log rather than a lookup API, and the second is the
-- important one. A per-request `GET /verify?domain=…` would tell us which
-- software every client runs — the profile this design refuses to let vendors
-- build, gathered from everyone at once. And a domain owner monitoring the log
-- sees any attestation claiming their domain, **including one we should not
-- have issued**. Certificate Transparency solved this; we are not going to
-- improve on it.

CREATE TYPE log_entry_kind AS ENUM (
    'attestation_issued',
    'attestation_withdrawn',
    'anchor_state_changed',
    'key_changed',
    'takedown',
    'tier_flag_changed',   -- billing's only output into the public side
    'log_policy'
);

CREATE TABLE log_entry (
    -- The RFC 6962 leaf index, not a sequence: a sequence leaves gaps on
    -- rollback, and a gapless index is what every proof is arithmetic over.
    seq         bigint PRIMARY KEY,
    kind        log_entry_kind NOT NULL,
    -- The canonical bytes that were hashed, stored verbatim. Re-serialising to
    -- answer a query would invalidate every proof ever issued over this leaf if
    -- it differed by one byte.
    data        bytea NOT NULL,
    leaf_hash   bytea NOT NULL CHECK (octet_length(leaf_hash) = 32),
    appended_at timestamptz NOT NULL DEFAULT now(),
    -- For a monitor's "show me everything about my anchor" filter. Never an
    -- input to a proof.
    anchor_id   bigint REFERENCES anchor(id)
);
CREATE UNIQUE INDEX log_entry_leaf ON log_entry (leaf_hash);
CREATE INDEX log_entry_anchor ON log_entry (anchor_id, seq);

-- Complete internal nodes only. Level 0 is leaves; node (L, i) covers leaves
-- [i*2^L, (i+1)*2^L) and is written once, when that range fills. The ragged
-- right edge is recomputed per request from these — O(log n) reads, never a
-- rehash of the tree.
CREATE TABLE log_node (
    level smallint NOT NULL,
    idx   bigint   NOT NULL,
    hash  bytea    NOT NULL CHECK (octet_length(hash) = 32),
    PRIMARY KEY (level, idx)
);

CREATE TABLE sth (
    tree_size    bigint PRIMARY KEY,
    root_hash    bytea  NOT NULL CHECK (octet_length(root_hash) = 32),
    timestamp_ms bigint NOT NULL,
    signature    jsonb  NOT NULL,
    key_id       text   NOT NULL,
    issued_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sth_latest ON sth (tree_size DESC);

-- Notice and action. Two duties survive any risk appetite: a route for a
-- notice, and a **statement of reasons to the affected party**. The second is
-- already built — it is the same log entry — and that is why removing quietly
-- is impossible here rather than merely discouraged.
CREATE TABLE notice (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    anchor_id    bigint REFERENCES anchor(id),
    received_at  timestamptz NOT NULL DEFAULT now(),
    -- A closed vocabulary, so weaponised claims are visible and countable.
    reason_code  text NOT NULL,
    -- The only personal data on the public side, and it is never served.
    notifier     jsonb NOT NULL,
    acted_at     timestamptz,
    action       text CHECK (action IN ('degraded', 'refused')),
    -- SV39: a notice aimed at a problem class rather than an anchor is refused,
    -- and the refusal is recorded — so "no such path exists" is demonstrable
    -- rather than asserted. A vendor must not be able to forbid anyone from
    -- saying their name out loud.
    refused_reason text,
    log_seq      bigint
);
CREATE INDEX notice_by_anchor ON notice (anchor_id, received_at DESC);

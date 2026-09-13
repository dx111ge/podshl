-- Clusters, observations, and the decision tree.
--
-- The k-threshold counts **distinct pseudonyms**, and that is the whole
-- difference between this schema and the one it replaces. Counting submissions
-- cannot tell five people from one person reporting five times, which is
-- exactly the distinction a threshold protecting a rare configuration depends
-- on. Here it is a unique index, so it holds whatever the application does.

CREATE TYPE cluster_subject AS ENUM ('source', 'domain');

CREATE TABLE epoch (
    epoch        int PRIMARY KEY,          -- YYYYMM
    opened_at    timestamptz NOT NULL DEFAULT now(),
    closed_at    timestamptz,
    -- Deliberately NOT the salt. A column would be in every base backup and
    -- every WAL archive, so "discarded when the epoch rolls" would be true of
    -- the live row and false of the archive — the promise honest only until the
    -- first restore. The salt is a file; this records which one, so a restarted
    -- process can prove it loaded the right one.
    salt_id      text NOT NULL,
    salt_sha256  bytea NOT NULL CHECK (octet_length(salt_sha256) = 32),
    destroyed_at timestamptz,
    CONSTRAINT a_closed_epoch_has_no_live_salt
        CHECK (closed_at IS NULL OR destroyed_at IS NOT NULL)
);

CREATE TABLE cluster (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    subject_kind  cluster_subject NOT NULL,
    -- Owned: an attested source, which may have a tree, so switches exist.
    -- Unowned: a bare host, exact signature matches only. Similarity is curated
    -- capital, and curation needs an owner.
    source_id     bigint REFERENCES source(id),
    subject_host  text,
    -- Derived from the signature, never supplied as free text. An earlier draft
    -- took a truncated problem sentence as the class, which breaks the rule the
    -- rest of the design obeys.
    problem_class text NOT NULL,

    -- The sorted field names the signature carries. Two reports are comparable
    -- only if their shapes match; a shape change is a new cluster, not a merge.
    signature_shape text[] NOT NULL,
    -- sha256 over the canonical signature. This is exactly the path parameter
    -- of GET /cluster/<hash>, and the reason that request can be answered at
    -- the edge without ever reaching us.
    signature_hash  bytea NOT NULL CHECK (octet_length(signature_hash) = 32),
    signature       jsonb NOT NULL,

    first_epoch   int NOT NULL,
    last_epoch    int NOT NULL,

    -- Three counters, three different and honest meanings.
    --   reports_total          submissions. "47 reported", never "47 occurred".
    --   reporters_this_epoch   distinct pseudonyms in the open epoch. Exact.
    --   peak_epoch_reporters   the largest such count over all epochs — a true
    --                          LOWER BOUND on distinct people, and therefore the
    --                          only one the k-threshold is ever applied to.
    reports_total        int NOT NULL DEFAULT 0,
    reporters_this_epoch int NOT NULL DEFAULT 0,
    peak_epoch_reporters int NOT NULL DEFAULT 0,
    created_at    timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT cluster_subject_is_one_thing CHECK (
        (subject_kind = 'source' AND source_id IS NOT NULL AND subject_host IS NULL) OR
        (subject_kind = 'domain' AND source_id IS NULL     AND subject_host IS NOT NULL))
);
-- The edge path: one unique-index probe, one heap fetch, no external call.
CREATE UNIQUE INDEX cluster_by_hash ON cluster (signature_hash);
CREATE INDEX cluster_owned_ranked ON cluster (source_id, peak_epoch_reporters DESC)
    WHERE subject_kind = 'source';
CREATE INDEX cluster_host_ranked ON cluster (subject_host, peak_epoch_reporters DESC)
    WHERE subject_kind = 'domain';

CREATE TABLE tree (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_id     bigint NOT NULL REFERENCES source(id) ON DELETE CASCADE,
    problem_class text NOT NULL,
    commit        text,
    valid_from    timestamptz NOT NULL DEFAULT now(),
    valid_to      timestamptz
);
CREATE UNIQUE INDEX tree_current ON tree (source_id, problem_class) WHERE valid_to IS NULL;

CREATE TABLE tree_node (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tree_id     bigint NOT NULL REFERENCES tree(id) ON DELETE CASCADE,
    parent_id   bigint REFERENCES tree_node(id) ON DELETE CASCADE,
    depth       int NOT NULL,

    -- The predicate on the PARENT's switch that selects this node. One closed
    -- set of total orders and equalities: no regex, no distance, no threshold.
    -- A wrong guess here reaches a user.
    match_op    text CHECK (match_op IN ('eq', 'lt', 'le', 'gt', 'ge', 'in')),
    match_value jsonb,
    edge_label  text,                    -- shown to the developer, never matched on

    -- The switch THIS node applies to its children. NULL at a leaf.
    switch_fact text,
    -- 'reading'  — the value is already in every stored signature, so adding
    --              this switch re-partitions history.
    -- 'question' — no historic value exists; it applies forward only.
    -- The developer must declare which before it can be stored, because the
    -- authoring tool has to show that difference or the expensive option gets
    -- picked by accident.
    switch_kind text CHECK (switch_kind IN ('reading', 'question')),
    comparator  text CHECK (comparator IN ('string', 'number', 'version')),
    probe       jsonb,                   -- the Probe delivered as `need`

    -- A node may carry BOTH a switch and an answer. That answer is what its
    -- children's "don't know" branch lands on — computed on the way down, never
    -- looked for on the way back up, which is what makes a dead end structurally
    -- impossible rather than merely unlikely.
    solution_id text,

    CONSTRAINT switch_is_complete CHECK (
        (switch_fact IS NULL AND switch_kind IS NULL AND comparator IS NULL) OR
        (switch_fact IS NOT NULL AND switch_kind IS NOT NULL AND comparator IS NOT NULL)),
    CONSTRAINT a_question_declares_its_probe CHECK (
        switch_kind IS DISTINCT FROM 'question' OR probe IS NOT NULL),
    CONSTRAINT the_root_has_no_edge CHECK (
        (parent_id IS NULL AND match_op IS NULL) OR
        (parent_id IS NOT NULL AND match_op IS NOT NULL))
);
CREATE INDEX tree_node_children ON tree_node (parent_id);
CREATE INDEX tree_node_by_tree  ON tree_node (tree_id, depth);

CREATE TABLE link (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    cluster_id  bigint NOT NULL REFERENCES cluster(id) ON DELETE CASCADE,
    source_id   bigint NOT NULL REFERENCES source(id),
    solution_id text   NOT NULL,
    node_id     bigint REFERENCES tree_node(id) ON DELETE SET NULL,
    linked_at   timestamptz NOT NULL DEFAULT now(),
    unlinked_at timestamptz,
    UNIQUE (cluster_id, source_id, solution_id, node_id)
);

CREATE TABLE observation (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    cluster_id    bigint NOT NULL REFERENCES cluster(id) ON DELETE CASCADE,
    epoch         int    NOT NULL REFERENCES epoch(epoch),
    model_class   text   NOT NULL,
    ux_severity   text,
    outcome       text CHECK (outcome IN ('resolved', 'unresolved', 'escalated', 'abstained')),
    -- Which solution was tried. Without this join, "worked for some and not
    -- others" is not computable — and that sentence is the whole reason the
    -- report comes after the attempt rather than before it.
    tried_link_id bigint REFERENCES link(id),
    -- The generalised values themselves, so a switch added later can read
    -- history. A bag of counters could not.
    observed      jsonb NOT NULL,
    -- HMAC(HKDF(epoch_salt, cluster_id), pseudonym), 16 bytes. Unrelated across
    -- clusters and across epochs, so nothing joins on either axis, and
    -- untestable against any pseudonym once the salt file is gone.
    seen_key      bytea NOT NULL CHECK (octet_length(seen_key) = 16),
    -- No timestamp. The epoch is the entire time resolution this design allows.

    -- Only ever present with its own consent, naming the destination.
    description         text,
    description_consent jsonb,
    CONSTRAINT free_text_carries_its_own_consent CHECK (
        description IS NULL OR (
            description_consent ? 'destination' AND
            description_consent ? 'granted_at' AND
            (description_consent ->> 'granted')::boolean))
);
-- This index IS the k-threshold's honesty. A second submission from the same
-- pseudonym in the same epoch cannot become a second row, so no amount of
-- application-level carelessness can turn one person into five.
CREATE UNIQUE INDEX observation_once ON observation (cluster_id, epoch, seen_key);
CREATE INDEX observation_by_cluster ON observation (cluster_id, epoch);
CREATE INDEX observation_repartition ON observation USING gin (observed jsonb_path_ops);
CREATE INDEX observation_by_link ON observation (tried_link_id) WHERE tried_link_id IS NOT NULL;

-- A switch does not MOVE history; it reads it. `seen_key` is salted per
-- cluster, so reattaching a row to another cluster would need an HMAC over a
-- pseudonym we deliberately do not have — and moving rows would either break
-- the uniqueness above or force us to keep the pseudonym, which is the one
-- thing the whole scheme exists to avoid. A partition is therefore a derived
-- grouping over the original cluster.
CREATE TABLE cluster_partition (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    cluster_id  bigint NOT NULL REFERENCES cluster(id) ON DELETE CASCADE,
    node_id     bigint NOT NULL REFERENCES tree_node(id) ON DELETE CASCADE,
    fact        text  NOT NULL,
    match_op    text  NOT NULL,
    match_value jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (cluster_id, node_id)
);

-- Abuse protection for the query path. Its own table, holding a count and
-- nothing else: no cluster reference, no timestamp beyond the epoch. Otherwise
-- "a query touches no store at all" becomes false in exactly the way that
-- matters — we would learn what was asked.
CREATE TABLE query_budget (
    pseudonym text NOT NULL,
    epoch     int  NOT NULL,
    queries   int  NOT NULL DEFAULT 0,
    PRIMARY KEY (pseudonym, epoch)
);

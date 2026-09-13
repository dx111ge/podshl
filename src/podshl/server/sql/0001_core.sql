-- Anchors, attestations, and the mirror.
--
-- Two things SERVER.md's sketch conflates are split here, because they are
-- different state machines: `anchor.status` is *control liveness only* — does
-- the claimant still control the location — and the attested/unknown/revoked
-- triple is derived from the live attestation row. A repository can be actively
-- maintained while a check fails transiently, and an anchor can verify
-- perfectly for a project nobody has touched since 2021. One is a security
-- property, the other is an age.

CREATE TYPE anchor_kind    AS ENUM ('url', 'dns');
CREATE TYPE control_status AS ENUM ('live', 'stale', 'unknown');
CREATE TYPE tier           AS ENUM ('oss', 'enterprise');

-- The distinction every resolver in this project used to destroy by returning
-- NULL for all of it. The 14/90-day grading is arithmetic over exactly this
-- column: a claimant who stopped serving a file has said something; a network
-- that would not carry our question has not.
CREATE TYPE probe_reason AS ENUM (
    'confirmed',        -- asked, answered, matched
    'contradicted',     -- asked, answered, wrong token
    'absent',           -- asked, answered "no such thing" (404 / NXDOMAIN / NODATA)
    'malformed',        -- asked, answered, unparseable
    'redirected_away',  -- asked, told to go to another host; we did not follow
    'unreachable',      -- not asked: timeout, TCP reset, TLS failure, SERVFAIL
    'refused',          -- not asked: 403, 429, 451, DNS REFUSED
    'internal'          -- not asked: our bug, our outage. Never counts against them.
);

CREATE TABLE anchor (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind            anchor_kind NOT NULL,
    value           text NOT NULL,
    -- The comparison key, always punycode. Every check — endpoint-under-anchor,
    -- confusable skeleton, outreach join — uses this and never `value`, so a
    -- unicode host can never be compared as though it were ASCII.
    host            text NOT NULL,
    host_unicode    text,          -- display only, and never shown without `host`
    challenge_token text NOT NULL,

    status          control_status NOT NULL DEFAULT 'unknown',
    verified_at     timestamptz,   -- first confirmation ever
    last_checked    timestamptz,   -- last time we asked, whatever the answer
    last_confirmed  timestamptz,   -- last time the answer was 'confirmed'
    -- Advances ONLY on a probe that is evidence. NULL while every failure so
    -- far was our own inability to ask.
    failing_since   timestamptz,

    -- Set at ingest for a host confusable with a well-known mark. Not attested,
    -- and NOT blocked: exactly the state of a project that never registered.
    attest_hold     text,
    created_at      timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT anchor_host_is_punycode
        CHECK (host ~ '^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$'),
    CONSTRAINT anchor_url_is_https
        CHECK (kind <> 'url' OR value LIKE 'https://%'),
    UNIQUE (kind, value)
);
CREATE INDEX anchor_by_host ON anchor (host);
-- The sweep's only question: who has been failing long enough to change state?
-- Partial, so it holds a few dozen rows rather than every anchor.
CREATE INDEX anchor_failing ON anchor (failing_since)
    WHERE failing_since IS NOT NULL AND status <> 'unknown';

CREATE TABLE sweep_run (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    attempted  int NOT NULL DEFAULT 0,
    silent     int NOT NULL DEFAULT 0,
    -- A run in which a fifth of the internet was "unreachable" was our outage,
    -- not theirs, and its silence is excluded from every judgement.
    degraded   boolean NOT NULL DEFAULT false
);

CREATE TABLE anchor_probe (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    anchor_id   bigint NOT NULL REFERENCES anchor(id) ON DELETE CASCADE,
    at          timestamptz NOT NULL DEFAULT now(),
    reason      probe_reason NOT NULL,
    -- Denormalised from `reason` so the classification rule lives in one place
    -- both Python and SQL can read.
    is_evidence boolean NOT NULL,
    -- HTTP status, DNS rcode, the URL actually fetched, the token compared:
    -- enough to argue about a state change months later.
    detail      jsonb NOT NULL DEFAULT '{}'::jsonb,
    run_id      bigint REFERENCES sweep_run(id)
);
CREATE INDEX anchor_probe_recent ON anchor_probe (anchor_id, at DESC);

CREATE TABLE attestation (
    id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    anchor_id      bigint NOT NULL REFERENCES anchor(id),
    tier           tier   NOT NULL,

    -- SV4: there is deliberately no display-name column. For OSS the name IS
    -- anchor.value, rendered from the anchor row. For enterprise it is copied
    -- verbatim from a register file whose name is recorded beside it. There is
    -- nowhere to type "NVIDIA".
    lei            char(20),
    legal_name     text,
    lei_file       text,
    lei_checked    timestamptz,

    key_jwk        jsonb NOT NULL,
    -- RFC 7638 thumbprint. Key continuity is the tripwire, so the thing the
    -- tripwire compares gets its own indexed column.
    key_thumbprint bytea NOT NULL CHECK (octet_length(key_thumbprint) = 32),
    issued_at      timestamptz NOT NULL DEFAULT now(),

    -- Withdrawal is two different acts, and the schema refuses to let the
    -- second be spelled as the first. 'revoked' is an accusation — compromise
    -- or abuse, and nothing else. 'degraded' is a takedown or a 90-day lapse,
    -- and means exactly `unknown`: the state of a project that never
    -- registered. Someone who stopped working on something has done nothing
    -- wrong.
    withdrawn_at     timestamptz,
    withdrawn_kind   text CHECK (withdrawn_kind IN ('revoked', 'degraded')),
    withdrawn_reason text,

    issued_seq     bigint NOT NULL,
    withdrawn_seq  bigint,

    CONSTRAINT enterprise_identity_comes_from_the_register CHECK (
        (tier = 'oss'        AND lei IS NULL     AND legal_name IS NULL) OR
        (tier = 'enterprise' AND lei IS NOT NULL AND legal_name IS NOT NULL)),
    CONSTRAINT withdrawal_states_a_reason CHECK (
        (withdrawn_at IS NULL AND withdrawn_kind IS NULL AND withdrawn_reason IS NULL
         AND withdrawn_seq IS NULL)
     OR (withdrawn_at IS NOT NULL AND withdrawn_kind IS NOT NULL
         AND withdrawn_reason IS NOT NULL AND withdrawn_seq IS NOT NULL))
);
-- One live attestation per anchor. A key rotation withdraws and re-issues, so
-- continuity is a readable sequence of rows rather than an UPDATE that erases
-- its own evidence.
CREATE UNIQUE INDEX attestation_one_live ON attestation (anchor_id)
    WHERE withdrawn_at IS NULL;
CREATE INDEX attestation_by_thumbprint ON attestation (key_thumbprint);
CREATE INDEX attestation_history ON attestation (anchor_id, issued_at DESC);

CREATE TABLE source (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    anchor_id     bigint NOT NULL REFERENCES anchor(id),
    manifest_url  text NOT NULL,
    -- Nothing outside this prefix is ever fetched. Recorded rather than
    -- re-derived, so "fetch the recorded URL or nothing" is a comparison.
    fetch_prefix  text NOT NULL,

    last_commit   text,            -- as declared by the manifest, republished as such
    etag          text,
    last_modified text,
    content_hash  bytea,
    last_fetched  timestamptz,
    last_changed  timestamptz,
    -- The ingest queue is this column and the partial index below it.
    next_fetch_at timestamptz NOT NULL DEFAULT now(),
    consecutive_silence int NOT NULL DEFAULT 0,

    -- The three deprecation signals, in SERVER.md's order and never merged.
    -- The developer's own word is authoritative; the forge's archive flag is an
    -- explicit act by the owner; commit age is an observation and stays one.
    declared_status      text CHECK (declared_status IN ('active', 'deprecated')),
    successor_url        text,
    forge_archived       boolean,        -- NULL = we did not ask / cannot tell
    forge_last_commit_at timestamptz,

    mirror_state  text NOT NULL DEFAULT 'serving'
                  CHECK (mirror_state IN ('serving', 'withheld')),
    UNIQUE (anchor_id, manifest_url)
);
-- The conditional-GET pass reads only the head of this index; ten thousand
-- rows never get scanned, and withheld sources are not in it at all.
CREATE INDEX source_due ON source (next_fetch_at) WHERE mirror_state = 'serving';

CREATE TABLE card (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_id    bigint NOT NULL REFERENCES source(id) ON DELETE CASCADE,
    json         jsonb  NOT NULL,
    langs        text[] NOT NULL,
    endpoint_url text,              -- NULL for enterprise: we are not in the path
    commit       text,
    content_hash bytea NOT NULL,
    signature    jsonb,
    log_seq      bigint,
    valid_from   timestamptz NOT NULL DEFAULT now(),
    valid_to     timestamptz,
    -- SV7 as a constraint rather than only as a validator, so a bug in the
    -- validator still cannot store a card without English.
    CONSTRAINT english_is_the_one_obligation CHECK ('en' = ANY (langs))
);
CREATE UNIQUE INDEX card_current ON card (source_id) WHERE valid_to IS NULL;

CREATE TABLE solution (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_id    bigint NOT NULL REFERENCES source(id) ON DELETE CASCADE,
    solution_id  text   NOT NULL,
    answers      jsonb  NOT NULL,
    proposes     jsonb  NOT NULL,
    text_by_lang jsonb  NOT NULL,
    severity     text CHECK (severity IN ('info', 'low', 'medium', 'high')),
    path         text   NOT NULL,
    commit       text,
    content_hash bytea  NOT NULL,
    valid_from   timestamptz NOT NULL DEFAULT now(),
    valid_to     timestamptz,
    CONSTRAINT solution_has_english
        CHECK (text_by_lang ? 'en' AND length(text_by_lang ->> 'en') > 0),
    CONSTRAINT proposes_is_a_list CHECK (jsonb_typeof(proposes) = 'array')
);
CREATE UNIQUE INDEX solution_current ON solution (source_id, solution_id)
    WHERE valid_to IS NULL;
CREATE INDEX solution_by_class ON solution ((answers ->> 'problem_class'))
    WHERE valid_to IS NULL;

CREATE TABLE lei_record (
    lei           char(20) PRIMARY KEY,
    legal_name    text NOT NULL,
    entity_status text NOT NULL,     -- ACTIVE | INACTIVE
    reg_status    text NOT NULL,     -- ISSUED | LAPSED | RETIRED | ANNULLED
    country       char(2),
    -- The published file this came from. An attestation names its register
    -- source the way the mirror names its commit.
    file_id       text NOT NULL,
    loaded_at     timestamptz NOT NULL DEFAULT now()
);

-- Compared against a curated list, NOT against every other anchor. The latter
-- makes attestation order-dependent and lets a squatter who registered first
-- block a legitimate anchor — which would be the trademark-register role this
-- project refuses to take.
CREATE TABLE well_known_mark (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    label      text NOT NULL,
    skeleton   text NOT NULL,        -- UTS 39 skeleton of `label`
    owner_host text,
    added_by   text NOT NULL,
    added_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX well_known_mark_skeleton ON well_known_mark (skeleton);

-- The anchor challenge IS the account. No password, no email, no session: each
-- of those would be a credential we then hold, and holding none is the point.
CREATE TABLE dashboard_claim (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    anchor_id  bigint NOT NULL REFERENCES anchor(id),
    token_hash bytea NOT NULL,
    issued_at  timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz
);
CREATE UNIQUE INDEX dashboard_claim_token ON dashboard_claim (token_hash);
CREATE INDEX dashboard_claim_live ON dashboard_claim (anchor_id) WHERE revoked_at IS NULL;

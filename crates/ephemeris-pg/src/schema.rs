/// SQL to initialize the Ephemeris schema.
/// Requires the ltree and pgcrypto extensions.
pub const INIT_SCHEMA: &str = r#"
CREATE EXTENSION IF NOT EXISTS ltree;
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- EPCIS Event Ledger (Write model)
CREATE TABLE IF NOT EXISTS epcis_events (
    id          UUID PRIMARY KEY,
    event_type  TEXT NOT NULL,
    event_time  TIMESTAMPTZ NOT NULL,
    event_data  JSONB NOT NULL,
    record_time TIMESTAMPTZ NOT NULL DEFAULT now(),
    event_hash  TEXT UNIQUE  -- For idempotent writes (hash of event content)
);

CREATE INDEX IF NOT EXISTS idx_events_time ON epcis_events (event_time);
CREATE INDEX IF NOT EXISTS idx_events_type ON epcis_events (event_type);
CREATE INDEX IF NOT EXISTS idx_events_data ON epcis_events USING GIN (event_data jsonb_path_ops);

-- Aggregation Hierarchy (Read model)
CREATE TABLE IF NOT EXISTS aggregation (
    id          SERIAL PRIMARY KEY,
    child_epc   TEXT NOT NULL UNIQUE,
    parent_epc  TEXT NOT NULL,
    path        ltree NOT NULL,
    event_id    UUID NOT NULL REFERENCES epcis_events(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_agg_path_gist ON aggregation USING GIST (path);
CREATE INDEX IF NOT EXISTS idx_agg_path_btree ON aggregation USING BTREE (path);
CREATE INDEX IF NOT EXISTS idx_agg_parent ON aggregation (parent_epc);
CREATE INDEX IF NOT EXISTS idx_agg_child ON aggregation (child_epc);

-- Serial Number State Tracking (OPEN-SCS lifecycle)
CREATE TABLE IF NOT EXISTS serial_numbers (
    epc         TEXT PRIMARY KEY,
    state       TEXT NOT NULL,
    sid_class   TEXT,
    pool_id     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sn_state ON serial_numbers (state);
CREATE INDEX IF NOT EXISTS idx_sn_sid_class ON serial_numbers (sid_class) WHERE sid_class IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sn_pool ON serial_numbers (pool_id) WHERE pool_id IS NOT NULL;

-- Serial Number Transition Audit Log
CREATE TABLE IF NOT EXISTS sn_transitions (
    id          SERIAL PRIMARY KEY,
    epc         TEXT NOT NULL,
    from_state  TEXT NOT NULL,
    to_state    TEXT NOT NULL,
    biz_step    TEXT NOT NULL,
    event_id    UUID REFERENCES epcis_events(id),
    source      TEXT NOT NULL,
    timestamp   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_snt_epc ON sn_transitions (epc, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_snt_event ON sn_transitions (event_id) WHERE event_id IS NOT NULL;

-- Serial Number Pools (OPEN-SCS pool management)
CREATE TABLE IF NOT EXISTS sn_pools (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    sid_class   TEXT,
    esm_endpoint TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_pools_sid_class ON sn_pools(sid_class) WHERE sid_class IS NOT NULL;

-- Pool selection criteria (OPEN-SCS §6.9)
CREATE TABLE IF NOT EXISTS pool_criteria (
    pool_id     UUID NOT NULL REFERENCES sn_pools(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (pool_id, key, value)
);

CREATE INDEX IF NOT EXISTS idx_pool_criteria_key ON pool_criteria(key, value);
"#;

/// SQL to initialize the Ephemeris schema.
/// Requires the ltree extension.
pub const INIT_SCHEMA: &str = r#"
CREATE EXTENSION IF NOT EXISTS ltree;

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
"#;

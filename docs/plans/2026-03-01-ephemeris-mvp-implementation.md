# Ephemeris MVP Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Prove the repository trait abstraction holds across PostgreSQL and ArangoDB, with a working MQTT→DB→API pipeline.

**Architecture:** Cargo workspace with `ephemeris-core` (traits + domain types), connector crates (`ephemeris-pg`, `ephemeris-arango`), MQTT ingestion, Axum REST API, and a unified app binary that wires backends at startup via config. TDD throughout.

**Tech Stack:** Rust (stable), tokio 1.49, tokio-postgres 0.7, deadpool-postgres 0.14, rumqttc 0.25, axum 0.8, reqwest 0.13, serde 1, clap 4.5, config 0.15, testcontainers 0.27, mockall 0.14, cargo-deny 0.19, chrono 0.4, thiserror 2, uuid 1.18

**Key Decision — Async Traits:** Use native `async fn` in traits (Rust 1.75+). Use `trait_variant` crate to generate `Send` variants for Tokio. Use `mockall` with `#[automock]` for test mocks. No `async-trait` crate needed since we use static dispatch (generics), not `dyn Trait`.

---

## Phase 1: Foundation

### Task 1: Scaffold Cargo Workspace

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/ephemeris-core/Cargo.toml`
- Create: `crates/ephemeris-core/src/lib.rs`
- Create: `.gitignore`
- Create: `deny.toml`
- Create: `rust-toolchain.toml`

**Step 1: Create workspace root Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/ephemeris-core",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"
repository = "https://github.com/saturnis-io/ephemeris"

[workspace.dependencies]
# Core
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = { version = "1.18", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
trait-variant = "0.1"

# Async
tokio = { version = "1.49", features = ["full"] }

# Database - Tier 1
tokio-postgres = { version = "0.7", features = ["with-serde_json-1", "with-uuid-1", "with-chrono-0_4"] }
deadpool-postgres = { version = "0.14", features = ["rt_tokio_1"] }

# Database - Tier 2 (Enterprise)
reqwest = { version = "0.13", features = ["json"] }

# MQTT
rumqttc = "0.25"

# API
axum = "0.8"
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }

# Config
config = { version = "0.15", features = ["toml"] }
clap = { version = "4.5", features = ["derive"] }
toml = "0.8"

# Testing
mockall = "0.14"
testcontainers = "0.27"
testcontainers-modules = { version = "0.15", features = ["postgres"] }
```

**Step 2: Create ephemeris-core/Cargo.toml**

```toml
[package]
name = "ephemeris-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
trait-variant = { workspace = true }

[dev-dependencies]
mockall = { workspace = true }
tokio = { workspace = true }
```

**Step 3: Create ephemeris-core/src/lib.rs**

```rust
pub mod domain;
pub mod error;
pub mod repository;
```

**Step 4: Create .gitignore**

```
/target
*.swp
*.swo
.env
*.log
```

**Step 5: Create deny.toml**

```toml
[graph]
targets = []

[advisories]
ignore = []

[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Zlib",
]
confidence-threshold = 0.93

[bans]
multiple-versions = "warn"
deny = []

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

**Step 6: Create rust-toolchain.toml**

```toml
[toolchain]
channel = "stable"
```

**Step 7: Verify workspace compiles**

Run: `cargo check`
Expected: Compiles with no errors.

**Step 8: Verify license check passes**

Run: `cargo install cargo-deny && cargo deny check licenses`
Expected: All licenses allowed.

**Step 9: Commit**

```bash
git add -A
git commit -m "feat: scaffold Cargo workspace with ephemeris-core crate"
```

---

### Task 2: Core Domain Types

**Files:**
- Create: `crates/ephemeris-core/src/domain/mod.rs`
- Create: `crates/ephemeris-core/src/domain/epc.rs`
- Create: `crates/ephemeris-core/src/domain/event.rs`
- Create: `crates/ephemeris-core/src/domain/aggregation.rs`
- Create: `crates/ephemeris-core/src/domain/query.rs`

**Step 1: Write unit tests for Epc parsing**

In `crates/ephemeris-core/src/domain/epc.rs`:

```rust
/// Electronic Product Code — a URI identifier for a physical object or class.
/// Supports both URN format (urn:epc:id:sgtin:...) and GS1 Digital Link (https://id.gs1.org/...).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Epc(String);

impl Epc {
    pub fn new(uri: impl Into<String>) -> Self {
        Self(uri.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Epc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epc_from_urn() {
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        assert_eq!(epc.as_str(), "urn:epc:id:sgtin:0614141.107346.2017");
    }

    #[test]
    fn test_epc_from_digital_link() {
        let epc = Epc::new("https://id.gs1.org/01/09521568251204/21/10");
        assert_eq!(epc.as_str(), "https://id.gs1.org/01/09521568251204/21/10");
    }

    #[test]
    fn test_epc_equality() {
        let a = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        let b = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");
        assert_eq!(a, b);
    }
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test -p ephemeris-core`

**Step 3: Write EpcisEvent types**

In `crates/ephemeris-core/src/domain/event.rs`, define the EPCIS 2.0 event types following the GS1 spec field names:

```rust
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use super::epc::Epc;

/// Unique identifier for a stored event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// EPCIS 2.0 action types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Action {
    Observe,
    Add,
    Delete,
}

/// Quantity with optional unit of measure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantityElement {
    pub epc_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uom: Option<String>,
}

/// Business transaction reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BizTransaction {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub tx_type: Option<String>,
    pub biz_transaction: String,
}

/// Location reference (readPoint or bizLocation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationRef {
    pub id: String,
}

/// Source or destination party/location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDest {
    #[serde(rename = "type")]
    pub sd_type: String,
    #[serde(alias = "source", alias = "destination")]
    pub identifier: String,
}

/// The top-level EPCIS event enum.
/// Each variant holds the event-type-specific fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EpcisEvent {
    ObjectEvent(ObjectEventData),
    AggregationEvent(AggregationEventData),
    TransformationEvent(TransformationEventData),
}

/// Common fields shared by all event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonEventFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub event_time: DateTime<FixedOffset>,
    pub event_time_zone_offset: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_time: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biz_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_point: Option<LocationRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biz_location: Option<LocationRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub biz_transaction_list: Vec<BizTransaction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_list: Vec<SourceDest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_list: Vec<SourceDest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEventData {
    #[serde(flatten)]
    pub common: CommonEventFields,
    pub action: Action,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub epc_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quantity_list: Vec<QuantityElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationEventData {
    #[serde(flatten)]
    pub common: CommonEventFields,
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, rename = "childEPCs", skip_serializing_if = "Vec::is_empty")]
    pub child_epcs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_quantity_list: Vec<QuantityElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformationEventData {
    #[serde(flatten)]
    pub common: CommonEventFields,
    #[serde(default, rename = "inputEPCList", skip_serializing_if = "Vec::is_empty")]
    pub input_epc_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_quantity_list: Vec<QuantityElement>,
    #[serde(default, rename = "outputEPCList", skip_serializing_if = "Vec::is_empty")]
    pub output_epc_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_quantity_list: Vec<QuantityElement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transformation_id: Option<String>,
}
```

**Step 4: Write tests for event serialization roundtrip**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_event_roundtrip() {
        let json = r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2005-04-03T20:33:31.116-06:00",
            "eventTimeZoneOffset": "-06:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"],
            "bizStep": "shipping",
            "readPoint": {"id": "urn:epc:id:sgln:0614141.07346.1234"}
        }"#;

        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        match &event {
            EpcisEvent::ObjectEvent(data) => {
                assert_eq!(data.action, Action::Observe);
                assert_eq!(data.epc_list.len(), 1);
                assert_eq!(data.common.biz_step.as_deref(), Some("shipping"));
            }
            _ => panic!("Expected ObjectEvent"),
        }

        // Roundtrip
        let serialized = serde_json::to_string(&event).unwrap();
        let _: EpcisEvent = serde_json::from_str(&serialized).unwrap();
    }

    #[test]
    fn test_aggregation_event_roundtrip() {
        let json = r#"{
            "type": "AggregationEvent",
            "action": "ADD",
            "eventTime": "2013-06-08T14:58:56.591+02:00",
            "eventTimeZoneOffset": "+02:00",
            "parentID": "urn:epc:id:sscc:0614141.1234567890",
            "childEPCs": [
                "urn:epc:id:sgtin:0614141.107346.2017",
                "urn:epc:id:sgtin:0614141.107346.2018"
            ],
            "bizStep": "packing"
        }"#;

        let event: EpcisEvent = serde_json::from_str(json).unwrap();
        match &event {
            EpcisEvent::AggregationEvent(data) => {
                assert_eq!(data.action, Action::Add);
                assert_eq!(data.parent_id.as_deref(), Some("urn:epc:id:sscc:0614141.1234567890"));
                assert_eq!(data.child_epcs.len(), 2);
            }
            _ => panic!("Expected AggregationEvent"),
        }
    }
}
```

**Step 5: Run tests**

Run: `cargo test -p ephemeris-core`
Expected: All pass.

**Step 6: Write aggregation types**

In `crates/ephemeris-core/src/domain/aggregation.rs`:

```rust
use super::epc::Epc;
use serde::{Deserialize, Serialize};

/// A node in the aggregation tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationNode {
    pub epc: Epc,
    pub children: Vec<AggregationNode>,
}

/// A flat representation of the full hierarchy from a root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationTree {
    pub root: Epc,
    pub nodes: Vec<AggregationNode>,
}
```

**Step 7: Write query types**

In `crates/ephemeris-core/src/domain/query.rs`:

```rust
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

/// Query parameters for filtering events (subset of EPCIS 2.0 Query Interface).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ge_event_time: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lt_event_time: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eq_biz_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_epc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_parent_id: Option<String>,
    /// Max results to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_page: Option<u32>,
    /// Pagination token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}
```

**Step 8: Wire up domain/mod.rs**

```rust
pub mod aggregation;
pub mod epc;
pub mod event;
pub mod query;

pub use aggregation::*;
pub use epc::*;
pub use event::*;
pub use query::*;
```

**Step 9: Run all tests, commit**

Run: `cargo test -p ephemeris-core`
Expected: All pass.

```bash
git add crates/ephemeris-core/src/domain/
git commit -m "feat(core): add EPCIS 2.0 domain types with serde roundtrip tests"
```

---

### Task 3: Core Error Types

**Files:**
- Create: `crates/ephemeris-core/src/error.rs`

**Step 1: Define core error types**

```rust
use thiserror::Error;

/// Errors that can occur in repository operations.
#[derive(Error, Debug)]
pub enum RepoError {
    #[error("event not found: {0}")]
    NotFound(String),

    #[error("duplicate event: {0}")]
    Duplicate(String),

    #[error("connection error: {0}")]
    Connection(String),

    #[error("query error: {0}")]
    Query(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("internal error: {0}")]
    Internal(String),
}
```

**Step 2: Commit**

```bash
git add crates/ephemeris-core/src/error.rs
git commit -m "feat(core): add repository error types"
```

---

### Task 4: Core Repository Traits

**Files:**
- Create: `crates/ephemeris-core/src/repository/mod.rs`
- Create: `crates/ephemeris-core/src/repository/event.rs`
- Create: `crates/ephemeris-core/src/repository/aggregation.rs`

**Step 1: Define EventRepository trait**

In `crates/ephemeris-core/src/repository/event.rs`:

```rust
use crate::domain::{EpcisEvent, EventId, EventQuery};
use crate::error::RepoError;

/// Repository for storing and querying EPCIS events.
///
/// Implementations must be idempotent on store_event — duplicate
/// event IDs should be silently ignored (return existing EventId).
#[trait_variant::make(Send)]
pub trait EventRepository: Sync {
    /// Store an EPCIS event. Returns the assigned EventId.
    /// Idempotent: duplicate event_id returns Ok with existing id.
    async fn store_event(&self, event: &EpcisEvent) -> Result<EventId, RepoError>;

    /// Retrieve a single event by its ID.
    async fn get_event(&self, id: &EventId) -> Result<Option<EpcisEvent>, RepoError>;

    /// Query events with filters. Returns matching events.
    async fn query_events(&self, query: &EventQuery) -> Result<Vec<EpcisEvent>, RepoError>;
}
```

**Step 2: Define AggregationRepository trait**

In `crates/ephemeris-core/src/repository/aggregation.rs`:

```rust
use crate::domain::{AggregationTree, Epc, EventId};
use crate::error::RepoError;

/// Repository for managing the packaging aggregation hierarchy.
///
/// Models parent-child relationships (Pallet → Case → Carton → Unit).
/// The event_id links each relationship back to the EPCIS AggregationEvent that created it.
#[trait_variant::make(Send)]
pub trait AggregationRepository: Sync {
    /// Record that parent contains child, linked to the source event.
    async fn add_child(&self, parent: &Epc, child: &Epc, event_id: &EventId) -> Result<(), RepoError>;

    /// Remove a child from its parent (for disaggregation/unpack events).
    async fn remove_child(&self, parent: &Epc, child: &Epc) -> Result<(), RepoError>;

    /// Get direct children of a parent.
    async fn get_children(&self, parent: &Epc) -> Result<Vec<Epc>, RepoError>;

    /// Get all ancestors of a child, from immediate parent to root.
    async fn get_ancestors(&self, child: &Epc) -> Result<Vec<Epc>, RepoError>;

    /// Get the full hierarchy tree rooted at the given EPC.
    async fn get_full_hierarchy(&self, root: &Epc) -> Result<AggregationTree, RepoError>;
}
```

**Step 3: Wire up repository/mod.rs**

```rust
pub mod aggregation;
pub mod event;

pub use aggregation::*;
pub use event::*;
```

**Step 4: Verify it compiles**

Run: `cargo check -p ephemeris-core`
Expected: Compiles. The `trait_variant::make(Send)` generates a `SendEventRepository` and `SendAggregationRepository` variant automatically.

**Step 5: Commit**

```bash
git add crates/ephemeris-core/src/repository/
git commit -m "feat(core): add EventRepository and AggregationRepository async traits"
```

---

## Phase 2: PostgreSQL Connector

### Task 5: Scaffold ephemeris-pg Crate

**Files:**
- Create: `crates/ephemeris-pg/Cargo.toml`
- Create: `crates/ephemeris-pg/src/lib.rs`
- Create: `crates/ephemeris-pg/src/schema.rs`
- Modify: `Cargo.toml` (add to workspace members)

**Step 1: Create Cargo.toml for ephemeris-pg**

```toml
[package]
name = "ephemeris-pg"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
ephemeris-core = { path = "../ephemeris-core" }
tokio-postgres = { workspace = true }
deadpool-postgres = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
testcontainers = { workspace = true }
testcontainers-modules = { workspace = true }
```

**Step 2: Add to workspace members in root Cargo.toml**

Add `"crates/ephemeris-pg"` to the `[workspace] members` array.

**Step 3: Create schema.rs with SQL migration**

```rust
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
```

**Step 4: Create lib.rs**

```rust
pub mod schema;
pub mod event_repo;
pub mod aggregation_repo;

pub use event_repo::PgEventRepository;
pub use aggregation_repo::PgAggregationRepository;
```

**Step 5: Verify it compiles**

Run: `cargo check -p ephemeris-pg`

**Step 6: Commit**

```bash
git add crates/ephemeris-pg/ Cargo.toml
git commit -m "feat(pg): scaffold PostgreSQL connector crate with schema"
```

---

### Task 6: PostgreSQL EventRepository Implementation

**Files:**
- Create: `crates/ephemeris-pg/src/event_repo.rs`

**Step 1: Write integration test (fails — no implementation)**

At end of `event_repo.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    async fn setup_test_db() -> (PgEventRepository, impl Drop) {
        let container = Postgres::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let url = format!("host={} port={} user=postgres password=postgres dbname=postgres", host, port);

        let repo = PgEventRepository::connect(&url).await.unwrap();
        repo.run_migrations().await.unwrap();
        (repo, container)
    }

    #[tokio::test]
    async fn test_store_and_retrieve_event() {
        let (repo, _container) = setup_test_db().await;

        let json: serde_json::Value = serde_json::from_str(r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2005-04-03T20:33:31.116-06:00",
            "eventTimeZoneOffset": "-06:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"],
            "bizStep": "shipping"
        }"#).unwrap();

        let event: EpcisEvent = serde_json::from_value(json).unwrap();
        let event_id = repo.store_event(&event).await.unwrap();
        let retrieved = repo.get_event(&event_id).await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_idempotent_store() {
        let (repo, _container) = setup_test_db().await;

        let json: serde_json::Value = serde_json::from_str(r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2005-04-03T20:33:31.116-06:00",
            "eventTimeZoneOffset": "-06:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"]
        }"#).unwrap();
        let event: EpcisEvent = serde_json::from_value(json).unwrap();

        let id1 = repo.store_event(&event).await.unwrap();
        let id2 = repo.store_event(&event).await.unwrap();
        assert_eq!(id1.0, id2.0);
    }
}
```

**Step 2: Implement PgEventRepository**

```rust
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use ephemeris_core::domain::{EpcisEvent, EventId, EventQuery};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::EventRepository;
use serde_json::Value;
use tokio_postgres::NoTls;
use uuid::Uuid;

use crate::schema::INIT_SCHEMA;

pub struct PgEventRepository {
    pool: Pool,
}

impl PgEventRepository {
    pub async fn connect(conn_str: &str) -> Result<Self, RepoError> {
        let mut cfg = Config::new();
        // Parse the connection string parameters
        for part in conn_str.split_whitespace() {
            if let Some((key, val)) = part.split_once('=') {
                match key {
                    "host" => cfg.host = Some(val.to_string()),
                    "port" => cfg.port = val.parse().ok(),
                    "user" => cfg.user = Some(val.to_string()),
                    "password" => cfg.password = Some(val.to_string()),
                    "dbname" => cfg.dbname = Some(val.to_string()),
                    _ => {}
                }
            }
        }
        cfg.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> Result<(), RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;
        client.batch_execute(INIT_SCHEMA).await
            .map_err(|e| RepoError::Query(e.to_string()))?;
        Ok(())
    }

    fn hash_event(event: &EpcisEvent) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let json = serde_json::to_string(event).unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        json.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn event_type_name(event: &EpcisEvent) -> &'static str {
        match event {
            EpcisEvent::ObjectEvent(_) => "ObjectEvent",
            EpcisEvent::AggregationEvent(_) => "AggregationEvent",
            EpcisEvent::TransformationEvent(_) => "TransformationEvent",
        }
    }

    fn event_time(event: &EpcisEvent) -> chrono::DateTime<chrono::FixedOffset> {
        match event {
            EpcisEvent::ObjectEvent(d) => d.common.event_time,
            EpcisEvent::AggregationEvent(d) => d.common.event_time,
            EpcisEvent::TransformationEvent(d) => d.common.event_time,
        }
    }
}

impl EventRepository for PgEventRepository {
    async fn store_event(&self, event: &EpcisEvent) -> Result<EventId, RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let hash = Self::hash_event(event);
        let event_data: Value = serde_json::to_value(event)
            .map_err(|e| RepoError::Serialization(e.to_string()))?;

        // Check for duplicate (idempotent)
        let existing = client
            .query_opt("SELECT id FROM epcis_events WHERE event_hash = $1", &[&hash])
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        if let Some(row) = existing {
            let id: Uuid = row.get(0);
            return Ok(EventId(id));
        }

        let id = Uuid::new_v4();
        let event_type = Self::event_type_name(event);
        let event_time = Self::event_time(event);

        client
            .execute(
                "INSERT INTO epcis_events (id, event_type, event_time, event_data, event_hash) VALUES ($1, $2, $3, $4, $5)",
                &[&id, &event_type, &event_time, &event_data, &hash],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(EventId(id))
    }

    async fn get_event(&self, id: &EventId) -> Result<Option<EpcisEvent>, RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let row = client
            .query_opt("SELECT event_data FROM epcis_events WHERE id = $1", &[&id.0])
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        match row {
            Some(row) => {
                let data: Value = row.get(0);
                let event: EpcisEvent = serde_json::from_value(data)
                    .map_err(|e| RepoError::Serialization(e.to_string()))?;
                Ok(Some(event))
            }
            None => Ok(None),
        }
    }

    async fn query_events(&self, query: &EventQuery) -> Result<Vec<EpcisEvent>, RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let mut sql = String::from("SELECT event_data FROM epcis_events WHERE 1=1");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
        let mut idx = 1;

        if let Some(ref ge) = query.ge_event_time {
            sql.push_str(&format!(" AND event_time >= ${}", idx));
            params.push(Box::new(ge.clone()));
            idx += 1;
        }
        if let Some(ref lt) = query.lt_event_time {
            sql.push_str(&format!(" AND event_time < ${}", idx));
            params.push(Box::new(lt.clone()));
            idx += 1;
        }
        if let Some(ref biz_step) = query.eq_biz_step {
            sql.push_str(&format!(" AND event_data->>'bizStep' = ${}", idx));
            params.push(Box::new(biz_step.clone()));
            idx += 1;
        }
        if let Some(ref match_epc) = query.match_epc {
            sql.push_str(&format!(" AND event_data->'epcList' ? ${}", idx));
            params.push(Box::new(match_epc.clone()));
            idx += 1;
        }

        let limit = query.per_page.unwrap_or(100);
        sql.push_str(&format!(" ORDER BY event_time DESC LIMIT ${}", idx));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = client
            .query(&sql, &param_refs)
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let mut events = Vec::new();
        for row in rows {
            let data: Value = row.get(0);
            let event: EpcisEvent = serde_json::from_value(data)
                .map_err(|e| RepoError::Serialization(e.to_string()))?;
            events.push(event);
        }

        Ok(events)
    }
}
```

**Step 3: Run integration tests**

Run: `cargo test -p ephemeris-pg -- --test-threads=1`
Expected: All pass (requires Docker running for testcontainers).

**Step 4: Commit**

```bash
git add crates/ephemeris-pg/src/event_repo.rs
git commit -m "feat(pg): implement PgEventRepository with idempotent store and JSONB queries"
```

---

### Task 7: PostgreSQL AggregationRepository (ltree)

**Files:**
- Create: `crates/ephemeris-pg/src/aggregation_repo.rs`

**Step 1: Write integration tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // (reuse setup_test_db pattern from Task 6)

    #[tokio::test]
    async fn test_add_and_get_children() {
        let (event_repo, agg_repo, _container) = setup_test_db().await;

        // Store a dummy event to get an event_id
        let event = make_test_aggregation_event();
        let event_id = event_repo.store_event(&event).await.unwrap();

        let pallet = Epc::new("urn:epc:id:sscc:0614141.0000001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.0000002");
        let case2 = Epc::new("urn:epc:id:sscc:0614141.0000003");

        agg_repo.add_child(&pallet, &case1, &event_id).await.unwrap();
        agg_repo.add_child(&pallet, &case2, &event_id).await.unwrap();

        let children = agg_repo.get_children(&pallet).await.unwrap();
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_get_ancestors() {
        let (event_repo, agg_repo, _container) = setup_test_db().await;
        let event_id = store_dummy_event(&event_repo).await;

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C001");
        let unit1 = Epc::new("urn:epc:id:sgtin:0614141.107346.001");

        agg_repo.add_child(&pallet, &case1, &event_id).await.unwrap();
        agg_repo.add_child(&case1, &unit1, &event_id).await.unwrap();

        let ancestors = agg_repo.get_ancestors(&unit1).await.unwrap();
        assert_eq!(ancestors.len(), 2); // case1, pallet
    }

    #[tokio::test]
    async fn test_full_hierarchy() {
        let (event_repo, agg_repo, _container) = setup_test_db().await;
        let event_id = store_dummy_event(&event_repo).await;

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C001");
        let unit1 = Epc::new("urn:epc:id:sgtin:0614141.107346.001");
        let unit2 = Epc::new("urn:epc:id:sgtin:0614141.107346.002");

        agg_repo.add_child(&pallet, &case1, &event_id).await.unwrap();
        agg_repo.add_child(&case1, &unit1, &event_id).await.unwrap();
        agg_repo.add_child(&case1, &unit2, &event_id).await.unwrap();

        let tree = agg_repo.get_full_hierarchy(&pallet).await.unwrap();
        assert_eq!(tree.root, pallet);
        assert_eq!(tree.nodes.len(), 1); // case1
        assert_eq!(tree.nodes[0].children.len(), 2); // unit1, unit2
    }
}
```

**Step 2: Implement PgAggregationRepository**

The key design: Each EPC gets an ltree label derived by replacing non-alphanumeric chars with underscores. The `path` column stores the full ancestry path. When adding a child, look up the parent's path and append the child's label.

```rust
pub struct PgAggregationRepository {
    pool: Pool,
}

impl PgAggregationRepository {
    pub async fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Convert an EPC URI to a valid ltree label (alphanumeric + underscore only).
    fn epc_to_label(epc: &Epc) -> String {
        epc.as_str()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect()
    }
}

impl AggregationRepository for PgAggregationRepository {
    async fn add_child(&self, parent: &Epc, child: &Epc, event_id: &EventId) -> Result<(), RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let parent_label = Self::epc_to_label(parent);
        let child_label = Self::epc_to_label(child);

        // Find parent's path, or create root entry for parent
        let parent_path = client
            .query_opt(
                "SELECT path FROM aggregation WHERE child_epc = $1",
                &[&parent.as_str()],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let parent_path_str = match parent_path {
            Some(row) => {
                let p: String = row.get(0);
                p
            }
            None => parent_label.clone(), // Root node — path is just its own label
        };

        let child_path = format!("{}.{}", parent_path_str, child_label);

        client
            .execute(
                "INSERT INTO aggregation (child_epc, parent_epc, path, event_id) VALUES ($1, $2, $3::ltree, $4)
                 ON CONFLICT (child_epc) DO UPDATE SET parent_epc = $2, path = $3::ltree, event_id = $4",
                &[&child.as_str(), &parent.as_str(), &child_path, &event_id.0],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(())
    }

    async fn remove_child(&self, _parent: &Epc, child: &Epc) -> Result<(), RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        // Remove the child and all its descendants
        let child_path_row = client
            .query_opt("SELECT path FROM aggregation WHERE child_epc = $1", &[&child.as_str()])
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        if let Some(row) = child_path_row {
            let path: String = row.get(0);
            client
                .execute(
                    "DELETE FROM aggregation WHERE path <@ $1::ltree",
                    &[&path],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;
        }

        Ok(())
    }

    async fn get_children(&self, parent: &Epc) -> Result<Vec<Epc>, RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let rows = client
            .query(
                "SELECT child_epc FROM aggregation WHERE parent_epc = $1",
                &[&parent.as_str()],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| Epc::new(r.get::<_, String>(0))).collect())
    }

    async fn get_ancestors(&self, child: &Epc) -> Result<Vec<Epc>, RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        // Walk up the parent chain
        let mut ancestors = Vec::new();
        let mut current = child.clone();

        loop {
            let row = client
                .query_opt(
                    "SELECT parent_epc FROM aggregation WHERE child_epc = $1",
                    &[&current.as_str()],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;

            match row {
                Some(r) => {
                    let parent_epc: String = r.get(0);
                    let parent = Epc::new(&parent_epc);
                    ancestors.push(parent.clone());
                    current = parent;
                }
                None => break,
            }
        }

        Ok(ancestors)
    }

    async fn get_full_hierarchy(&self, root: &Epc) -> Result<AggregationTree, RepoError> {
        let client = self.pool.get().await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        // Get all descendants using ltree
        let root_label = Self::epc_to_label(root);

        let rows = client
            .query(
                "SELECT child_epc, parent_epc, path FROM aggregation WHERE path <@ $1::ltree ORDER BY path",
                &[&root_label],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        // Build tree from flat rows
        let tree = self.build_tree(root, &rows);
        Ok(tree)
    }
}
```

Note: The `build_tree` helper method reconstructs the tree from flat DB rows. Implementation details are straightforward recursion over the sorted path results.

**Step 3: Run integration tests**

Run: `cargo test -p ephemeris-pg -- --test-threads=1`
Expected: All pass.

**Step 4: Commit**

```bash
git add crates/ephemeris-pg/src/aggregation_repo.rs
git commit -m "feat(pg): implement PgAggregationRepository with ltree hierarchy"
```

---

## Phase 3: ArangoDB Connector

### Task 8: Scaffold ephemeris-arango Crate

**Files:**
- Create: `crates/ephemeris-arango/Cargo.toml`
- Create: `crates/ephemeris-arango/src/lib.rs`
- Create: `crates/ephemeris-arango/src/client.rs`
- Create: `crates/ephemeris-arango/src/aggregation_repo.rs`
- Modify: `Cargo.toml` (add to workspace members, add feature flag)

**Step 1: Create Cargo.toml**

```toml
[package]
name = "ephemeris-arango"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
ephemeris-core = { path = "../ephemeris-core" }
reqwest = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
testcontainers = { workspace = true }
```

**Step 2: Update workspace root Cargo.toml**

Add `"crates/ephemeris-arango"` to members. Add feature flags:

```toml
[workspace.features]
enterprise-arango = []
```

Note: Feature flags on workspace level are defined in `ephemeris-app`'s Cargo.toml, not the workspace root. The workspace root just lists the members.

**Step 3: Implement ArangoDB HTTP client wrapper**

In `crates/ephemeris-arango/src/client.rs`:

```rust
use reqwest::Client;
use serde_json::{json, Value};

pub struct ArangoClient {
    client: Client,
    base_url: String,
    database: String,
    auth_header: Option<String>,
}

impl ArangoClient {
    pub async fn connect(
        base_url: &str,
        database: &str,
        username: &str,
        password: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let client = Client::new();

        // Get JWT token
        let auth_resp: Value = client
            .post(format!("{}/_open/auth", base_url))
            .json(&json!({"username": username, "password": password}))
            .send()
            .await?
            .json()
            .await?;

        let token = auth_resp["jwt"].as_str().map(|t| format!("bearer {}", t));

        Ok(Self {
            client,
            base_url: base_url.to_string(),
            database: database.to_string(),
            auth_header: token,
        })
    }

    pub async fn connect_no_auth(base_url: &str, database: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            database: database.to_string(),
            auth_header: None,
        }
    }

    fn db_url(&self, path: &str) -> String {
        format!("{}/_db/{}{}", self.base_url, self.database, path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = self.db_url(path);
        let mut req = self.client.request(method, &url);
        if let Some(ref auth) = self.auth_header {
            req = req.header("Authorization", auth);
        }
        req
    }

    pub async fn create_graph(&self, name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.request(reqwest::Method::POST, "/_api/gharial")
            .json(&json!({
                "name": name,
                "edgeDefinitions": [{
                    "collection": "contains",
                    "from": ["packaging"],
                    "to": ["packaging"]
                }]
            }))
            .send()
            .await?;
        Ok(())
    }

    pub async fn insert_vertex(&self, graph: &str, collection: &str, data: &Value) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let resp: Value = self.request(reqwest::Method::POST, &format!("/_api/gharial/{}/vertex/{}", graph, collection))
            .json(data)
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn insert_edge(&self, graph: &str, collection: &str, from: &str, to: &str) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let resp: Value = self.request(reqwest::Method::POST, &format!("/_api/gharial/{}/edge/{}", graph, collection))
            .json(&json!({"_from": from, "_to": to}))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn execute_aql(&self, query: &str, bind_vars: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error + Send + Sync>> {
        let resp: Value = self.request(reqwest::Method::POST, "/_api/cursor")
            .json(&json!({
                "query": query,
                "bindVars": bind_vars,
                "batchSize": 1000
            }))
            .send()
            .await?
            .json()
            .await?;

        let results = resp["result"].as_array().cloned().unwrap_or_default();
        Ok(results)
    }
}
```

**Step 4: Implement ArangoAggregationRepository**

In `crates/ephemeris-arango/src/aggregation_repo.rs`, implement `AggregationRepository` using the graph API. Vertices are stored in a `packaging` collection, edges in `contains`. Graph traversal uses AQL `OUTBOUND`/`INBOUND` queries.

Key methods:
- `add_child`: Insert vertex for child (if not exists), insert edge from parent to child
- `get_children`: AQL `FOR v IN 1..1 OUTBOUND @start GRAPH @g RETURN v`
- `get_ancestors`: AQL `FOR v IN 1..100 INBOUND @start GRAPH @g RETURN v`
- `get_full_hierarchy`: AQL `FOR v, e, p IN 1..100 OUTBOUND @start GRAPH @g RETURN {vertex: v, depth: LENGTH(p.edges)}`

**Step 5: Write integration tests using testcontainers with ArangoDB image**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers::{GenericImage, runners::AsyncRunner};

    async fn setup_arango() -> (ArangoAggregationRepository, impl Drop) {
        let container = GenericImage::new("arangodb", "3.12")
            .with_env_var("ARANGO_NO_AUTH", "1")
            .with_exposed_port(8529.into())
            .start()
            .await
            .unwrap();

        let port = container.get_host_port_ipv4(8529).await.unwrap();
        let host = container.get_host().await.unwrap();
        let base_url = format!("http://{}:{}", host, port);

        // Wait for ArangoDB to be ready (poll health endpoint)
        // ... (retry loop on GET /_api/version)

        let client = ArangoClient::connect_no_auth(&base_url, "_system").await;
        client.create_graph("packaging_graph").await.unwrap();

        let repo = ArangoAggregationRepository::new(client, "packaging_graph".to_string());
        (repo, container)
    }

    #[tokio::test]
    async fn test_add_and_get_children_arango() {
        let (repo, _container) = setup_arango().await;
        // Same test logic as PostgreSQL — proves the abstraction holds
        let pallet = Epc::new("urn:epc:id:sscc:0614141.P001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C001");
        let event_id = EventId::new();

        repo.add_child(&pallet, &case1, &event_id).await.unwrap();
        let children = repo.get_children(&pallet).await.unwrap();
        assert_eq!(children.len(), 1);
    }
}
```

**Step 6: Run tests, commit**

Run: `cargo test -p ephemeris-arango -- --test-threads=1`

```bash
git add crates/ephemeris-arango/
git commit -m "feat(arango): implement ArangoDB AggregationRepository via HTTP REST API"
```

---

## Phase 4: MQTT Ingestion

### Task 9: Scaffold ephemeris-mqtt Crate

**Files:**
- Create: `crates/ephemeris-mqtt/Cargo.toml`
- Create: `crates/ephemeris-mqtt/src/lib.rs`
- Create: `crates/ephemeris-mqtt/src/subscriber.rs`
- Create: `crates/ephemeris-mqtt/src/handler.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "ephemeris-mqtt"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
ephemeris-core = { path = "../ephemeris-core" }
rumqttc = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }

[dev-dependencies]
mockall = { workspace = true }
```

**Step 2: Implement EventHandler**

In `handler.rs`, the handler receives parsed EPCIS events and routes them to the appropriate repository methods:

```rust
use ephemeris_core::domain::{Epc, EpcisEvent};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::{EventRepository, AggregationRepository};

pub struct EventHandler<E: EventRepository, A: AggregationRepository> {
    event_repo: E,
    agg_repo: A,
}

impl<E: EventRepository, A: AggregationRepository> EventHandler<E, A> {
    pub fn new(event_repo: E, agg_repo: A) -> Self {
        Self { event_repo, agg_repo }
    }

    pub async fn handle_event(&self, event: &EpcisEvent) -> Result<(), RepoError> {
        // 1. Store the raw event
        let event_id = self.event_repo.store_event(event).await?;

        // 2. If aggregation event, update the hierarchy
        if let EpcisEvent::AggregationEvent(data) = event {
            if let Some(ref parent_id) = data.parent_id {
                let parent = Epc::new(parent_id);
                match data.action {
                    ephemeris_core::domain::Action::Add | ephemeris_core::domain::Action::Observe => {
                        for child_epc in &data.child_epcs {
                            self.agg_repo.add_child(&parent, &Epc::new(child_epc), &event_id).await?;
                        }
                    }
                    ephemeris_core::domain::Action::Delete => {
                        for child_epc in &data.child_epcs {
                            self.agg_repo.remove_child(&parent, &Epc::new(child_epc)).await?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
```

**Step 3: Write unit tests with mocked repositories**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    // Generate mock implementations of the repository traits
    // and test that handle_event correctly routes to store_event + add_child
}
```

**Step 4: Implement MqttSubscriber**

In `subscriber.rs`, connect to the MQTT broker, subscribe to topics, deserialize incoming payloads, and pass them to `EventHandler`.

```rust
use rumqttc::v5::{AsyncClient, Event, MqttOptions};
use rumqttc::v5::mqttbytes::v5::Packet;

pub struct MqttSubscriber {
    client: AsyncClient,
    eventloop: rumqttc::v5::EventLoop,
}

impl MqttSubscriber {
    pub fn new(broker_url: &str, client_id: &str) -> Self {
        let mut opts = MqttOptions::new(client_id, broker_url, 1883);
        opts.set_keep_alive(std::time::Duration::from_secs(30));
        let (client, eventloop) = AsyncClient::new(opts, 100);
        Self { client, eventloop }
    }

    pub async fn subscribe(&self, topics: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        for topic in topics {
            self.client.subscribe(topic, rumqttc::v5::mqttbytes::QoS::AtLeastOnce).await?;
        }
        Ok(())
    }

    /// Run the event loop. Calls handler for each received EPCIS event.
    pub async fn run<E: EventRepository, A: AggregationRepository>(
        &mut self,
        handler: &EventHandler<E, A>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match self.eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    let payload = &publish.payload;
                    match serde_json::from_slice::<EpcisEvent>(payload) {
                        Ok(event) => {
                            if let Err(e) = handler.handle_event(&event).await {
                                tracing::error!("Failed to handle event: {}", e);
                                // TODO: publish to dead-letter topic
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Invalid EPCIS payload: {}", e);
                            // TODO: publish to dead-letter topic
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("MQTT error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
}
```

**Step 5: Commit**

```bash
git add crates/ephemeris-mqtt/
git commit -m "feat(mqtt): implement MQTT subscriber with event handler routing"
```

---

## Phase 5: REST API

### Task 10: Scaffold ephemeris-api Crate

**Files:**
- Create: `crates/ephemeris-api/Cargo.toml`
- Create: `crates/ephemeris-api/src/lib.rs`
- Create: `crates/ephemeris-api/src/routes/mod.rs`
- Create: `crates/ephemeris-api/src/routes/events.rs`
- Create: `crates/ephemeris-api/src/routes/hierarchy.rs`
- Create: `crates/ephemeris-api/src/routes/health.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "ephemeris-api"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
ephemeris-core = { path = "../ephemeris-core" }
axum = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }

[dev-dependencies]
mockall = { workspace = true }
```

**Step 2: Implement REST routes**

Events API:
- `GET /events` — query events with EPCIS 2.0 query params
- `GET /events/:event_id` — get single event by ID
- `POST /events` — sync capture of a single event (returns 201)

Hierarchy API:
- `GET /hierarchy/:epc` — get full aggregation tree for an EPC
- `GET /hierarchy/:epc/children` — get direct children
- `GET /hierarchy/:epc/ancestors` — get ancestors

Health:
- `GET /health` — returns 200 with `{"status": "ok"}`

The routes use Axum extractors with `Arc<dyn EventRepository>` or generic state. Since we're using static dispatch, the router is generic over the repository types.

```rust
use axum::{Router, routing::get, routing::post, extract::State, extract::Path, extract::Query, Json};
use std::sync::Arc;

pub struct AppState<E: EventRepository, A: AggregationRepository> {
    pub event_repo: E,
    pub agg_repo: A,
}

pub fn create_router<E: EventRepository + 'static, A: AggregationRepository + 'static>(
    state: Arc<AppState<E, A>>,
) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/events", get(query_events::<E, A>).post(capture_event::<E, A>))
        .route("/events/{event_id}", get(get_event::<E, A>))
        .route("/hierarchy/{epc}", get(get_hierarchy::<E, A>))
        .route("/hierarchy/{epc}/children", get(get_children::<E, A>))
        .route("/hierarchy/{epc}/ancestors", get(get_ancestors::<E, A>))
        .with_state(state)
}
```

**Step 3: Write tests with mock repositories, commit**

```bash
git add crates/ephemeris-api/
git commit -m "feat(api): implement REST API routes for events and hierarchy"
```

---

## Phase 6: App Wiring & Config

### Task 11: Scaffold ephemeris-app Crate

**Files:**
- Create: `crates/ephemeris-app/Cargo.toml`
- Create: `crates/ephemeris-app/src/main.rs`
- Create: `crates/ephemeris-app/src/config.rs`
- Create: `ephemeris.toml` (example config at project root)

**Step 1: Create Cargo.toml with feature flags**

```toml
[package]
name = "ephemeris-app"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "ephemeris"
path = "src/main.rs"

[features]
default = []
enterprise-arango = ["dep:ephemeris-arango"]
enterprise-couch = []
enterprise = ["enterprise-arango", "enterprise-couch"]

[dependencies]
ephemeris-core = { path = "../ephemeris-core" }
ephemeris-pg = { path = "../ephemeris-pg" }
ephemeris-mqtt = { path = "../ephemeris-mqtt" }
ephemeris-api = { path = "../ephemeris-api" }
ephemeris-arango = { path = "../ephemeris-arango", optional = true }

tokio = { workspace = true }
config = { workspace = true }
clap = { workspace = true }
serde = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
toml = { workspace = true }
```

**Step 2: Implement config.rs**

```rust
use config::{Config, Environment, File};
use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(name = "ephemeris", version, about = "Saturnis Ephemeris — Track & Trace Engine")]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "ephemeris.toml")]
    pub config: String,

    /// Database backend
    #[arg(long)]
    pub database_backend: Option<String>,

    /// API bind address
    #[arg(long)]
    pub api_bind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub mqtt: MqttConfig,
    pub database: DatabaseConfig,
    pub api: ApiConfig,
}

#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    pub broker_url: String,
    pub client_id: String,
    pub topics: Vec<String>,
    pub qos: u8,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub backend: String,
    pub postgres: Option<PostgresConfig>,
    #[cfg(feature = "enterprise-arango")]
    pub arango: Option<ArangoConfig>,
}

#[derive(Debug, Deserialize)]
pub struct PostgresConfig {
    pub url: String,
    pub pool_size: Option<u32>,
}

#[cfg(feature = "enterprise-arango")]
#[derive(Debug, Deserialize)]
pub struct ArangoConfig {
    pub url: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiConfig {
    pub bind: String,
}

impl AppConfig {
    pub fn load(cli: &Cli) -> Result<Self, config::ConfigError> {
        Config::builder()
            .add_source(File::with_name(&cli.config).required(false))
            .add_source(Environment::with_prefix("EPHEMERIS").separator("__"))
            .build()?
            .try_deserialize()
    }
}
```

**Step 3: Implement main.rs with backend wiring**

```rust
use clap::Parser;
use std::sync::Arc;

mod config;
use config::{AppConfig, Cli};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = AppConfig::load(&cli)?;

    match config.database.backend.as_str() {
        "postgres" => {
            let pg_config = config.database.postgres.as_ref()
                .expect("PostgreSQL config required when backend=postgres");
            let event_repo = ephemeris_pg::PgEventRepository::connect(&pg_config.url).await?;
            event_repo.run_migrations().await?;
            let agg_repo = ephemeris_pg::PgAggregationRepository::new(/* pool */);
            run_app(event_repo, agg_repo, config).await
        }
        #[cfg(feature = "enterprise-arango")]
        "arango" => {
            // ArangoDB for aggregation, PostgreSQL for events
            todo!("Wire ArangoDB aggregation + PG events")
        }
        other => {
            #[cfg(not(feature = "enterprise-arango"))]
            if other == "arango" {
                eprintln!("ERROR: ArangoDB backend requires the enterprise build.");
                eprintln!("This binary was compiled without enterprise features.");
                eprintln!("Contact sales@saturnis.io for enterprise licensing.");
                std::process::exit(1);
            }
            eprintln!("Unknown database backend: {}", other);
            std::process::exit(1);
        }
    }
}

async fn run_app<E, A>(
    event_repo: E,
    agg_repo: A,
    config: AppConfig,
) -> Result<(), Box<dyn std::error::Error>>
where
    E: ephemeris_core::repository::EventRepository + 'static,
    A: ephemeris_core::repository::AggregationRepository + 'static,
{
    let state = Arc::new(ephemeris_api::AppState { event_repo, agg_repo });

    // Start API server
    let router = ephemeris_api::create_router(state.clone());
    let listener = tokio::net::TcpListener::bind(&config.api.bind).await?;
    tracing::info!("API listening on {}", config.api.bind);

    // Start MQTT subscriber (in background task)
    // let mut mqtt = MqttSubscriber::new(...);
    // tokio::spawn(async move { mqtt.run(&handler).await });

    axum::serve(listener, router).await?;
    Ok(())
}
```

**Step 4: Create example ephemeris.toml at project root**

(Use the config from the design doc)

**Step 5: Verify default build compiles, enterprise build compiles**

Run: `cargo build -p ephemeris-app`
Run: `cargo build -p ephemeris-app --features enterprise-arango`

**Step 6: Commit**

```bash
git add crates/ephemeris-app/ ephemeris.toml
git commit -m "feat(app): implement config loading and backend wiring with feature flags"
```

---

## Phase 7: Test Kit & E2E

### Task 12: Scaffold ephemeris-testkit

**Files:**
- Create: `crates/ephemeris-testkit/Cargo.toml`
- Create: `crates/ephemeris-testkit/src/lib.rs`
- Create: `crates/ephemeris-testkit/src/event_generator.rs`
- Create: `crates/ephemeris-testkit/src/mqtt_publisher.rs`
- Create: `crates/ephemeris-testkit/src/dashboard.rs`
- Create: `crates/ephemeris-testkit/static/index.html`

The testkit provides:
1. **Event generator** — creates valid random EPCIS events for testing
2. **MQTT publisher** — sends events to a broker at configurable rates
3. **Dashboard** — simple web UI for manual testing (served by a small axum server)

The dashboard is a single HTML page with:
- Event type selector (ObjectEvent, AggregationEvent)
- JSON editor for the payload
- Send button (posts to testkit HTTP endpoint which publishes to MQTT)
- Live event feed (polls the Ephemeris REST API)

**Step 1: Create test fixtures**

Create `tests/fixtures/object_event.json` and `tests/fixtures/aggregation_event.json` with valid EPCIS 2.0 payloads from the GS1 spec examples.

**Step 2: Implement event_generator.rs**

Generates random but valid EPCIS events with realistic EPCs, timestamps, and bizStep values.

**Step 3: Implement mqtt_publisher.rs**

Takes events and publishes them to a configurable MQTT broker.

**Step 4: Implement dashboard**

Minimal axum server serving `static/index.html` + a `POST /send` endpoint that publishes to MQTT.

**Step 5: Commit**

```bash
git add crates/ephemeris-testkit/ tests/fixtures/
git commit -m "feat(testkit): add event generator, MQTT publisher, and test dashboard UI"
```

---

### Task 13: E2E Pipeline Test

**Files:**
- Create: `tests/e2e/pipeline_test.rs`
- Create: `docker-compose.test.yml`

**Step 1: Create docker-compose.test.yml**

```yaml
services:
  mosquitto:
    image: eclipse-mosquitto:2
    ports:
      - "1883:1883"
    volumes:
      - ./tests/mosquitto.conf:/mosquitto/config/mosquitto.conf

  postgres:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: ephemeris_test
    ports:
      - "5432:5432"
```

**Step 2: Write E2E test**

The test:
1. Starts containers (Mosquitto + PostgreSQL) via testcontainers
2. Starts Ephemeris app (ingest + API) pointing at those containers
3. Publishes EPCIS events via MQTT
4. Waits briefly, then queries the REST API
5. Verifies events were stored and aggregation hierarchy is correct

**Step 3: Commit**

```bash
git add tests/ docker-compose.test.yml
git commit -m "test: add E2E pipeline test (MQTT → PostgreSQL → REST API)"
```

---

## Phase 8: CI & Validation

### Task 14: CI Pipeline

**Files:**
- Create: `.github/workflows/ci.yml`

**Step 1: Create CI workflow**

```yaml
name: CI
on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Format check
        run: cargo fmt --check

      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: License check
        run: cargo install cargo-deny && cargo deny check licenses

  test:
    runs-on: ubuntu-latest
    needs: check
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Unit tests
        run: cargo test --workspace --lib

      - name: Integration tests (requires Docker)
        run: cargo test --workspace -- --test-threads=1

  build-enterprise:
    runs-on: ubuntu-latest
    needs: check
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Build with enterprise features
        run: cargo build --features enterprise

      - name: Build default (open-core only)
        run: cargo build
```

**Step 2: Commit**

```bash
git add .github/
git commit -m "ci: add GitHub Actions workflow with lint, license check, and tests"
```

---

### Task 15: Final Validation

**Step 1: Run the full suite locally**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo deny check licenses
cargo test --workspace -- --test-threads=1
cargo build
cargo build --features enterprise
```

**Step 2: Verify the abstraction proof**

Run the same aggregation test against both PostgreSQL and ArangoDB backends. Both should produce identical results, proving the trait abstraction holds.

**Step 3: Verify feature flag isolation**

```bash
# Default build should NOT contain any ArangoDB code
cargo build 2>&1 | grep -i arango  # Should find nothing
```

**Step 4: Commit any final fixes**

```bash
git add -A
git commit -m "chore: final MVP validation and cleanup"
```

---

## Summary: Commit Sequence

| # | Commit | Phase |
|---|---|---|
| 1 | `feat: scaffold Cargo workspace with ephemeris-core crate` | 1 |
| 2 | `feat(core): add EPCIS 2.0 domain types with serde roundtrip tests` | 1 |
| 3 | `feat(core): add repository error types` | 1 |
| 4 | `feat(core): add EventRepository and AggregationRepository async traits` | 1 |
| 5 | `feat(pg): scaffold PostgreSQL connector crate with schema` | 2 |
| 6 | `feat(pg): implement PgEventRepository with idempotent store and JSONB queries` | 2 |
| 7 | `feat(pg): implement PgAggregationRepository with ltree hierarchy` | 2 |
| 8 | `feat(arango): implement ArangoDB AggregationRepository via HTTP REST API` | 3 |
| 9 | `feat(mqtt): implement MQTT subscriber with event handler routing` | 4 |
| 10 | `feat(api): implement REST API routes for events and hierarchy` | 5 |
| 11 | `feat(app): implement config loading and backend wiring with feature flags` | 6 |
| 12 | `feat(testkit): add event generator, MQTT publisher, and test dashboard UI` | 7 |
| 13 | `test: add E2E pipeline test (MQTT → PostgreSQL → REST API)` | 7 |
| 14 | `ci: add GitHub Actions workflow with lint, license check, and tests` | 8 |
| 15 | `chore: final MVP validation and cleanup` | 8 |

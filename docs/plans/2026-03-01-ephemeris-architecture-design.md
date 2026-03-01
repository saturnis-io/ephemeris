# Ephemeris Architecture Design

**Date:** 2026-03-01
**Status:** Approved
**Product:** Saturnis Ephemeris — Enterprise-grade Track & Trace / Serialization Engine

---

## 1. Product Vision

Ephemeris is an open-core Track & Trace (T&T) and serialization engine that replaces legacy ISA-95 bottlenecks (XML, SOAP, per-serial billing) with EPCIS 2.0, event-driven edge ingestion, and a decoupled database architecture.

- **Open Core:** Core ingestion, business logic, and PostgreSQL connector are free.
- **Monetization:** Self-serve licensing portal unlocks enterprise features (HA clustering, advanced compliance, premium DB connectors). Revenue is NOT per-serial-number.
- **Target Deployment:** Level 3 (Site/Edge). Must operate through cloud disconnections.

## 2. Language & Runtime

**Rust.** Single static binaries, no runtime dependencies, excellent type safety for the repository pattern. Deployment artifacts: MSI installer (Windows), static binary (Linux), Docker image.

## 3. Legal Firewall: Strict Abstraction

To protect the proprietary codebase from viral open-source licenses (GPLv3, SSPL, BSL):

### Hard Rules (Enforced in CI)

1. **No embedded engines.** Never add `rusqlite`, `sled`, `rocksdb`, or any crate that bundles a database engine. Ephemeris is always a network client.
2. **No DB types in core.** `ephemeris-core` must never depend on `tokio-postgres`, `arangodb`, `couch_rs`, or any database client crate.
3. **Network-boundary isolation.** All database communication goes over TCP/HTTP. Connector crates use network client libraries only.
4. **Feature-gated enterprise connectors.** `ephemeris-arango` and `ephemeris-couch` are behind `enterprise-arango` and `enterprise-couch` Cargo features. Default builds exclude them entirely.
5. **No viral-licensed dependencies in core path.** Every dependency in the default build must be MIT, Apache-2.0, or BSD. Enforced via `cargo-deny` in CI.
6. **BYOD model.** Saturnis provides the pipeline; the end-user provisions and licenses enterprise databases. Legal liability for running restrictively-licensed DB software is on the customer, not Saturnis.

## 4. Architecture: Approach A (Trait-Based Monolith)

Single Cargo workspace, multiple crates. Compile-time feature flags gate enterprise code. The same workspace can produce a single combined binary OR separate ingest/server binaries for elastic scaling.

### Workspace Structure

```
ephemeris/
├── Cargo.toml                    # Workspace root
├── ephemeris.toml                # Example config
├── crates/
│   ├── ephemeris-core/           # Domain types, async traits, validation
│   ├── ephemeris-mqtt/           # MQTT subscriber (rumqttc)
│   ├── ephemeris-pg/             # PostgreSQL connector (tokio-postgres, Tier 1)
│   ├── ephemeris-arango/         # ArangoDB connector (reqwest, Enterprise)
│   ├── ephemeris-couch/          # CouchDB connector (reqwest, Enterprise)
│   ├── ephemeris-api/            # REST + EPCIS 2.0 Query Interface (axum)
│   ├── ephemeris-app/            # Binary entrypoint, config, DI wiring
│   └── ephemeris-testkit/        # Dev-only: test publisher, event generator, test dashboard UI
├── docs/
├── tasks/
└── tests/
    ├── fixtures/                 # Sample EPCIS 2.0 JSON-LD payloads
    ├── integration/              # Per-connector tests with testcontainers
    └── harness/                  # Reusable test utilities
```

### Crate Dependency Rules

- `ephemeris-core`: Zero external DB deps. Defines traits and domain types only.
- `ephemeris-pg`, `ephemeris-arango`, `ephemeris-couch`: Implement core traits. Depend on core + their network client library.
- `ephemeris-mqtt`: Depends on core + rumqttc (pure Rust, no C deps).
- `ephemeris-api`: Depends on core + axum/tower.
- `ephemeris-app`: Depends on all crates. Wires together at startup based on config.
- `ephemeris-testkit`: Dev-dependency only. Never shipped in production builds.

## 5. Repository Abstraction (Core Traits)

```rust
#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn store_event(&self, event: &EpcisEvent) -> Result<EventId, RepoError>;
    async fn get_event(&self, id: &EventId) -> Result<Option<EpcisEvent>, RepoError>;
    async fn query_events(&self, query: &EventQuery) -> Result<Vec<EpcisEvent>, RepoError>;
}

#[async_trait]
pub trait AggregationRepository: Send + Sync {
    async fn add_child(&self, parent: &Epc, child: &Epc, event_id: &EventId) -> Result<(), RepoError>;
    async fn get_children(&self, parent: &Epc) -> Result<Vec<Epc>, RepoError>;
    async fn get_ancestors(&self, child: &Epc) -> Result<Vec<Epc>, RepoError>;
    async fn get_full_hierarchy(&self, root: &Epc) -> Result<AggregationTree, RepoError>;
}
```

The `ephemeris-app` binary selects implementations at startup:

```rust
match config.database.backend {
    DbBackend::Postgres => {
        let event_repo = PgEventRepository::new(&config.database.postgres).await?;
        let agg_repo = PgAggregationRepository::new(&config.database.postgres).await?;
        run_app(event_repo, agg_repo, config).await
    }
    #[cfg(feature = "enterprise-arango")]
    DbBackend::ArangoDB => { /* ... */ }
}
```

## 6. Tiered Database Architecture

### Tier 1: PostgreSQL (Open Core)

- **Events:** Raw EPCIS 2.0 JSON-LD stored in indexed JSONB columns.
- **Aggregation:** Parent-child hierarchy via PostgreSQL `ltree` extension. Massive query acceleration over recursive JOINs.
- **License:** Purely permissive (PostgreSQL License).

### Tier 2: Enterprise Connectors (Feature-Gated)

- **ArangoDB (Graph Engine):** Routes aggregation data for instantaneous traversal of complex packaging hierarchies. Connected via HTTP REST API.
- **CouchDB (Document Engine):** Raw EPCIS event ledger with master-to-master replication for edge-to-cloud sync. Connected via HTTP REST API.
- **BYOD:** Customer provisions and licenses these databases.

## 7. Data Flow & CQRS

### Write Path (Ingest)

```
Scanner/PLC → MQTT Broker → ephemeris-mqtt → Validate → EventRepository::store_event()
                                                      → AggregationRepository::add_child()
```

- MQTT QoS 1 (at-least-once). `store_event()` is idempotent (duplicate detection).
- Invalid events → dead-letter topic.
- No in-memory queue beyond MQTT client buffer. The broker IS the queue.

### Read Path (Query)

```
Client → ephemeris-api (Axum) → EventRepository::query_events()
                               → AggregationRepository::get_full_hierarchy()
```

- REST API + EPCIS 2.0 Query Interface.
- Aggregation queries traverse the hierarchy.

### CQRS Separation

- **Tier 1 (PG):** Logical separation — separate tables/indexes, same database.
- **Tier 2 (Enterprise):** Physical separation — events in CouchDB, aggregation in ArangoDB.

## 8. Edge Ingestion (MQTT Unified Namespace)

- Scanners, PLCs, vision systems publish JSON to MQTT topics.
- Ephemeris subscribes and processes at its own pace.
- Topic pattern: `plant/{site}/events/{eventType}`
- MQTT 5.0 shared subscriptions enable horizontal scaling of ingest instances.

## 9. Deployment Modes

| Mode | Binary | When |
|---|---|---|
| Single-site | `ephemeris-app` (combined ingest + API) | Small plants, edge, simplicity |
| Scaled | Separate `ephemeris-ingest` + `ephemeris-server` | Elastic scale, independent scaling |

Both modes from the same workspace. Customer's choice at deploy time.

### Deployment Artifacts (CI produces all)

- Windows: MSI installer (cargo-wix), runs as Windows Service (windows-service crate)
- Linux: Static binary (musl), systemd unit, .deb/.rpm packages
- Docker: Multi-stage build, scratch/alpine final image (5-25 MB)

## 10. Configuration

### `ephemeris.toml`

```toml
[mqtt]
broker_url = "mqtt://localhost:1883"
client_id = "ephemeris-site-01"
topics = ["plant/+/events/#"]
qos = 1

[database]
backend = "postgres"

[database.postgres]
url = "postgresql://user:pass@localhost:5432/ephemeris"
pool_size = 10

[database.arango]
url = "http://localhost:8529"
database = "ephemeris"

[database.couch]
url = "http://localhost:5984"
database = "ephemeris_events"

[api]
bind = "0.0.0.0:8080"

[license]
key = ""
```

### Override Hierarchy (lowest → highest priority)

1. `ephemeris.toml` (file)
2. Environment variables (`EPHEMERIS_MQTT__BROKER_URL`)
3. CLI flags (`--mqtt-broker-url`)

## 11. Testing Strategy

### Three Layers

| Layer | What | How |
|---|---|---|
| Unit | Core logic, validation, domain types | Mock traits via `mockall` |
| Integration | Each connector vs real DB | `testcontainers-rs` (Docker) |
| E2E | Full pipeline: MQTT → ingest → DB → API | Docker Compose |

### Test Harness (`ephemeris-testkit`)

- **MQTT test publisher:** Reads fixtures or generates synthetic EPCIS events, publishes at configurable rates.
- **Event generator:** Produces random valid EPCIS events for load testing.
- **Test Dashboard UI:** Simple web page for manually composing/sending events, viewing live event feed. Dev-only, never shipped.
- **Assertion helpers:** Reusable verification functions for integration tests.

### CI Pipeline

Every PR: `cargo fmt` → `cargo clippy` → `cargo deny` (license check) → unit tests → integration tests (testcontainers) → E2E pipeline test.

## 12. Open-Core vs Enterprise Split (Feature Flags)

```toml
# Cargo.toml (workspace root)
[features]
default = []
enterprise-arango = ["ephemeris-arango"]
enterprise-couch = ["ephemeris-couch"]
enterprise = ["enterprise-arango", "enterprise-couch"]
```

- Default build: PostgreSQL only. Zero enterprise code in the binary.
- Enterprise build: `cargo build --features enterprise`
- Runtime: If enterprise backend selected but feature not compiled, startup fails with clear error message directing to licensing.

## 13. MVP Scope

### Delivers

1. `ephemeris-core` — Domain types, repository traits, EPCIS validation
2. `ephemeris-pg` — PostgreSQL implementation (JSONB events, ltree aggregation)
3. `ephemeris-arango` — ArangoDB `AggregationRepository` implementation
4. `ephemeris-mqtt` — MQTT subscriber + event processing
5. `ephemeris-api` — Minimal REST API (events + hierarchy queries)
6. `ephemeris-app` — Config loading, backend selection, wiring
7. `ephemeris-testkit` — Test publisher, generator, dashboard UI
8. Full test suite (unit, integration, E2E)

### Proves

- Repository trait abstraction holds across PostgreSQL and ArangoDB
- MQTT → DB → API pipeline works end-to-end
- Feature flags correctly exclude enterprise code from open builds
- Legal firewall rules enforced in CI

### Defers

- CouchDB connector
- Full EPCIS 2.0 Query Interface
- Windows MSI packaging
- HA clustering, compliance reporting
- License key validation system

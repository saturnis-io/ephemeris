# Serial Number Lifecycle — Design Document

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement the plan created from this design.

**Goal:** Add OPEN-SCS-aligned serial number state tracking to Ephemeris, enabling full lifecycle management from pre-commissioning through release/destruction.

**Architecture:** Service layer pattern — a new `SerialNumberService` in `ephemeris-core` contains business logic (state machine, transition validation), backed by a thin `SerialNumberRepository` trait with a PostgreSQL implementation. Integrates with the existing MQTT handler and REST API.

**Standard Reference:** OPEN-SCS PSS Version 1 (OPC Foundation, 2019). Used as a conceptual guide — shorthand names are canonical, not full URIs.

---

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| State enforcement | Permissive with warnings | Don't block production for integration bugs. Warn on invalid transitions via `tracing::warn!`. |
| State scope | All 12 states | Enum is cheap. Avoids schema migrations later. |
| State driver | Event-driven + REST override | MQTT events auto-update state. REST allows operator corrections. |
| History | Full audit trail | `sn_transitions` table logs every change for GxP compliance. |
| Architecture | Service layer (Approach C) | Business logic in `SerialNumberService`, repos stay thin. |
| Storage format | Shorthand enum names | DB stores `"commissioned"` not `"http://open-scs.org/disp/commissioned"`. URI mapping is internal utility only. |
| SID Class | Optional metadata field | Seeds future Phase 4 validation without cost now. |

---

## 1. Domain Types

**File:** `crates/ephemeris-core/src/domain/serial_number.rs`

### SnState

The 12-state enum from OPEN-SCS PSS §5:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnState {
    Unassigned,
    Unallocated,
    Allocated,
    SnInvalid,
    Encoded,
    LabelSampled,
    LabelScrapped,
    Commissioned,
    Sampled,
    Inactive,
    Destroyed,
    Released,
}
```

Implements `Display` and `FromStr` using the snake_case shorthand (`"unassigned"`, `"commissioned"`, etc.).

Provides internal utility methods:
- `to_disposition_uri() -> &'static str` — maps to OPEN-SCS/GS1 disposition URIs
- `from_disposition_uri(uri: &str) -> Option<Self>` — parses from URIs (for interop)

### BizStep Mapping

A standalone function (not a type — avoids over-engineering):

```rust
/// Map a bizStep string to the target SN state, if the event affects SN state.
/// Returns None for events like packing/unpacking that don't change SN state.
pub fn biz_step_to_target_state(biz_step: &str) -> Option<SnState>
```

Accepts both shorthand (`"commissioning"`) and URI formats (`"urn:epcglobal:cbv:bizstep:commissioning"`).

Mapping table (from OPEN-SCS PSS §5 Table 3):

| bizStep | Target State |
|---|---|
| `provisioning` | Unallocated |
| `sn_returning` | Unassigned |
| `sn_allocating` | Allocated |
| `sn_deallocating` | Unallocated |
| `sn_invalidating` | SnInvalid |
| `sn_encoding` | Encoded |
| `label_sampling` | LabelSampled |
| `label_scrapping` | LabelScrapped |
| `commissioning` | Commissioned |
| `inspecting` | Sampled |
| `shipping` | Released |
| `decommissioning` | Inactive |
| `destroying` | Destroyed |
| `label_inspecting` | *(no state change)* |
| `packing` | *(no state change — aggregation only)* |
| `unpacking` | *(no state change — aggregation only)* |

### Transition Validity

A standalone function:

```rust
/// Check if a state transition is valid per OPEN-SCS state machine.
/// Used for permissive warnings, not enforcement.
pub fn is_valid_transition(from: SnState, to: SnState) -> bool
```

Valid transitions (from OPEN-SCS PSS §5 Figure 4):

| From | Valid Targets |
|---|---|
| Unassigned | Unallocated |
| Unallocated | Unassigned, Allocated, SnInvalid |
| Allocated | Unallocated, Encoded, SnInvalid |
| Encoded | LabelSampled, LabelScrapped, Commissioned |
| Commissioned | Sampled, Inactive, Destroyed, Released |
| *(any provisioned state)* | SnInvalid |

### SerialNumber

The tracked entity:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialNumber {
    pub epc: Epc,
    pub state: SnState,
    pub sid_class: Option<String>,
    pub pool_id: Option<String>,
    pub updated_at: DateTime<FixedOffset>,
    pub created_at: DateTime<FixedOffset>,
}
```

### SnTransition

History record:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnTransition {
    pub epc: Epc,
    pub from_state: SnState,
    pub to_state: SnState,
    pub biz_step: String,
    pub event_id: Option<EventId>,
    pub source: TransitionSource,
    pub timestamp: DateTime<FixedOffset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionSource {
    Mqtt,
    RestApi,
    System,
}
```

### SerialNumberQuery

Query parameters for the API:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SerialNumberQuery {
    pub state: Option<SnState>,
    pub sid_class: Option<String>,
    pub pool_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
```

---

## 2. Repository Trait

**File:** `crates/ephemeris-core/src/repository/serial_number.rs`

```rust
#[trait_variant::make(Send)]
pub trait SerialNumberRepository: Sync {
    /// Upsert a serial number's current state.
    /// Creates the record on first encounter.
    async fn upsert_state(
        &self,
        epc: &Epc,
        state: SnState,
        sid_class: Option<&str>,
        pool_id: Option<&str>,
    ) -> Result<(), RepoError>;

    /// Get current state of a serial number.
    async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError>;

    /// Query serial numbers with filters.
    async fn query(
        &self,
        query: &SerialNumberQuery,
    ) -> Result<Vec<SerialNumber>, RepoError>;

    /// Record a state transition in the audit log.
    async fn record_transition(
        &self,
        transition: &SnTransition,
    ) -> Result<(), RepoError>;

    /// Get transition history for an EPC, newest-first.
    async fn get_history(
        &self,
        epc: &Epc,
        limit: u32,
    ) -> Result<Vec<SnTransition>, RepoError>;
}
```

---

## 3. Service Layer

**File:** `crates/ephemeris-core/src/service/serial_number.rs`

The `SerialNumberService` contains all business logic. Repos stay thin.

```rust
pub struct SerialNumberService<S: SerialNumberRepository> {
    repo: S,
}
```

### Core Method: `process_transition`

```rust
pub async fn process_transition(
    &self,
    epc: &Epc,
    biz_step: &str,
    event_id: Option<&EventId>,
    source: TransitionSource,
) -> Result<Option<SnState>, RepoError>
```

Flow:
1. Call `biz_step_to_target_state(biz_step)` — if `None`, return `Ok(None)` (event doesn't affect SN state)
2. Call `self.repo.get_state(epc)` — get current state, default to `Unassigned` if not tracked yet
3. Call `is_valid_transition(current, target)` — if invalid, emit `tracing::warn!("invalid SN transition for {epc}: {current} -> {target} via {biz_step}")` but continue
4. Call `self.repo.upsert_state(epc, target, ...)` — update current state
5. Call `self.repo.record_transition(...)` — log the audit entry
6. Return `Ok(Some(target))`

### Convenience Methods

- `get_state(epc)` — delegates to repo
- `get_history(epc, limit)` — delegates to repo
- `query(query)` — delegates to repo
- `manual_override(epc, target_state, reason)` — for REST API operator overrides, logs with `TransitionSource::RestApi`

---

## 4. PostgreSQL Implementation

**File:** `crates/ephemeris-pg/src/serial_number_repo.rs`

### Schema

```sql
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
```

### Implementation

```rust
#[derive(Clone)]
pub struct PgSerialNumberRepository {
    pool: Pool,
}
```

`upsert_state` uses `INSERT ... ON CONFLICT (epc) DO UPDATE SET state = $2, updated_at = now()`.

States stored as shorthand strings: `"commissioned"`, `"encoded"`, etc.

---

## 5. Integration Points

### MQTT EventHandler

**File:** `crates/ephemeris-mqtt/src/handler.rs`

`EventHandler<E, A>` becomes `EventHandler<E, A, S>` where `S: SerialNumberRepository`.

Gains a `sn_service: SerialNumberService<S>` field.

Updated `handle_event()` flow:
1. Store event (existing)
2. Route aggregation (existing)
3. **New:** Extract `biz_step` from `event.common().biz_step`
4. For each EPC in the event (epc_list for ObjectEvent, parent + children for AggregationEvent):
   - Call `sn_service.process_transition(epc, biz_step, Some(event_id), TransitionSource::Mqtt)`

### REST API Routes

**File:** `crates/ephemeris-api/src/routes/serial_numbers.rs`

New routes:

| Method | Path | Handler |
|---|---|---|
| GET | `/serial-numbers/{epc}` | Get current SN state + metadata |
| GET | `/serial-numbers/{epc}/history` | Get transition audit trail |
| GET | `/serial-numbers` | Query SNs by state/sid_class/pool |
| POST | `/serial-numbers/{epc}/transition` | Manual state override |

POST body:
```json
{
    "target_state": "destroyed",
    "reason": "Operator override"
}
```

### AppState

`AppState<E, A>` becomes `AppState<E, A, S>` with `sn_service: SerialNumberService<S>`.

### App Wiring (main.rs)

Construct `PgSerialNumberRepository` from the same PG pool, wrap in `SerialNumberService`, pass to `EventHandler` and `AppState`.

---

## 6. Testing Strategy

### Unit Tests (ephemeris-core)
- `biz_step_to_target_state` — verify all 17 bizStep mappings
- `is_valid_transition` — verify valid/invalid transition matrix
- `SnState` Display/FromStr roundtrips
- `SerialNumberService::process_transition` — mock repo, verify:
  - Valid transition: state updated + history logged
  - Invalid transition: warning emitted + state still updated (permissive)
  - Unknown bizStep: no state change, no error
  - First-seen EPC: defaults to Unassigned, transitions from there

### Integration Tests (ephemeris-pg)
- `PgSerialNumberRepository` against testcontainers Postgres:
  - upsert_state creates and updates correctly
  - query_by_state returns correct results
  - record_transition + get_history returns ordered results
  - get_state returns None for unknown EPC

### API Tests (ephemeris-api)
- Stub repos, test new routes with tower::oneshot:
  - GET /serial-numbers/{epc} — 200 with state, 404 for unknown
  - GET /serial-numbers?state=commissioned — filtered results
  - GET /serial-numbers/{epc}/history — ordered transitions
  - POST /serial-numbers/{epc}/transition — 200, state updated

### Handler Tests (ephemeris-mqtt)
- Mock all three repos, verify:
  - ObjectEvent with bizStep=commissioning → SN transitions to Commissioned
  - AggregationEvent with packing → no SN state change
  - Event with unknown bizStep → no SN state change

---

## 7. File Summary

| Action | Path |
|---|---|
| Create | `crates/ephemeris-core/src/domain/serial_number.rs` |
| Create | `crates/ephemeris-core/src/repository/serial_number.rs` |
| Create | `crates/ephemeris-core/src/service/mod.rs` |
| Create | `crates/ephemeris-core/src/service/serial_number.rs` |
| Create | `crates/ephemeris-pg/src/serial_number_repo.rs` |
| Create | `crates/ephemeris-api/src/routes/serial_numbers.rs` |
| Modify | `crates/ephemeris-core/src/domain/mod.rs` — add serial_number module |
| Modify | `crates/ephemeris-core/src/repository/mod.rs` — add serial_number module |
| Modify | `crates/ephemeris-core/src/lib.rs` — add service module |
| Modify | `crates/ephemeris-mqtt/src/handler.rs` — add S generic + SN service |
| Modify | `crates/ephemeris-api/src/lib.rs` — add SN routes + update AppState |
| Modify | `crates/ephemeris-api/src/state.rs` — add S generic |
| Modify | `crates/ephemeris-pg/src/lib.rs` — add serial_number_repo module |
| Modify | `crates/ephemeris-pg/src/schema.rs` (or migration) — add SN tables |
| Modify | `crates/ephemeris-app/src/main.rs` — wire SN repo + service |
| Modify | `crates/ephemeris-app/src/config.rs` — no changes needed (PG pool shared) |

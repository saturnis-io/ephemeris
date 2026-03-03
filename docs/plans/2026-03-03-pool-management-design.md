# Pool Management Design — Phase 2 OPEN-SCS Alignment

**Goal:** Add three-tier serial number pool management (ESM → SSM → LSM) following OPEN-SCS PSS §6.4 and §7.2-7.6.

**Architecture:** `PoolService<P, C>` in `ephemeris-core` wrapping a `PoolRepository` trait + `EsmClient` trait. Dual backend implementation (PostgreSQL + ArangoDB). Upstream ESM communication via reqwest REST client.

---

## 1. Domain Types

### PoolCriterionKey (hybrid enum)

Typed keys for the 10 OPEN-SCS pool selection criteria (PSS §6.9), plus extensibility:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolCriterionKey {
    Gtin,           // GS1 Global Trade Item Number (14 digits, left-padded)
    SsccGcp,        // GS1 Company Prefix for SSCCs (4-12 digits)
    SsccExtension,  // SSCC extension digit (0-9)
    CountryCode,    // ISO 3166-1 (DE, FR, US)
    Location,       // GS1 GLN or facility ID
    Sublocation,    // Line/equipment ID
    LotNumber,      // Production lot/batch
    PoolId,         // Direct pool reference
    SidClassId,     // SID Class defining format
    OrderId,        // Production order
    Custom(String), // Extensibility beyond OPEN-SCS
}
```

### PoolSelectionCriteria

Typed wrapper around a list of key-value pairs:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolSelectionCriteria {
    pub criteria: Vec<(PoolCriterionKey, String)>,
}
```

### SerialNumberPool

Lightweight pool entity — primarily a named set of selection criteria:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialNumberPool {
    pub id: PoolId,                        // UUID
    pub name: String,                      // Human-readable
    pub sid_class: Option<String>,         // Associated SID class
    pub criteria: PoolSelectionCriteria,   // What SNs belong to this pool
    pub esm_endpoint: Option<String>,      // Upstream ESM URL (if pull-enabled)
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}
```

### Transaction Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolRequest {
    pub count: u32,
    pub criteria: PoolSelectionCriteria,
    pub output_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolResponse {
    pub serial_numbers: Vec<Epc>,
    pub pool_id: PoolId,
    pub fulfilled: u32,
    pub requested: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolReturnRequest {
    pub serial_numbers: Vec<Epc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub pool_id: PoolId,
    pub total: u64,
    pub unassigned: u64,
    pub unallocated: u64,
    pub allocated: u64,
    pub encoded: u64,
    pub commissioned: u64,
    pub other: u64,
}
```

### Linkage

`SerialNumber.pool_id` (already in schema) links SNs to pools. Pool operations are state transitions on those SNs — request moves `Unallocated→Allocated`, return moves `Allocated→Unallocated`.

---

## 2. Repository & Service Layer

### PoolRepository trait (ephemeris-core)

```rust
pub trait PoolRepository: Send + Sync {
    async fn create_pool(&self, pool: &SerialNumberPool) -> Result<PoolId, RepoError>;
    async fn get_pool(&self, id: &PoolId) -> Result<Option<SerialNumberPool>, RepoError>;
    async fn list_pools(&self, filter: &PoolQuery) -> Result<Vec<SerialNumberPool>, RepoError>;
    async fn assign_to_pool(&self, pool_id: &PoolId, epcs: &[Epc]) -> Result<u32, RepoError>;
    async fn request_numbers(
        &self, pool_id: &PoolId, count: u32, criteria: &PoolSelectionCriteria,
    ) -> Result<Vec<Epc>, RepoError>;
    async fn return_numbers(&self, pool_id: &PoolId, epcs: &[Epc]) -> Result<u32, RepoError>;
    async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError>;
}
```

### EsmClient trait (ephemeris-core)

```rust
pub trait EsmClient: Send + Sync {
    async fn request_unassigned(
        &self, count: u32, criteria: &PoolSelectionCriteria,
    ) -> Result<Vec<Epc>, EsmError>;
    async fn return_unallocated(&self, epcs: &[Epc]) -> Result<u32, EsmError>;
}
```

### PoolService (ephemeris-core)

`PoolService<P: PoolRepository, C: EsmClient>` orchestrates:

- **Request Unassigned**: calls `esm_client.request_unassigned()` → stores SNs → assigns to pool
- **Request Unallocated**: calls `pool_repo.request_numbers()` → returns allocated SNs
- **Request Allocated**: returns already-allocated SNs ready for encoding
- **Return**: calls `pool_repo.return_numbers()` → optionally pushes back to ESM

### Implementation split

| Component | ephemeris-pg | ephemeris-arango |
|-----------|-------------|-----------------|
| `PoolRepository` | Relational tables + SQL | Document collections + graph edges + AQL |
| `EsmClient` | reqwest HTTP client | Reuses same reqwest impl |

---

## 3. PostgreSQL Schema

```sql
CREATE TABLE sn_pools (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    sid_class   TEXT,
    esm_endpoint TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_pools_sid_class ON sn_pools(sid_class);

CREATE TABLE pool_criteria (
    pool_id     UUID NOT NULL REFERENCES sn_pools(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (pool_id, key, value)
);

CREATE INDEX idx_pool_criteria_key ON pool_criteria(key, value);
```

Pool operations use the existing `serial_numbers` table via `pool_id`:

- **Request**: `SELECT ... WHERE pool_id = $1 AND state = 'unallocated' FOR UPDATE SKIP LOCKED LIMIT $2` then `UPDATE SET state = 'allocated'`
- **Return**: `UPDATE serial_numbers SET state = 'unallocated' WHERE epc = ANY($1) AND pool_id = $2`
- **Stats**: `SELECT state, COUNT(*) FROM serial_numbers WHERE pool_id = $1 GROUP BY state`

---

## 4. ArangoDB Schema

```
Document collections:
  sn_pools         — { _key: uuid, name, sid_class, esm_endpoint, criteria, timestamps }

Edge collections:
  pool_contains    — pool → serial_number edges
```

Graph advantages:
- Pool→SN relationships as edges enable "which pools share SNs?" queries
- Criteria matching via AQL graph traversal
- Pool stats via graph aggregation: `FOR v IN 1..1 OUTBOUND pool GRAPH 'pool_graph' COLLECT state = v.state WITH COUNT INTO c`

---

## 5. REST API Endpoints

```
POST   /pools                       — Create a pool
GET    /pools                       — List pools (?sid_class=, ?location= filters)
GET    /pools/{id}                  — Get pool details + stats
DELETE /pools/{id}                  — Delete an empty pool

POST   /pools/{id}/request          — Request SNs (Unallocated → Allocated)
POST   /pools/{id}/return           — Return SNs (Allocated → Unallocated)
POST   /pools/{id}/receive          — Receive SNs into pool (ESM push or manual import)

POST   /pools/{id}/request-upstream — Request new SNs from upstream ESM
POST   /pools/{id}/return-upstream  — Return SNs back to upstream ESM
```

### OPEN-SCS mapping

| OPEN-SCS Function | REST Endpoint | State Transition |
|---|---|---|
| Request Unassigned (§7.2) | `POST /pools/{id}/request-upstream` | ESM: Unassigned → SSM: Unallocated |
| Request Unallocated (§7.3) | `POST /pools/{id}/request` | Unallocated → Allocated |
| Request Allocated (§7.4) | `GET /serial-numbers?poolId={id}&state=allocated` | Read-only |
| Return Unallocated (§7.5) | `POST /pools/{id}/return-upstream` | SSM: Unallocated → ESM |
| Return Allocated (§7.6) | `POST /pools/{id}/return` | Allocated → Unallocated |

### Request/response examples

```json
// POST /pools
{
  "name": "GTIN-614141 Production Pool",
  "sidClass": "sgtin",
  "criteria": [["gtin", "06141410123456"]],
  "esmEndpoint": "https://esm.corp.example.com/api/v1"
}

// POST /pools/{id}/request
{ "count": 100, "criteria": [["lot_number", "LOT-2026-A"]] }
// Response
{ "poolId": "uuid", "serialNumbers": ["urn:epc:id:sgtin:..."], "fulfilled": 100, "requested": 100 }

// POST /pools/{id}/return
{ "serialNumbers": ["urn:epc:id:sgtin:...", "urn:epc:id:sgtin:..."] }
// Response
{ "poolId": "uuid", "returned": 2 }

// POST /pools/{id}/receive
{ "serialNumbers": ["urn:epc:id:sgtin:...", ...], "sidClass": "sgtin", "initialState": "unallocated" }
// Response
{ "poolId": "uuid", "received": 1000 }
```

---

## 6. ESM Client Configuration

```toml
[esm]
url = "https://esm.corp.example.com/api/v1"
api_key = "sk-..."
timeout_secs = 30
```

Optional — if no `[esm]` section, upstream endpoints return 503 ("no ESM configured"). Local pool operations (receive, request, return) still work.

reqwest implementation lives in `ephemeris-pg` crate alongside the PG pool repository (same open-core tier). Trait defined in `ephemeris-core` (no HTTP client dependency in core).

---

## 7. Error Handling

- **Insufficient SNs**: Returns what's available, `fulfilled < requested`. No error.
- **Return wrong pool**: 400 — SN doesn't belong to this pool.
- **Return wrong state**: 400 — SN isn't `Allocated`, includes current state in error.
- **ESM unavailable**: 502 — upstream request failed, local operations unaffected.
- **Concurrent requests**: PG uses `SELECT ... FOR UPDATE SKIP LOCKED`. Arango uses transaction isolation.
- **Empty pool**: Returns `fulfilled: 0`, no error.

---

## 8. Testing Strategy

- **Unit tests**: Domain type serde, criteria matching, service orchestration with mock repos
- **Integration tests**: PG + Arango pool repos against testcontainers
- **API tests**: Stub repos (same pattern as SN lifecycle tests in ephemeris-api)
- **E2E**: Extend `test-app.ps1` with pool menu options
- **ESM client**: Mock HTTP server for upstream flow testing

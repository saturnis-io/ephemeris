# Pool Management Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add three-tier serial number pool management (ESM → SSM → LSM) with CRUD operations, request/return flows, and upstream ESM communication, following OPEN-SCS PSS §6.4 and §7.2-7.6.

**Architecture:** `PoolService<P, S>` in `ephemeris-core` wrapping a `PoolRepository` trait + `EsmClient` trait. PostgreSQL implementation with `sn_pools` + `pool_criteria` tables. REST API with 9 endpoints. Upstream ESM communication via reqwest HTTP client (trait in core, impl in ephemeris-pg). Follows the existing pattern: domain types → repository trait → PG impl → service → API routes → app wiring.

**Tech Stack:** Rust, axum, tokio-postgres, deadpool-postgres, reqwest, chrono, uuid, serde, trait-variant

**Design doc:** `docs/plans/2026-03-03-pool-management-design.md`

---

## Task 1: Pool Domain Types

Add `PoolId`, `PoolCriterionKey`, `PoolSelectionCriteria`, `SerialNumberPool`, and transaction types to `ephemeris-core`.

**Files:**
- Create: `crates/ephemeris-core/src/domain/pool.rs`
- Modify: `crates/ephemeris-core/src/domain/mod.rs`

**Step 1: Write the failing test**

Add a new file `crates/ephemeris-core/src/domain/pool.rs` with tests at the bottom. Start with just the test module — the types will be above it:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_id_display_and_new() {
        let id = PoolId::new();
        let s = id.to_string();
        // UUID format: 8-4-4-4-12 hex chars
        assert_eq!(s.len(), 36);
        assert_eq!(id, id.clone());
    }

    #[test]
    fn test_pool_criterion_key_serde_roundtrip() {
        let keys = vec![
            PoolCriterionKey::Gtin,
            PoolCriterionKey::SsccGcp,
            PoolCriterionKey::CountryCode,
            PoolCriterionKey::Custom("my_custom_key".to_string()),
        ];
        for key in keys {
            let json = serde_json::to_string(&key).unwrap();
            let back: PoolCriterionKey = serde_json::from_str(&json).unwrap();
            assert_eq!(back, key);
        }
    }

    #[test]
    fn test_pool_criterion_key_snake_case_serialization() {
        assert_eq!(
            serde_json::to_string(&PoolCriterionKey::SsccGcp).unwrap(),
            "\"sscc_gcp\""
        );
        assert_eq!(
            serde_json::to_string(&PoolCriterionKey::CountryCode).unwrap(),
            "\"country_code\""
        );
        assert_eq!(
            serde_json::to_string(&PoolCriterionKey::SidClassId).unwrap(),
            "\"sid_class_id\""
        );
    }

    #[test]
    fn test_pool_selection_criteria_default_is_empty() {
        let c = PoolSelectionCriteria::default();
        assert!(c.criteria.is_empty());
    }

    #[test]
    fn test_serial_number_pool_serde_roundtrip() {
        let pool = SerialNumberPool {
            id: PoolId::new(),
            name: "Test Pool".to_string(),
            sid_class: Some("sgtin".to_string()),
            criteria: PoolSelectionCriteria {
                criteria: vec![
                    (PoolCriterionKey::Gtin, "06141410123456".to_string()),
                ],
            },
            esm_endpoint: None,
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        };
        let json = serde_json::to_string(&pool).unwrap();
        let back: SerialNumberPool = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, pool.id);
        assert_eq!(back.name, pool.name);
        assert_eq!(back.sid_class, pool.sid_class);
    }

    #[test]
    fn test_pool_request_serde() {
        let json = r#"{"count":100,"criteria":{"criteria":[["gtin","06141410123456"]]}}"#;
        let req: PoolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.count, 100);
        assert_eq!(req.criteria.criteria.len(), 1);
    }

    #[test]
    fn test_pool_stats_default_zeros() {
        let stats = PoolStats {
            pool_id: PoolId::new(),
            total: 0,
            unassigned: 0,
            unallocated: 0,
            allocated: 0,
            encoded: 0,
            commissioned: 0,
            other: 0,
        };
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn test_pool_query_default() {
        let q = PoolQuery::default();
        assert!(q.sid_class.is_none());
        assert!(q.name_contains.is_none());
        assert_eq!(q.limit, None);
        assert_eq!(q.offset, None);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ephemeris-core pool::tests --no-run 2>&1`
Expected: Compilation fails — types don't exist yet.

**Step 3: Write the domain types above the tests**

Full `crates/ephemeris-core/src/domain/pool.rs`:

```rust
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::epc::Epc;

/// Unique identifier for a serial number pool.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PoolId(pub Uuid);

impl PoolId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PoolId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for PoolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Typed keys for the 10 OPEN-SCS pool selection criteria (PSS §6.9),
/// plus extensibility via Custom.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolCriterionKey {
    Gtin,
    SsccGcp,
    SsccExtension,
    CountryCode,
    Location,
    Sublocation,
    LotNumber,
    PoolId,
    SidClassId,
    OrderId,
    Custom(String),
}

/// Typed wrapper around a list of key-value selection criteria.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolSelectionCriteria {
    pub criteria: Vec<(PoolCriterionKey, String)>,
}

/// A serial number pool — a named set of selection criteria
/// that groups serial numbers for allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialNumberPool {
    pub id: super::pool::PoolId,
    pub name: String,
    pub sid_class: Option<String>,
    pub criteria: PoolSelectionCriteria,
    pub esm_endpoint: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

/// Request to allocate serial numbers from a pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolRequest {
    pub count: u32,
    pub criteria: PoolSelectionCriteria,
    pub output_format: Option<String>,
}

/// Response from a pool allocation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolResponse {
    pub serial_numbers: Vec<Epc>,
    pub pool_id: super::pool::PoolId,
    pub fulfilled: u32,
    pub requested: u32,
}

/// Request to return serial numbers back to a pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolReturnRequest {
    pub serial_numbers: Vec<Epc>,
}

/// Request to receive (import) serial numbers into a pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolReceiveRequest {
    pub serial_numbers: Vec<Epc>,
    pub sid_class: Option<String>,
    pub initial_state: Option<String>,
}

/// Statistics for a serial number pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub pool_id: super::pool::PoolId,
    pub total: u64,
    pub unassigned: u64,
    pub unallocated: u64,
    pub allocated: u64,
    pub encoded: u64,
    pub commissioned: u64,
    pub other: u64,
}

/// Query parameters for listing pools.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolQuery {
    pub sid_class: Option<String>,
    pub name_contains: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// tests go here (from Step 1)
```

**Note:** Replace `super::pool::PoolId` with just `PoolId` — the above is for clarity. The actual type references within the same module use the local name.

**Step 4: Register the module**

In `crates/ephemeris-core/src/domain/mod.rs`, add:

```rust
pub mod pool;
```

and:

```rust
pub use pool::*;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p ephemeris-core pool::tests -- --nocapture`
Expected: All 7 tests PASS.

**Step 6: Commit**

```bash
git add crates/ephemeris-core/src/domain/pool.rs crates/ephemeris-core/src/domain/mod.rs
git commit -m "feat(core): add pool management domain types

PoolId, PoolCriterionKey (10 OPEN-SCS keys + Custom), PoolSelectionCriteria,
SerialNumberPool, PoolRequest, PoolResponse, PoolReturnRequest,
PoolReceiveRequest, PoolStats, PoolQuery."
```

---

## Task 2: PoolRepository Trait

Define the `PoolRepository` trait in `ephemeris-core` following the same pattern as `SerialNumberRepository`.

**Files:**
- Create: `crates/ephemeris-core/src/repository/pool.rs`
- Modify: `crates/ephemeris-core/src/repository/mod.rs`

**Step 1: Write the trait definition**

Create `crates/ephemeris-core/src/repository/pool.rs`:

```rust
use crate::domain::{Epc, PoolId, PoolQuery, PoolSelectionCriteria, PoolStats, SerialNumberPool};
use crate::error::RepoError;

/// Repository for serial number pool management.
///
/// Implementations handle pool CRUD, serial number assignment,
/// and allocation/return operations. Business logic (ESM orchestration,
/// validation) lives in PoolService, not here.
#[trait_variant::make(Send)]
pub trait PoolRepository: Sync {
    /// Create a new pool. Returns the generated pool ID.
    async fn create_pool(&self, pool: &SerialNumberPool) -> Result<PoolId, RepoError>;

    /// Get a pool by ID. Returns None if not found.
    async fn get_pool(&self, id: &PoolId) -> Result<Option<SerialNumberPool>, RepoError>;

    /// List pools with optional filters.
    async fn list_pools(&self, filter: &PoolQuery) -> Result<Vec<SerialNumberPool>, RepoError>;

    /// Delete an empty pool. Returns error if pool has assigned serial numbers.
    async fn delete_pool(&self, id: &PoolId) -> Result<(), RepoError>;

    /// Assign serial numbers to a pool (receive/import).
    /// Sets pool_id on each SN and optionally updates state.
    async fn assign_to_pool(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
        initial_state: Option<&str>,
    ) -> Result<u32, RepoError>;

    /// Request (allocate) serial numbers from a pool.
    /// Moves SNs from Unallocated → Allocated and returns them.
    async fn request_numbers(
        &self,
        pool_id: &PoolId,
        count: u32,
    ) -> Result<Vec<Epc>, RepoError>;

    /// Return (deallocate) serial numbers back to a pool.
    /// Moves SNs from Allocated → Unallocated.
    async fn return_numbers(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
    ) -> Result<u32, RepoError>;

    /// Get pool statistics (counts by SN state).
    async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError>;
}
```

**Step 2: Register the module**

In `crates/ephemeris-core/src/repository/mod.rs`, add:

```rust
pub mod pool;
```

and:

```rust
pub use pool::*;
```

**Step 3: Verify it compiles**

Run: `cargo check -p ephemeris-core`
Expected: Compiles cleanly.

**Step 4: Commit**

```bash
git add crates/ephemeris-core/src/repository/pool.rs crates/ephemeris-core/src/repository/mod.rs
git commit -m "feat(core): add PoolRepository trait

CRUD, assign_to_pool, request_numbers, return_numbers, get_pool_stats.
Follows same pattern as SerialNumberRepository."
```

---

## Task 3: EsmClient Trait

Define the `EsmClient` trait in `ephemeris-core` for upstream ESM communication.

**Files:**
- Create: `crates/ephemeris-core/src/repository/esm.rs`
- Modify: `crates/ephemeris-core/src/repository/mod.rs`
- Modify: `crates/ephemeris-core/src/error.rs`

**Step 1: Add EsmError to error.rs**

In `crates/ephemeris-core/src/error.rs`, add a new error enum:

```rust
/// Errors from upstream ESM communication.
#[derive(Error, Debug)]
pub enum EsmError {
    #[error("ESM not configured")]
    NotConfigured,

    #[error("ESM connection failed: {0}")]
    Connection(String),

    #[error("ESM request failed: {status} {body}")]
    Request { status: u16, body: String },

    #[error("ESM response parse error: {0}")]
    Parse(String),

    #[error("ESM timeout: {0}")]
    Timeout(String),
}
```

**Step 2: Write the EsmClient trait**

Create `crates/ephemeris-core/src/repository/esm.rs`:

```rust
use crate::domain::{Epc, PoolSelectionCriteria};
use crate::error::EsmError;

/// Client for upstream Enterprise Serialization Manager (ESM) communication.
///
/// The ESM sits at ISA-95 Level 4 and manages the global serial number supply.
/// This trait abstracts the HTTP communication so the service layer doesn't
/// depend on reqwest or any HTTP client.
#[trait_variant::make(Send)]
pub trait EsmClient: Sync {
    /// Request unassigned serial numbers from the upstream ESM.
    /// OPEN-SCS PSS §7.2: ESM allocates SNs → SSM stores as Unallocated.
    async fn request_unassigned(
        &self,
        count: u32,
        criteria: &PoolSelectionCriteria,
    ) -> Result<Vec<Epc>, EsmError>;

    /// Return unallocated serial numbers back to the upstream ESM.
    /// OPEN-SCS PSS §7.5: SSM returns unused SNs → ESM marks as Unassigned.
    async fn return_unallocated(&self, epcs: &[Epc]) -> Result<u32, EsmError>;
}
```

**Step 3: Register the module**

In `crates/ephemeris-core/src/repository/mod.rs`, add:

```rust
pub mod esm;
```

and:

```rust
pub use esm::*;
```

**Step 4: Verify it compiles**

Run: `cargo check -p ephemeris-core`
Expected: Compiles cleanly.

**Step 5: Commit**

```bash
git add crates/ephemeris-core/src/error.rs crates/ephemeris-core/src/repository/esm.rs crates/ephemeris-core/src/repository/mod.rs
git commit -m "feat(core): add EsmClient trait and EsmError

Upstream ESM communication abstraction for OPEN-SCS §7.2 (request unassigned)
and §7.5 (return unallocated). No HTTP deps in core."
```

---

## Task 4: PoolService

Implement `PoolService<P, C>` in `ephemeris-core` that orchestrates pool repository and ESM client operations.

**Files:**
- Create: `crates/ephemeris-core/src/service/pool.rs`
- Modify: `crates/ephemeris-core/src/service/mod.rs`

**Step 1: Write failing tests**

Create `crates/ephemeris-core/src/service/pool.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Epc, PoolCriterionKey, PoolId, PoolSelectionCriteria, SerialNumberPool};
    use std::sync::Mutex;

    struct StubPoolRepo {
        pools: Mutex<Vec<SerialNumberPool>>,
        allocated: Mutex<Vec<Epc>>,
    }

    impl StubPoolRepo {
        fn empty() -> Self {
            Self {
                pools: Mutex::new(Vec::new()),
                allocated: Mutex::new(Vec::new()),
            }
        }

        fn with_pool(pool: SerialNumberPool) -> Self {
            Self {
                pools: Mutex::new(vec![pool]),
                allocated: Mutex::new(Vec::new()),
            }
        }
    }

    impl PoolRepository for StubPoolRepo {
        async fn create_pool(&self, pool: &SerialNumberPool) -> Result<PoolId, RepoError> {
            let id = pool.id.clone();
            self.pools.lock().unwrap().push(pool.clone());
            Ok(id)
        }

        async fn get_pool(&self, id: &PoolId) -> Result<Option<SerialNumberPool>, RepoError> {
            Ok(self.pools.lock().unwrap().iter().find(|p| p.id == *id).cloned())
        }

        async fn list_pools(&self, _filter: &PoolQuery) -> Result<Vec<SerialNumberPool>, RepoError> {
            Ok(self.pools.lock().unwrap().clone())
        }

        async fn delete_pool(&self, id: &PoolId) -> Result<(), RepoError> {
            self.pools.lock().unwrap().retain(|p| p.id != *id);
            Ok(())
        }

        async fn assign_to_pool(
            &self,
            _pool_id: &PoolId,
            epcs: &[Epc],
            _initial_state: Option<&str>,
        ) -> Result<u32, RepoError> {
            Ok(epcs.len() as u32)
        }

        async fn request_numbers(
            &self,
            _pool_id: &PoolId,
            count: u32,
        ) -> Result<Vec<Epc>, RepoError> {
            let mut result = Vec::new();
            for i in 0..count {
                let epc = Epc::new(format!("urn:epc:id:sgtin:0614141.107346.{i:04}"));
                self.allocated.lock().unwrap().push(epc.clone());
                result.push(epc);
            }
            Ok(result)
        }

        async fn return_numbers(
            &self,
            _pool_id: &PoolId,
            epcs: &[Epc],
        ) -> Result<u32, RepoError> {
            Ok(epcs.len() as u32)
        }

        async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError> {
            Ok(PoolStats {
                pool_id: pool_id.clone(),
                total: 0,
                unassigned: 0,
                unallocated: 0,
                allocated: 0,
                encoded: 0,
                commissioned: 0,
                other: 0,
            })
        }
    }

    struct StubEsmClient;

    impl EsmClient for StubEsmClient {
        async fn request_unassigned(
            &self,
            count: u32,
            _criteria: &PoolSelectionCriteria,
        ) -> Result<Vec<Epc>, EsmError> {
            let mut epcs = Vec::new();
            for i in 0..count {
                epcs.push(Epc::new(format!("urn:epc:id:sgtin:ESM.{i:04}")));
            }
            Ok(epcs)
        }

        async fn return_unallocated(&self, epcs: &[Epc]) -> Result<u32, EsmError> {
            Ok(epcs.len() as u32)
        }
    }

    fn make_pool() -> SerialNumberPool {
        SerialNumberPool {
            id: PoolId::new(),
            name: "Test Pool".to_string(),
            sid_class: Some("sgtin".to_string()),
            criteria: PoolSelectionCriteria {
                criteria: vec![(PoolCriterionKey::Gtin, "06141410123456".to_string())],
            },
            esm_endpoint: Some("https://esm.example.com/api/v1".to_string()),
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[tokio::test]
    async fn test_create_pool() {
        let pool = make_pool();
        let service = PoolService::new(StubPoolRepo::empty(), StubEsmClient);
        let id = service.create_pool(&pool).await.unwrap();
        assert_eq!(id, pool.id);
    }

    #[tokio::test]
    async fn test_get_pool_found() {
        let pool = make_pool();
        let id = pool.id.clone();
        let service = PoolService::new(StubPoolRepo::with_pool(pool), StubEsmClient);
        let result = service.get_pool(&id).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_get_pool_not_found() {
        let service = PoolService::new(StubPoolRepo::empty(), StubEsmClient);
        let result = service.get_pool(&PoolId::new()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_request_numbers() {
        let pool = make_pool();
        let id = pool.id.clone();
        let service = PoolService::new(StubPoolRepo::with_pool(pool), StubEsmClient);
        let response = service.request_numbers(&id, 5).await.unwrap();
        assert_eq!(response.fulfilled, 5);
        assert_eq!(response.requested, 5);
        assert_eq!(response.serial_numbers.len(), 5);
    }

    #[tokio::test]
    async fn test_return_numbers() {
        let pool = make_pool();
        let id = pool.id.clone();
        let epcs = vec![
            Epc::new("urn:epc:id:sgtin:0614141.107346.0001"),
            Epc::new("urn:epc:id:sgtin:0614141.107346.0002"),
        ];
        let service = PoolService::new(StubPoolRepo::with_pool(pool), StubEsmClient);
        let returned = service.return_numbers(&id, &epcs).await.unwrap();
        assert_eq!(returned, 2);
    }

    #[tokio::test]
    async fn test_receive_numbers() {
        let pool = make_pool();
        let id = pool.id.clone();
        let epcs = vec![
            Epc::new("urn:epc:id:sgtin:0614141.107346.0001"),
            Epc::new("urn:epc:id:sgtin:0614141.107346.0002"),
            Epc::new("urn:epc:id:sgtin:0614141.107346.0003"),
        ];
        let service = PoolService::new(StubPoolRepo::with_pool(pool), StubEsmClient);
        let received = service
            .receive_numbers(&id, &epcs, None, None)
            .await
            .unwrap();
        assert_eq!(received, 3);
    }

    #[tokio::test]
    async fn test_request_upstream() {
        let pool = make_pool();
        let id = pool.id.clone();
        let criteria = PoolSelectionCriteria::default();
        let service = PoolService::new(StubPoolRepo::with_pool(pool), StubEsmClient);
        let result = service.request_upstream(&id, 10, &criteria).await.unwrap();
        assert_eq!(result.fulfilled, 10);
    }

    #[tokio::test]
    async fn test_return_upstream() {
        let pool = make_pool();
        let id = pool.id.clone();
        let epcs = vec![Epc::new("urn:epc:id:sgtin:0614141.107346.0001")];
        let service = PoolService::new(StubPoolRepo::with_pool(pool), StubEsmClient);
        let returned = service.return_upstream(&id, &epcs).await.unwrap();
        assert_eq!(returned, 1);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ephemeris-core pool::tests --no-run 2>&1`
Expected: Compilation fails — `PoolService` doesn't exist.

**Step 3: Write the PoolService implementation above the tests**

```rust
use crate::domain::{
    Epc, PoolId, PoolQuery, PoolResponse, PoolSelectionCriteria, PoolStats, SerialNumberPool,
};
use crate::error::{EsmError, RepoError};
use crate::repository::{EsmClient, PoolRepository};

/// Service layer for serial number pool management.
///
/// Orchestrates pool CRUD, local allocation/return, and upstream ESM
/// communication. Delegates storage to PoolRepository and upstream
/// calls to EsmClient.
pub struct PoolService<P: PoolRepository, C: EsmClient> {
    pool_repo: P,
    esm_client: C,
}

impl<P: PoolRepository, C: EsmClient> PoolService<P, C> {
    pub fn new(pool_repo: P, esm_client: C) -> Self {
        Self {
            pool_repo,
            esm_client,
        }
    }

    /// Create a new pool.
    pub async fn create_pool(&self, pool: &SerialNumberPool) -> Result<PoolId, RepoError> {
        self.pool_repo.create_pool(pool).await
    }

    /// Get a pool by ID.
    pub async fn get_pool(
        &self,
        id: &PoolId,
    ) -> Result<Option<SerialNumberPool>, RepoError> {
        self.pool_repo.get_pool(id).await
    }

    /// List pools with optional filters.
    pub async fn list_pools(
        &self,
        filter: &PoolQuery,
    ) -> Result<Vec<SerialNumberPool>, RepoError> {
        self.pool_repo.list_pools(filter).await
    }

    /// Delete an empty pool.
    pub async fn delete_pool(&self, id: &PoolId) -> Result<(), RepoError> {
        self.pool_repo.delete_pool(id).await
    }

    /// Request (allocate) serial numbers from a pool.
    /// OPEN-SCS §7.3: Unallocated → Allocated.
    pub async fn request_numbers(
        &self,
        pool_id: &PoolId,
        count: u32,
    ) -> Result<PoolResponse, RepoError> {
        let epcs = self.pool_repo.request_numbers(pool_id, count).await?;
        let fulfilled = epcs.len() as u32;
        Ok(PoolResponse {
            serial_numbers: epcs,
            pool_id: pool_id.clone(),
            fulfilled,
            requested: count,
        })
    }

    /// Return (deallocate) serial numbers back to a pool.
    /// OPEN-SCS §7.6: Allocated → Unallocated.
    pub async fn return_numbers(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
    ) -> Result<u32, RepoError> {
        self.pool_repo.return_numbers(pool_id, epcs).await
    }

    /// Receive (import) serial numbers into a pool.
    /// Used for ESM push or manual import.
    pub async fn receive_numbers(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
        _sid_class: Option<&str>,
        initial_state: Option<&str>,
    ) -> Result<u32, RepoError> {
        self.pool_repo
            .assign_to_pool(pool_id, epcs, initial_state)
            .await
    }

    /// Get pool statistics.
    pub async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError> {
        self.pool_repo.get_pool_stats(pool_id).await
    }

    /// Request new serial numbers from the upstream ESM.
    /// OPEN-SCS §7.2: ESM → SSM. Receives SNs and assigns to pool.
    pub async fn request_upstream(
        &self,
        pool_id: &PoolId,
        count: u32,
        criteria: &PoolSelectionCriteria,
    ) -> Result<PoolResponse, EsmError> {
        let epcs = self
            .esm_client
            .request_unassigned(count, criteria)
            .await?;
        let fulfilled = epcs.len() as u32;

        // Store received SNs in pool
        self.pool_repo
            .assign_to_pool(pool_id, &epcs, Some("unallocated"))
            .await
            .map_err(|e| EsmError::Connection(format!("failed to store ESM SNs: {e}")))?;

        Ok(PoolResponse {
            serial_numbers: epcs,
            pool_id: pool_id.clone(),
            fulfilled,
            requested: count,
        })
    }

    /// Return unallocated serial numbers back to the upstream ESM.
    /// OPEN-SCS §7.5: SSM → ESM.
    pub async fn return_upstream(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
    ) -> Result<u32, EsmError> {
        let returned = self.esm_client.return_unallocated(epcs).await?;

        // Remove SNs from pool after successful upstream return
        self.pool_repo
            .return_numbers(pool_id, epcs)
            .await
            .map_err(|e| EsmError::Connection(format!("failed to update pool after ESM return: {e}")))?;

        Ok(returned)
    }
}
```

**Step 4: Register the module**

In `crates/ephemeris-core/src/service/mod.rs`, add:

```rust
pub mod pool;

pub use pool::PoolService;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p ephemeris-core service::pool::tests -- --nocapture`
Expected: All 8 tests PASS.

**Step 6: Commit**

```bash
git add crates/ephemeris-core/src/service/pool.rs crates/ephemeris-core/src/service/mod.rs
git commit -m "feat(core): add PoolService for pool orchestration

Wraps PoolRepository + EsmClient. Local request/return (§7.3/§7.6),
upstream request/return (§7.2/§7.5), receive, CRUD, stats."
```

---

## Task 5: PostgreSQL Pool Schema

Add `sn_pools` and `pool_criteria` tables to the PG schema.

**Files:**
- Modify: `crates/ephemeris-pg/src/schema.rs`

**Step 1: Add pool tables to INIT_SCHEMA**

Append to the `INIT_SCHEMA` string in `crates/ephemeris-pg/src/schema.rs`, before the closing `"#;`:

```sql

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

-- Add foreign key from serial_numbers.pool_id to sn_pools.id
-- (pool_id column already exists on serial_numbers from Phase 1)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_sn_pool'
    ) THEN
        ALTER TABLE serial_numbers
            ADD CONSTRAINT fk_sn_pool
            FOREIGN KEY (pool_id) REFERENCES sn_pools(id);
    END IF;
END $$;
```

**Step 2: Verify schema compiles**

Run: `cargo check -p ephemeris-pg`
Expected: Compiles cleanly (schema.rs is just a string constant).

**Step 3: Commit**

```bash
git add crates/ephemeris-pg/src/schema.rs
git commit -m "feat(pg): add sn_pools and pool_criteria tables to schema

Pool table, criteria junction table, FK from serial_numbers.pool_id,
and indexes per OPEN-SCS §6.9."
```

---

## Task 6: PostgreSQL PoolRepository Implementation

Implement `PoolRepository` for PostgreSQL.

**Files:**
- Create: `crates/ephemeris-pg/src/pool_repo.rs`
- Modify: `crates/ephemeris-pg/src/lib.rs`

**Step 1: Write the integration test**

Create `crates/ephemeris-pg/src/pool_repo.rs` with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial_number_repo::PgSerialNumberRepository;
    use ephemeris_core::domain::{Epc, PoolCriterionKey, PoolSelectionCriteria, SnState};
    use ephemeris_core::repository::SerialNumberRepository;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    async fn setup_test_db() -> (PgPoolRepository, PgSerialNumberRepository, impl std::any::Any) {
        let container = Postgres::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();

        let mut cfg = deadpool_postgres::Config::new();
        cfg.host = Some(host.to_string());
        cfg.port = Some(port);
        cfg.user = Some("postgres".to_string());
        cfg.password = Some("postgres".to_string());
        cfg.dbname = Some("postgres".to_string());
        let pool = cfg
            .create_pool(
                Some(deadpool_postgres::Runtime::Tokio1),
                tokio_postgres::NoTls,
            )
            .unwrap();

        let client = pool.get().await.unwrap();
        client
            .batch_execute(crate::schema::INIT_SCHEMA)
            .await
            .unwrap();

        let pool_repo = PgPoolRepository::new(pool.clone());
        let sn_repo = PgSerialNumberRepository::new(pool);
        (pool_repo, sn_repo, container)
    }

    fn make_pool(name: &str) -> SerialNumberPool {
        SerialNumberPool {
            id: PoolId::new(),
            name: name.to_string(),
            sid_class: Some("sgtin".to_string()),
            criteria: PoolSelectionCriteria {
                criteria: vec![
                    (PoolCriterionKey::Gtin, "06141410123456".to_string()),
                ],
            },
            esm_endpoint: None,
            created_at: chrono::Utc::now().fixed_offset(),
            updated_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[tokio::test]
    async fn test_create_and_get_pool() {
        let (repo, _, _container) = setup_test_db().await;
        let pool = make_pool("Test Pool");
        let id = pool.id.clone();

        repo.create_pool(&pool).await.unwrap();

        let fetched = repo.get_pool(&id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test Pool");
        assert_eq!(fetched.sid_class.as_deref(), Some("sgtin"));
        assert_eq!(fetched.criteria.criteria.len(), 1);
        assert_eq!(fetched.criteria.criteria[0].0, PoolCriterionKey::Gtin);
    }

    #[tokio::test]
    async fn test_list_pools() {
        let (repo, _, _container) = setup_test_db().await;
        repo.create_pool(&make_pool("Pool A")).await.unwrap();
        repo.create_pool(&make_pool("Pool B")).await.unwrap();

        let all = repo.list_pools(&PoolQuery::default()).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_empty_pool() {
        let (repo, _, _container) = setup_test_db().await;
        let pool = make_pool("Delete Me");
        let id = pool.id.clone();
        repo.create_pool(&pool).await.unwrap();

        repo.delete_pool(&id).await.unwrap();
        assert!(repo.get_pool(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_assign_and_request_numbers() {
        let (pool_repo, sn_repo, _container) = setup_test_db().await;
        let pool = make_pool("Alloc Pool");
        let pool_id = pool.id.clone();
        pool_repo.create_pool(&pool).await.unwrap();

        // Pre-create some SNs with unallocated state and this pool_id
        for i in 0..5 {
            let epc = Epc::new(format!("urn:epc:id:sgtin:0614141.107346.{i:04}"));
            sn_repo
                .upsert_state(&epc, SnState::Unallocated, None, Some(&pool_id.0.to_string()))
                .await
                .unwrap();
        }

        // Request 3
        let allocated = pool_repo.request_numbers(&pool_id, 3).await.unwrap();
        assert_eq!(allocated.len(), 3);

        // Verify they're now allocated
        for epc in &allocated {
            let sn = sn_repo.get_state(epc).await.unwrap().unwrap();
            assert_eq!(sn.state, SnState::Allocated);
        }
    }

    #[tokio::test]
    async fn test_return_numbers() {
        let (pool_repo, sn_repo, _container) = setup_test_db().await;
        let pool = make_pool("Return Pool");
        let pool_id = pool.id.clone();
        pool_repo.create_pool(&pool).await.unwrap();

        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.0001");
        sn_repo
            .upsert_state(&epc, SnState::Allocated, None, Some(&pool_id.0.to_string()))
            .await
            .unwrap();

        let returned = pool_repo.return_numbers(&pool_id, &[epc.clone()]).await.unwrap();
        assert_eq!(returned, 1);

        let sn = sn_repo.get_state(&epc).await.unwrap().unwrap();
        assert_eq!(sn.state, SnState::Unallocated);
    }

    #[tokio::test]
    async fn test_get_pool_stats() {
        let (pool_repo, sn_repo, _container) = setup_test_db().await;
        let pool = make_pool("Stats Pool");
        let pool_id = pool.id.clone();
        pool_repo.create_pool(&pool).await.unwrap();

        let pid_str = pool_id.0.to_string();
        sn_repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.001"),
            SnState::Unallocated, None, Some(&pid_str),
        ).await.unwrap();
        sn_repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.002"),
            SnState::Unallocated, None, Some(&pid_str),
        ).await.unwrap();
        sn_repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.003"),
            SnState::Allocated, None, Some(&pid_str),
        ).await.unwrap();

        let stats = pool_repo.get_pool_stats(&pool_id).await.unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.unallocated, 2);
        assert_eq!(stats.allocated, 1);
    }

    #[tokio::test]
    async fn test_request_more_than_available() {
        let (pool_repo, sn_repo, _container) = setup_test_db().await;
        let pool = make_pool("Sparse Pool");
        let pool_id = pool.id.clone();
        pool_repo.create_pool(&pool).await.unwrap();

        // Only 2 unallocated SNs
        for i in 0..2 {
            sn_repo.upsert_state(
                &Epc::new(format!("urn:epc:id:sgtin:0614141.107346.{i:04}")),
                SnState::Unallocated, None, Some(&pool_id.0.to_string()),
            ).await.unwrap();
        }

        // Request 10 — should return 2 (no error)
        let allocated = pool_repo.request_numbers(&pool_id, 10).await.unwrap();
        assert_eq!(allocated.len(), 2);
    }

    #[tokio::test]
    async fn test_assign_to_pool() {
        let (pool_repo, sn_repo, _container) = setup_test_db().await;
        let pool = make_pool("Import Pool");
        let pool_id = pool.id.clone();
        pool_repo.create_pool(&pool).await.unwrap();

        let epcs = vec![
            Epc::new("urn:epc:id:sgtin:0614141.107346.A001"),
            Epc::new("urn:epc:id:sgtin:0614141.107346.A002"),
        ];

        let count = pool_repo.assign_to_pool(&pool_id, &epcs, Some("unallocated")).await.unwrap();
        assert_eq!(count, 2);

        // Verify SNs are created with correct state and pool_id
        let sn = sn_repo.get_state(&epcs[0]).await.unwrap().unwrap();
        assert_eq!(sn.state, SnState::Unallocated);
        assert_eq!(sn.pool_id.as_deref(), Some(pool_id.0.to_string().as_str()));
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ephemeris-pg pool_repo::tests --no-run 2>&1`
Expected: Compilation fails — `PgPoolRepository` doesn't exist.

**Step 3: Write the implementation above the tests**

```rust
use std::str::FromStr;

use deadpool_postgres::Pool;
use ephemeris_core::domain::{
    Epc, PoolCriterionKey, PoolId, PoolQuery, PoolSelectionCriteria, PoolStats, SerialNumberPool,
    SnState,
};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::PoolRepository;

/// PostgreSQL-backed pool repository.
#[derive(Clone)]
pub struct PgPoolRepository {
    pool: Pool,
}

impl PgPoolRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl PoolRepository for PgPoolRepository {
    async fn create_pool(&self, pool: &SerialNumberPool) -> Result<PoolId, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        client
            .execute(
                "INSERT INTO sn_pools (id, name, sid_class, esm_endpoint, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)",
                &[
                    &pool.id.0,
                    &pool.name,
                    &pool.sid_class,
                    &pool.esm_endpoint,
                    &pool.created_at,
                    &pool.updated_at,
                ],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        // Insert criteria
        for (key, value) in &pool.criteria.criteria {
            let key_str = serde_json::to_value(key)
                .map_err(|e| RepoError::Serialization(e.to_string()))?
                .as_str()
                .unwrap_or("unknown")
                .to_string();

            client
                .execute(
                    "INSERT INTO pool_criteria (pool_id, key, value) VALUES ($1, $2, $3)",
                    &[&pool.id.0, &key_str, &value],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;
        }

        Ok(pool.id.clone())
    }

    async fn get_pool(&self, id: &PoolId) -> Result<Option<SerialNumberPool>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let row = client
            .query_opt(
                "SELECT id, name, sid_class, esm_endpoint, created_at, updated_at
                 FROM sn_pools WHERE id = $1",
                &[&id.0],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        // Fetch criteria
        let criteria_rows = client
            .query(
                "SELECT key, value FROM pool_criteria WHERE pool_id = $1",
                &[&id.0],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let criteria: Vec<(PoolCriterionKey, String)> = criteria_rows
            .iter()
            .map(|r| {
                let key_str: String = r.get(0);
                let value: String = r.get(1);
                let key = parse_criterion_key(&key_str);
                (key, value)
            })
            .collect();

        Ok(Some(SerialNumberPool {
            id: PoolId(row.get(0)),
            name: row.get(1),
            sid_class: row.get(2),
            criteria: PoolSelectionCriteria { criteria },
            esm_endpoint: row.get(3),
            created_at: row.get(4),
            updated_at: row.get(5),
        }))
    }

    async fn list_pools(&self, filter: &PoolQuery) -> Result<Vec<SerialNumberPool>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let mut sql =
            String::from("SELECT id, name, sid_class, esm_endpoint, created_at, updated_at FROM sn_pools WHERE 1=1");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        let mut idx = 1;

        if let Some(ref sid_class) = filter.sid_class {
            sql.push_str(&format!(" AND sid_class = ${idx}"));
            params.push(Box::new(sid_class.clone()));
            idx += 1;
        }
        if let Some(ref name_contains) = filter.name_contains {
            sql.push_str(&format!(" AND name ILIKE ${idx}"));
            params.push(Box::new(format!("%{name_contains}%")));
            idx += 1;
        }

        let limit = filter.limit.unwrap_or(100) as i64;
        let offset = filter.offset.unwrap_or(0) as i64;
        sql.push_str(&format!(
            " ORDER BY created_at DESC LIMIT ${idx} OFFSET ${}",
            idx + 1
        ));
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref() as _).collect();

        let rows = client
            .query(&sql, &param_refs)
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let mut pools = Vec::new();
        for row in &rows {
            let pool_id: uuid::Uuid = row.get(0);
            // Fetch criteria for each pool
            let criteria_rows = client
                .query(
                    "SELECT key, value FROM pool_criteria WHERE pool_id = $1",
                    &[&pool_id],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;

            let criteria: Vec<(PoolCriterionKey, String)> = criteria_rows
                .iter()
                .map(|r| {
                    let key_str: String = r.get(0);
                    let value: String = r.get(1);
                    (parse_criterion_key(&key_str), value)
                })
                .collect();

            pools.push(SerialNumberPool {
                id: PoolId(pool_id),
                name: row.get(1),
                sid_class: row.get(2),
                criteria: PoolSelectionCriteria { criteria },
                esm_endpoint: row.get(3),
                created_at: row.get(4),
                updated_at: row.get(5),
            });
        }

        Ok(pools)
    }

    async fn delete_pool(&self, id: &PoolId) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        // Check for assigned serial numbers
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM serial_numbers WHERE pool_id = $1",
                &[&id.0.to_string()],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?
            .get(0);

        if count > 0 {
            return Err(RepoError::Query(format!(
                "cannot delete pool {}: {} serial numbers still assigned",
                id.0, count
            )));
        }

        client
            .execute("DELETE FROM sn_pools WHERE id = $1", &[&id.0])
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(())
    }

    async fn assign_to_pool(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
        initial_state: Option<&str>,
    ) -> Result<u32, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let state = initial_state.unwrap_or("unallocated");
        let pool_id_str = pool_id.0.to_string();
        let mut count = 0u32;

        for epc in epcs {
            client
                .execute(
                    "INSERT INTO serial_numbers (epc, state, pool_id)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (epc) DO UPDATE SET
                         pool_id = $3,
                         state = $2,
                         updated_at = now()",
                    &[&epc.as_str(), &state, &pool_id_str],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;
            count += 1;
        }

        Ok(count)
    }

    async fn request_numbers(
        &self,
        pool_id: &PoolId,
        count: u32,
    ) -> Result<Vec<Epc>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let pool_id_str = pool_id.0.to_string();

        // SELECT ... FOR UPDATE SKIP LOCKED to handle concurrent requests
        let rows = client
            .query(
                "UPDATE serial_numbers SET state = 'allocated', updated_at = now()
                 WHERE epc IN (
                     SELECT epc FROM serial_numbers
                     WHERE pool_id = $1 AND state = 'unallocated'
                     FOR UPDATE SKIP LOCKED
                     LIMIT $2
                 )
                 RETURNING epc",
                &[&pool_id_str, &(count as i64)],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| Epc::new(r.get::<_, &str>(0))).collect())
    }

    async fn return_numbers(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
    ) -> Result<u32, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let pool_id_str = pool_id.0.to_string();
        let epc_strings: Vec<&str> = epcs.iter().map(|e| e.as_str()).collect();

        let rows_affected = client
            .execute(
                "UPDATE serial_numbers SET state = 'unallocated', updated_at = now()
                 WHERE epc = ANY($1) AND pool_id = $2",
                &[&epc_strings, &pool_id_str],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(rows_affected as u32)
    }

    async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let pool_id_str = pool_id.0.to_string();

        let rows = client
            .query(
                "SELECT state, COUNT(*) as cnt FROM serial_numbers
                 WHERE pool_id = $1 GROUP BY state",
                &[&pool_id_str],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let mut stats = PoolStats {
            pool_id: pool_id.clone(),
            total: 0,
            unassigned: 0,
            unallocated: 0,
            allocated: 0,
            encoded: 0,
            commissioned: 0,
            other: 0,
        };

        for row in &rows {
            let state_str: String = row.get(0);
            let count: i64 = row.get(1);
            let count = count as u64;
            stats.total += count;

            match SnState::from_str(&state_str) {
                Ok(SnState::Unassigned) => stats.unassigned += count,
                Ok(SnState::Unallocated) => stats.unallocated += count,
                Ok(SnState::Allocated) => stats.allocated += count,
                Ok(SnState::Encoded) => stats.encoded += count,
                Ok(SnState::Commissioned) => stats.commissioned += count,
                _ => stats.other += count,
            }
        }

        Ok(stats)
    }
}

/// Parse a criterion key string back to the enum.
fn parse_criterion_key(s: &str) -> PoolCriterionKey {
    match s {
        "gtin" => PoolCriterionKey::Gtin,
        "sscc_gcp" => PoolCriterionKey::SsccGcp,
        "sscc_extension" => PoolCriterionKey::SsccExtension,
        "country_code" => PoolCriterionKey::CountryCode,
        "location" => PoolCriterionKey::Location,
        "sublocation" => PoolCriterionKey::Sublocation,
        "lot_number" => PoolCriterionKey::LotNumber,
        "pool_id" => PoolCriterionKey::PoolId,
        "sid_class_id" => PoolCriterionKey::SidClassId,
        "order_id" => PoolCriterionKey::OrderId,
        other => PoolCriterionKey::Custom(other.to_string()),
    }
}
```

**Step 4: Register the module**

In `crates/ephemeris-pg/src/lib.rs`, add:

```rust
pub mod pool_repo;

pub use pool_repo::PgPoolRepository;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p ephemeris-pg pool_repo::tests -- --nocapture`
Expected: All 8 tests PASS (requires Docker for testcontainers).

**Step 6: Commit**

```bash
git add crates/ephemeris-pg/src/pool_repo.rs crates/ephemeris-pg/src/lib.rs
git commit -m "feat(pg): implement PgPoolRepository

CRUD, request_numbers (FOR UPDATE SKIP LOCKED), return_numbers,
assign_to_pool, get_pool_stats with state aggregation."
```

---

## Task 7: NoopEsmClient

Create a no-op ESM client for when no ESM is configured. This satisfies the `EsmClient` trait bound without requiring reqwest in builds that don't need upstream communication.

**Files:**
- Create: `crates/ephemeris-core/src/service/noop_esm.rs`
- Modify: `crates/ephemeris-core/src/service/mod.rs`

**Step 1: Write the NoopEsmClient**

Create `crates/ephemeris-core/src/service/noop_esm.rs`:

```rust
use crate::domain::{Epc, PoolSelectionCriteria};
use crate::error::EsmError;
use crate::repository::EsmClient;

/// No-op ESM client for deployments without upstream ESM connectivity.
///
/// All upstream operations return `EsmError::NotConfigured`.
/// Local pool operations (receive, request, return) still work.
#[derive(Clone)]
pub struct NoopEsmClient;

impl EsmClient for NoopEsmClient {
    async fn request_unassigned(
        &self,
        _count: u32,
        _criteria: &PoolSelectionCriteria,
    ) -> Result<Vec<Epc>, EsmError> {
        Err(EsmError::NotConfigured)
    }

    async fn return_unallocated(&self, _epcs: &[Epc]) -> Result<u32, EsmError> {
        Err(EsmError::NotConfigured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_request_returns_not_configured() {
        let client = NoopEsmClient;
        let result = client
            .request_unassigned(10, &PoolSelectionCriteria::default())
            .await;
        assert!(matches!(result, Err(EsmError::NotConfigured)));
    }

    #[tokio::test]
    async fn test_noop_return_returns_not_configured() {
        let client = NoopEsmClient;
        let result = client.return_unallocated(&[]).await;
        assert!(matches!(result, Err(EsmError::NotConfigured)));
    }
}
```

**Step 2: Register the module**

In `crates/ephemeris-core/src/service/mod.rs`, add:

```rust
pub mod noop_esm;

pub use noop_esm::NoopEsmClient;
```

**Step 3: Run tests**

Run: `cargo test -p ephemeris-core noop_esm -- --nocapture`
Expected: 2 tests PASS.

**Step 4: Commit**

```bash
git add crates/ephemeris-core/src/service/noop_esm.rs crates/ephemeris-core/src/service/mod.rs
git commit -m "feat(core): add NoopEsmClient for deployments without ESM

Returns EsmError::NotConfigured for all upstream operations.
Local pool operations still work via PoolService."
```

---

## Task 8: REST API Pool Routes

Add 9 pool management endpoints to the REST API.

**Files:**
- Create: `crates/ephemeris-api/src/routes/pools.rs`
- Modify: `crates/ephemeris-api/src/routes/mod.rs`
- Modify: `crates/ephemeris-api/src/state.rs`
- Modify: `crates/ephemeris-api/src/lib.rs`

**Step 1: Update AppState to include PoolService**

Modify `crates/ephemeris-api/src/state.rs`:

```rust
use ephemeris_core::repository::{
    AggregationRepository, EsmClient, EventRepository, PoolRepository, SerialNumberRepository,
};
use ephemeris_core::service::{PoolService, SerialNumberService};

/// Shared application state holding repository implementations and services.
pub struct AppState<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
> {
    pub event_repo: E,
    pub agg_repo: A,
    pub sn_service: SerialNumberService<S>,
    pub pool_service: PoolService<P, C>,
}
```

**Step 2: Update all generic signatures**

This change affects every file that references `AppState`. Update:

- `crates/ephemeris-api/src/lib.rs` — `create_router` function signature
- `crates/ephemeris-api/src/routes/events.rs` — all handler functions
- `crates/ephemeris-api/src/routes/serial_numbers.rs` — all handler functions
- `crates/ephemeris-api/src/routes/hierarchy.rs` — all handler functions
- `crates/ephemeris-api/src/routes/health.rs` — health check handler (if it uses State)

The pattern is: add `P: PoolRepository, C: EsmClient` to every generic parameter list where `AppState<E, A, S>` becomes `AppState<E, A, S, P, C>`.

For example, in `events.rs`:
```rust
pub async fn query_events<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    // ...
```

**Step 3: Write pool route handlers**

Create `crates/ephemeris-api/src/routes/pools.rs`:

```rust
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use ephemeris_core::domain::{
    Epc, PoolCriterionKey, PoolId, PoolQuery, PoolReceiveRequest, PoolRequest,
    PoolReturnRequest, PoolSelectionCriteria, SerialNumberPool,
};
use ephemeris_core::repository::{
    AggregationRepository, EsmClient, EventRepository, PoolRepository, SerialNumberRepository,
};

use crate::state::AppState;

/// POST body for creating a pool.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePoolRequest {
    pub name: String,
    pub sid_class: Option<String>,
    pub criteria: Option<Vec<(PoolCriterionKey, String)>>,
    pub esm_endpoint: Option<String>,
}

/// POST /pools — create a new pool.
pub async fn create_pool<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let now = chrono::Utc::now().fixed_offset();
    let pool = SerialNumberPool {
        id: PoolId::new(),
        name: req.name,
        sid_class: req.sid_class,
        criteria: PoolSelectionCriteria {
            criteria: req.criteria.unwrap_or_default(),
        },
        esm_endpoint: req.esm_endpoint,
        created_at: now,
        updated_at: now,
    };

    match state.pool_service.create_pool(&pool).await {
        Ok(id) => Ok((
            StatusCode::CREATED,
            Json(json!({"poolId": id.0})),
        )),
        Err(e) => {
            tracing::error!("Failed to create pool: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// GET /pools — list pools with optional filters.
pub async fn list_pools<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Query(query): Query<PoolQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .pool_service
        .list_pools(&query)
        .await
        .map(|pools| Json(serde_json::to_value(pools).unwrap()))
        .map_err(|e| {
            tracing::error!("Failed to list pools: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// GET /pools/{id} — get pool details + stats.
pub async fn get_pool<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state.pool_service.get_pool(&pool_id).await {
        Ok(Some(pool)) => {
            let stats = state.pool_service.get_pool_stats(&pool_id).await.ok();
            let mut val = serde_json::to_value(pool).unwrap();
            if let Some(s) = stats {
                val["stats"] = serde_json::to_value(s).unwrap();
            }
            Ok(Json(val))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("pool {id} not found")})),
        )),
        Err(e) => {
            tracing::error!("Failed to get pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// DELETE /pools/{id} — delete an empty pool.
pub async fn delete_pool<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state.pool_service.delete_pool(&pool_id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            tracing::error!("Failed to delete pool {id}: {e}");
            let status = if e.to_string().contains("still assigned") {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            Err((status, Json(json!({"error": e.to_string()}))))
        }
    }
}

/// POST /pools/{id}/request — request (allocate) SNs from a pool.
pub async fn request_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state.pool_service.request_numbers(&pool_id, req.count).await {
        Ok(response) => Ok(Json(serde_json::to_value(response).unwrap())),
        Err(e) => {
            tracing::error!("Failed to request numbers from pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// POST /pools/{id}/return — return (deallocate) SNs back to a pool.
pub async fn return_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolReturnRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state.pool_service.return_numbers(&pool_id, &req.serial_numbers).await {
        Ok(returned) => Ok(Json(json!({
            "poolId": id,
            "returned": returned,
        }))),
        Err(e) => {
            tracing::error!("Failed to return numbers to pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// POST /pools/{id}/receive — receive (import) SNs into a pool.
pub async fn receive_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolReceiveRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .receive_numbers(
            &pool_id,
            &req.serial_numbers,
            req.sid_class.as_deref(),
            req.initial_state.as_deref(),
        )
        .await
    {
        Ok(received) => Ok(Json(json!({
            "poolId": id,
            "received": received,
        }))),
        Err(e) => {
            tracing::error!("Failed to receive numbers into pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// POST /pools/{id}/request-upstream — request new SNs from upstream ESM.
pub async fn request_upstream<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .request_upstream(&pool_id, req.count, &req.criteria)
        .await
    {
        Ok(response) => Ok(Json(serde_json::to_value(response).unwrap())),
        Err(e) => {
            let status = match &e {
                ephemeris_core::error::EsmError::NotConfigured => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::BAD_GATEWAY,
            };
            tracing::error!("Failed to request from upstream ESM: {e}");
            Err((status, Json(json!({"error": e.to_string()}))))
        }
    }
}

/// POST /pools/{id}/return-upstream — return SNs back to upstream ESM.
pub async fn return_upstream<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolReturnRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .return_upstream(&pool_id, &req.serial_numbers)
        .await
    {
        Ok(returned) => Ok(Json(json!({
            "poolId": id,
            "returned": returned,
        }))),
        Err(e) => {
            let status = match &e {
                ephemeris_core::error::EsmError::NotConfigured => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::BAD_GATEWAY,
            };
            tracing::error!("Failed to return to upstream ESM: {e}");
            Err((status, Json(json!({"error": e.to_string()}))))
        }
    }
}
```

**Step 4: Register routes module**

In `crates/ephemeris-api/src/routes/mod.rs`, add:

```rust
pub mod pools;
```

**Step 5: Wire routes into the router**

In `crates/ephemeris-api/src/lib.rs`, update `create_router` to add pool routes. The function signature becomes:

```rust
pub fn create_router<E, A, S, P, C>(state: Arc<AppState<E, A, S, P, C>>) -> Router
where
    E: EventRepository + 'static,
    A: AggregationRepository + 'static,
    S: SerialNumberRepository + 'static,
    P: PoolRepository + 'static,
    C: EsmClient + 'static,
{
```

Add these routes before `.layer(TraceLayer::new_for_http())`:

```rust
        .route("/pools", post(pools::create_pool::<E, A, S, P, C>))
        .route("/pools", get(pools::list_pools::<E, A, S, P, C>))
        .route("/pools/{id}", get(pools::get_pool::<E, A, S, P, C>))
        .route("/pools/{id}", axum::routing::delete(pools::delete_pool::<E, A, S, P, C>))
        .route("/pools/{id}/request", post(pools::request_numbers::<E, A, S, P, C>))
        .route("/pools/{id}/return", post(pools::return_numbers::<E, A, S, P, C>))
        .route("/pools/{id}/receive", post(pools::receive_numbers::<E, A, S, P, C>))
        .route("/pools/{id}/request-upstream", post(pools::request_upstream::<E, A, S, P, C>))
        .route("/pools/{id}/return-upstream", post(pools::return_upstream::<E, A, S, P, C>))
```

**Step 6: Update test stubs in lib.rs**

In the `#[cfg(test)]` module in `crates/ephemeris-api/src/lib.rs`, add stub implementations for `PoolRepository` and `EsmClient`, and update the `AppState` construction in each test. Add these stubs:

```rust
    struct StubPoolRepo;

    impl ephemeris_core::repository::PoolRepository for StubPoolRepo {
        async fn create_pool(&self, pool: &ephemeris_core::domain::SerialNumberPool) -> Result<ephemeris_core::domain::PoolId, RepoError> {
            Ok(pool.id.clone())
        }
        async fn get_pool(&self, _id: &ephemeris_core::domain::PoolId) -> Result<Option<ephemeris_core::domain::SerialNumberPool>, RepoError> {
            Ok(None)
        }
        async fn list_pools(&self, _filter: &ephemeris_core::domain::PoolQuery) -> Result<Vec<ephemeris_core::domain::SerialNumberPool>, RepoError> {
            Ok(vec![])
        }
        async fn delete_pool(&self, _id: &ephemeris_core::domain::PoolId) -> Result<(), RepoError> {
            Ok(())
        }
        async fn assign_to_pool(&self, _pool_id: &ephemeris_core::domain::PoolId, epcs: &[Epc], _initial_state: Option<&str>) -> Result<u32, RepoError> {
            Ok(epcs.len() as u32)
        }
        async fn request_numbers(&self, _pool_id: &ephemeris_core::domain::PoolId, _count: u32) -> Result<Vec<Epc>, RepoError> {
            Ok(vec![])
        }
        async fn return_numbers(&self, _pool_id: &ephemeris_core::domain::PoolId, _epcs: &[Epc]) -> Result<u32, RepoError> {
            Ok(0)
        }
        async fn get_pool_stats(&self, pool_id: &ephemeris_core::domain::PoolId) -> Result<ephemeris_core::domain::PoolStats, RepoError> {
            Ok(ephemeris_core::domain::PoolStats {
                pool_id: pool_id.clone(),
                total: 0, unassigned: 0, unallocated: 0, allocated: 0,
                encoded: 0, commissioned: 0, other: 0,
            })
        }
    }

    use ephemeris_core::service::NoopEsmClient;
```

Update each test's `AppState` construction from:
```rust
AppState {
    event_repo: StubEventRepo,
    agg_repo: StubAggRepo,
    sn_service: SerialNumberService::new(StubSnRepo::new()),
}
```
to:
```rust
AppState {
    event_repo: StubEventRepo,
    agg_repo: StubAggRepo,
    sn_service: SerialNumberService::new(StubSnRepo::new()),
    pool_service: PoolService::new(StubPoolRepo, NoopEsmClient),
}
```

**Step 7: Verify everything compiles and tests pass**

Run: `cargo test -p ephemeris-api -- --nocapture`
Expected: All existing tests pass, plus the new pool endpoints are wired.

**Step 8: Commit**

```bash
git add crates/ephemeris-api/
git commit -m "feat(api): add 9 REST endpoints for pool management

POST /pools, GET /pools, GET /pools/{id}, DELETE /pools/{id},
POST /pools/{id}/request, /return, /receive, /request-upstream, /return-upstream.
AppState now generic over PoolRepository + EsmClient."
```

---

## Task 9: App Wiring

Wire `PgPoolRepository` and `NoopEsmClient` into the app startup.

**Files:**
- Modify: `crates/ephemeris-app/src/main.rs`

**Step 1: Update run_app signature and body**

The `run_app` function needs two new generic params (`P: PoolRepository, C: EsmClient`) and the `AppState` construction needs `pool_service`.

Update the function signature:

```rust
async fn run_app<E, A, S, P, C>(
    event_repo: E,
    agg_repo: A,
    sn_repo: S,
    pool_service: PoolService<P, C>,
    app_config: AppConfig,
) -> Result<(), Box<dyn std::error::Error>>
where
    E: EventRepository + Clone + 'static,
    A: AggregationRepository + Clone + 'static,
    S: SerialNumberRepository + Clone + 'static,
    P: PoolRepository + 'static,
    C: EsmClient + 'static,
```

Update `AppState` construction:

```rust
    let state = Arc::new(AppState {
        event_repo: event_repo.clone(),
        agg_repo: agg_repo.clone(),
        sn_service: SerialNumberService::new(sn_repo.clone()),
        pool_service,
    });
```

**Step 2: Build pool_service in the postgres match arm**

In the `"postgres"` branch of `main()`, after creating `sn_repo`:

```rust
            let pool_pool = build_pg_pool(&conn_str, pg_cfg.pool_size)?;
            let pool_repo = ephemeris_pg::PgPoolRepository::new(pool_pool);
            let pool_service = ephemeris_core::service::PoolService::new(
                pool_repo,
                ephemeris_core::service::NoopEsmClient,
            );

            run_app(event_repo, agg_repo, sn_repo, pool_service, app_config).await
```

**Step 3: Update the enterprise-arango match arm similarly**

In the `"arango"` branch, add the same pool construction:

```rust
            let pool_pool = build_pg_pool(&conn_str, pg_cfg.pool_size)?;
            let pool_repo = ephemeris_pg::PgPoolRepository::new(pool_pool);
            let pool_service = ephemeris_core::service::PoolService::new(
                pool_repo,
                ephemeris_core::service::NoopEsmClient,
            );

            run_app(event_repo, agg_repo, sn_repo, pool_service, app_config).await
```

**Step 4: Update MQTT handler imports**

The `EventHandler::new()` call might need updating if it references `AppState` generics. Check `crates/ephemeris-mqtt/src/handler.rs` — if it uses its own separate generics (not tied to AppState), no changes needed.

**Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles cleanly.

**Step 6: Commit**

```bash
git add crates/ephemeris-app/src/main.rs
git commit -m "feat(app): wire PgPoolRepository + NoopEsmClient into startup

Pool service available in both postgres and arango backends.
NoopEsmClient used by default (upstream endpoints return 503)."
```

---

## Task 10: API Pool Tests

Add integration-style tests for the pool REST endpoints using in-memory stubs (same pattern as existing API tests).

**Files:**
- Modify: `crates/ephemeris-api/src/lib.rs` (add tests to the existing `#[cfg(test)]` module)

**Step 1: Write pool API tests**

Add these tests to the `#[cfg(test)] mod tests` block in `crates/ephemeris-api/src/lib.rs`:

```rust
    #[tokio::test]
    async fn create_pool_returns_201() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(StubSnRepo::new()),
            pool_service: PoolService::new(StubPoolRepo, NoopEsmClient),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pools")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name": "Test Pool", "sidClass": "sgtin"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["poolId"].is_string());
    }

    #[tokio::test]
    async fn list_pools_returns_200() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(StubSnRepo::new()),
            pool_service: PoolService::new(StubPoolRepo, NoopEsmClient),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/pools")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_pool_not_found_returns_404() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(StubSnRepo::new()),
            pool_service: PoolService::new(StubPoolRepo, NoopEsmClient),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/pools/00000000-0000-0000-0000-000000000001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn request_upstream_without_esm_returns_503() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(StubSnRepo::new()),
            pool_service: PoolService::new(StubPoolRepo, NoopEsmClient),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pools/00000000-0000-0000-0000-000000000001/request-upstream")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"count": 10, "criteria": {"criteria": []}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
```

**Step 2: Run all API tests**

Run: `cargo test -p ephemeris-api -- --nocapture`
Expected: All tests pass (existing + 4 new pool tests).

**Step 3: Commit**

```bash
git add crates/ephemeris-api/src/lib.rs
git commit -m "test(api): add pool REST endpoint tests

create_pool 201, list_pools 200, get_pool 404,
request_upstream without ESM returns 503."
```

---

## Task 11: Full Build Verification + Clippy

Run the full test suite, clippy, and verify everything works together.

**Files:** None (verification only).

**Step 1: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: Zero warnings.

If any warnings appear, fix them before proceeding.

**Step 2: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass across all crates.

**Step 3: Run formatter**

Run: `cargo fmt --check`
Expected: No formatting issues.

**Step 4: Fix any issues found, then commit**

If any fixes were needed:
```bash
git add -A
git commit -m "fix: address clippy/fmt issues in pool management"
```

---

## Task 12: Update test-app.ps1

Add pool management menu options to the interactive test runner.

**Files:**
- Modify: `test-app.ps1`

**Step 1: Add pool test functions**

After the existing `Send-*` functions, add:

```powershell
function Create-Pool {
    Write-Host "`n  Creating pool..."
    $body = @"
{
    "name": "GTIN-614141 Production Pool",
    "sidClass": "sgtin",
    "criteria": [["gtin", "06141410123456"]]
}
"@
    $result = $body | curl.exe -s -X POST "http://localhost:8080/pools" -H "Content-Type: application/json" -d "@-"
    Write-Host "  Response: $result"
    # Extract poolId for subsequent operations
    $script:LastPoolId = ($result | ConvertFrom-Json).poolId
    Write-Host "  Pool ID: $($script:LastPoolId)"
}

function Receive-Numbers {
    if (-not $script:LastPoolId) {
        Write-Host "  No pool created yet. Create a pool first (option 7)."
        return
    }
    Write-Host "`n  Receiving 5 SNs into pool..."
    $body = @"
{
    "serialNumbers": [
        "urn:epc:id:sgtin:0614141.107346.P001",
        "urn:epc:id:sgtin:0614141.107346.P002",
        "urn:epc:id:sgtin:0614141.107346.P003",
        "urn:epc:id:sgtin:0614141.107346.P004",
        "urn:epc:id:sgtin:0614141.107346.P005"
    ],
    "sidClass": "sgtin",
    "initialState": "unallocated"
}
"@
    $result = $body | curl.exe -s -X POST "http://localhost:8080/pools/$($script:LastPoolId)/receive" -H "Content-Type: application/json" -d "@-"
    Write-Host "  Response: $result"
}

function Request-Numbers {
    if (-not $script:LastPoolId) {
        Write-Host "  No pool created yet. Create a pool first (option 7)."
        return
    }
    Write-Host "`n  Requesting 3 SNs from pool..."
    $body = @"
{"count": 3, "criteria": {"criteria": []}}
"@
    $result = $body | curl.exe -s -X POST "http://localhost:8080/pools/$($script:LastPoolId)/request" -H "Content-Type: application/json" -d "@-"
    Write-Host "  Response: $result"
}

function Get-PoolStats {
    if (-not $script:LastPoolId) {
        Write-Host "  No pool created yet. Create a pool first (option 7)."
        return
    }
    Write-Host "`n  Getting pool details + stats..."
    $result = curl.exe -s "http://localhost:8080/pools/$($script:LastPoolId)"
    Write-Host "  Response: $result"
}
```

**Step 2: Update the menu**

Add options 7-10 (or wherever the next available numbers are) to `Show-Menu`:

```
  7) Create pool
  8) Receive SNs into pool
  9) Request SNs from pool
  10) View pool stats
```

And add the corresponding switch cases.

**Step 3: Test manually**

Run: `.\test-app.ps1`
Expected: New menu options appear and work end-to-end.

**Step 4: Commit**

```bash
git add test-app.ps1
git commit -m "chore: add pool management options to interactive test runner"
```

---

## Dependency Graph

```
Task 1 (Domain Types)
  └─→ Task 2 (PoolRepository Trait) ─────────→ Task 6 (PG Pool Impl) ──→ Task 9 (App Wiring)
  └─→ Task 3 (EsmClient Trait) ──→ Task 7 (NoopEsmClient) ──────────────→ Task 9 (App Wiring)
  └─→ Task 4 (PoolService) ─────────────────────────────────────────────→ Task 9 (App Wiring)
Task 5 (PG Schema) ────────────→ Task 6 (PG Pool Impl)
Task 8 (API Routes) ← depends on Tasks 2, 3, 4
Task 9 (App Wiring) ← depends on Tasks 6, 7, 8
Task 10 (API Tests) ← depends on Task 8
Task 11 (Verification) ← depends on all
Task 12 (test-app.ps1) ← depends on Task 11
```

Tasks 1, 5 can run in parallel.
Tasks 2, 3 can run in parallel (both depend on 1).
Tasks 4, 7 can run after 2, 3.
Task 6 depends on 2, 5.
Task 8 depends on 2, 3, 4.
Task 9 depends on 6, 7, 8.
Tasks 10, 11, 12 are sequential at the end.

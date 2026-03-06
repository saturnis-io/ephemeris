use std::str::FromStr;

use crate::domain::{
    Epc, PoolId, PoolQuery, PoolResponse, PoolSelectionCriteria, PoolStats, SerialNumberPool,
    SnState,
};
use crate::error::{EsmError, RepoError};
use crate::repository::{EsmClient, PoolRepository};

/// Service layer for serial number pool management.
///
/// Orchestrates pool CRUD, local allocation/return, and upstream ESM
/// communication. Business logic lives here; storage is delegated to
/// the `PoolRepository` and upstream calls to the `EsmClient`.
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

    /// Create a new serial number pool.
    pub async fn create_pool(&self, pool: &SerialNumberPool) -> Result<PoolId, RepoError> {
        self.pool_repo.create_pool(pool).await
    }

    /// Get a pool by its ID.
    pub async fn get_pool(&self, id: &PoolId) -> Result<Option<SerialNumberPool>, RepoError> {
        self.pool_repo.get_pool(id).await
    }

    /// List pools matching the given filter criteria.
    pub async fn list_pools(&self, filter: &PoolQuery) -> Result<Vec<SerialNumberPool>, RepoError> {
        self.pool_repo.list_pools(filter).await
    }

    /// Delete an empty pool.
    pub async fn delete_pool(&self, id: &PoolId) -> Result<(), RepoError> {
        self.pool_repo.delete_pool(id).await
    }

    /// Request (allocate) serial numbers from a local pool.
    ///
    /// Delegates to the repository to move SNs from Unallocated to Allocated,
    /// then wraps the result in a `PoolResponse`.
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

    /// Return (deallocate) serial numbers back to a local pool.
    pub async fn return_numbers(&self, pool_id: &PoolId, epcs: &[Epc]) -> Result<u32, RepoError> {
        self.pool_repo.return_numbers(pool_id, epcs).await
    }

    /// Receive serial numbers into a pool (e.g., from an ESM or manual import).
    ///
    /// Validates `initial_state` against the `SnState` enum if provided.
    pub async fn receive_numbers(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
        _sid_class: Option<&str>,
        initial_state: Option<&str>,
    ) -> Result<u32, RepoError> {
        if let Some(state_str) = initial_state {
            SnState::from_str(state_str)
                .map_err(|_| RepoError::Query(format!("invalid initial_state: '{state_str}'")))?;
        }

        self.pool_repo
            .assign_to_pool(pool_id, epcs, initial_state)
            .await
    }

    /// Get aggregated statistics for a pool.
    pub async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError> {
        self.pool_repo.get_pool_stats(pool_id).await
    }

    /// Request serial numbers from the upstream ESM and assign them to a local pool.
    ///
    /// OPEN-SCS flow: ESM allocates SNs (Unassigned → Unallocated at SSM level),
    /// then we store them locally via `assign_to_pool`.
    pub async fn request_upstream(
        &self,
        pool_id: &PoolId,
        count: u32,
        criteria: &PoolSelectionCriteria,
    ) -> Result<PoolResponse, EsmError> {
        let epcs = self.esm_client.request_unassigned(count, criteria).await?;
        let fulfilled = epcs.len() as u32;

        self.pool_repo
            .assign_to_pool(pool_id, &epcs, None)
            .await
            .map_err(|e| EsmError::Connection(e.to_string()))?;

        Ok(PoolResponse {
            serial_numbers: epcs,
            pool_id: pool_id.clone(),
            fulfilled,
            requested: count,
        })
    }

    /// Return serial numbers from a local pool back to the upstream ESM.
    ///
    /// OPEN-SCS flow: SSM returns unused SNs → ESM marks as Unassigned,
    /// then we remove them from the local pool.
    ///
    /// Ordering: local state change first (retryable), then ESM call (commit step).
    /// If the ESM call fails, local state is already rolled back to unallocated
    /// which is safe — the SNs remain in the local pool for retry.
    pub async fn return_upstream(&self, pool_id: &PoolId, epcs: &[Epc]) -> Result<u32, EsmError> {
        self.pool_repo
            .return_numbers(pool_id, epcs)
            .await
            .map_err(|e| EsmError::Connection(e.to_string()))?;

        let count = self.esm_client.return_unallocated(epcs).await?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Epc, PoolId, PoolQuery, PoolSelectionCriteria, PoolStats, SerialNumberPool,
    };
    use crate::error::{EsmError, RepoError};
    use crate::repository::{EsmClient, PoolRepository};
    use std::sync::Mutex;

    /// In-memory stub for PoolRepository.
    struct StubPoolRepo {
        pools: Mutex<Vec<SerialNumberPool>>,
        allocated: Mutex<Vec<Epc>>,
    }

    impl StubPoolRepo {
        fn new() -> Self {
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

        fn with_pool_and_numbers(pool: SerialNumberPool, epcs: Vec<Epc>) -> Self {
            Self {
                pools: Mutex::new(vec![pool]),
                allocated: Mutex::new(epcs),
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
            let pools = self.pools.lock().unwrap();
            Ok(pools.iter().find(|p| p.id == *id).cloned())
        }

        async fn list_pools(
            &self,
            _filter: &PoolQuery,
        ) -> Result<Vec<SerialNumberPool>, RepoError> {
            Ok(self.pools.lock().unwrap().clone())
        }

        async fn delete_pool(&self, id: &PoolId) -> Result<(), RepoError> {
            let mut pools = self.pools.lock().unwrap();
            pools.retain(|p| p.id != *id);
            Ok(())
        }

        async fn assign_to_pool(
            &self,
            _pool_id: &PoolId,
            epcs: &[Epc],
            _initial_state: Option<&str>,
        ) -> Result<u32, RepoError> {
            let mut allocated = self.allocated.lock().unwrap();
            let count = epcs.len() as u32;
            allocated.extend_from_slice(epcs);
            Ok(count)
        }

        async fn request_numbers(
            &self,
            _pool_id: &PoolId,
            count: u32,
        ) -> Result<Vec<Epc>, RepoError> {
            let allocated = self.allocated.lock().unwrap();
            let result: Vec<Epc> = allocated.iter().take(count as usize).cloned().collect();
            Ok(result)
        }

        async fn return_numbers(&self, _pool_id: &PoolId, epcs: &[Epc]) -> Result<u32, RepoError> {
            Ok(epcs.len() as u32)
        }

        async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError> {
            let allocated = self.allocated.lock().unwrap();
            Ok(PoolStats {
                pool_id: pool_id.clone(),
                total: allocated.len() as u64,
                unassigned: 0,
                unallocated: 0,
                allocated: allocated.len() as u64,
                encoded: 0,
                commissioned: 0,
                other: 0,
            })
        }
    }

    /// In-memory stub for EsmClient that returns mock data.
    struct StubEsmClient;

    impl EsmClient for StubEsmClient {
        async fn request_unassigned(
            &self,
            count: u32,
            _criteria: &PoolSelectionCriteria,
        ) -> Result<Vec<Epc>, EsmError> {
            let epcs: Vec<Epc> = (0..count)
                .map(|i| Epc::new(format!("urn:epc:id:sgtin:0614141.107346.esm{i}")))
                .collect();
            Ok(epcs)
        }

        async fn return_unallocated(&self, epcs: &[Epc]) -> Result<u32, EsmError> {
            Ok(epcs.len() as u32)
        }
    }

    fn make_pool() -> SerialNumberPool {
        let now = chrono::Utc::now().fixed_offset();
        SerialNumberPool {
            id: PoolId::new(),
            name: "Test Pool".to_string(),
            sid_class: Some("sgtin".to_string()),
            criteria: PoolSelectionCriteria::default(),
            esm_endpoint: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_epcs(count: u32) -> Vec<Epc> {
        (0..count)
            .map(|i| Epc::new(format!("urn:epc:id:sgtin:0614141.107346.{i}")))
            .collect()
    }

    #[tokio::test]
    async fn test_create_pool() {
        let repo = StubPoolRepo::new();
        let service = PoolService::new(repo, StubEsmClient);
        let pool = make_pool();

        let id = service.create_pool(&pool).await.unwrap();
        assert_eq!(id, pool.id);
    }

    #[tokio::test]
    async fn test_get_pool_found() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let repo = StubPoolRepo::with_pool(pool);
        let service = PoolService::new(repo, StubEsmClient);

        let result = service.get_pool(&pool_id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, pool_id);
    }

    #[tokio::test]
    async fn test_get_pool_not_found() {
        let repo = StubPoolRepo::new();
        let service = PoolService::new(repo, StubEsmClient);

        let result = service.get_pool(&PoolId::new()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_request_numbers() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let epcs = make_epcs(5);
        let repo = StubPoolRepo::with_pool_and_numbers(pool, epcs);
        let service = PoolService::new(repo, StubEsmClient);

        let response = service.request_numbers(&pool_id, 3).await.unwrap();
        assert_eq!(response.requested, 3);
        assert_eq!(response.fulfilled, 3);
        assert_eq!(response.serial_numbers.len(), 3);
        assert_eq!(response.pool_id, pool_id);
    }

    #[tokio::test]
    async fn test_return_numbers() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let epcs = make_epcs(3);
        let repo = StubPoolRepo::with_pool(pool);
        let service = PoolService::new(repo, StubEsmClient);

        let count = service.return_numbers(&pool_id, &epcs).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_receive_numbers() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let epcs = make_epcs(4);
        let repo = StubPoolRepo::with_pool(pool);
        let service = PoolService::new(repo, StubEsmClient);

        let count = service
            .receive_numbers(&pool_id, &epcs, None, None)
            .await
            .unwrap();
        assert_eq!(count, 4);
    }

    #[tokio::test]
    async fn test_request_upstream() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let repo = StubPoolRepo::with_pool(pool);
        let service = PoolService::new(repo, StubEsmClient);

        let response = service
            .request_upstream(&pool_id, 5, &PoolSelectionCriteria::default())
            .await
            .unwrap();
        assert_eq!(response.requested, 5);
        assert_eq!(response.fulfilled, 5);
        assert_eq!(response.serial_numbers.len(), 5);
        assert_eq!(response.pool_id, pool_id);
    }

    #[tokio::test]
    async fn test_return_upstream() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let epcs = make_epcs(3);
        let repo = StubPoolRepo::with_pool(pool);
        let service = PoolService::new(repo, StubEsmClient);

        let count = service.return_upstream(&pool_id, &epcs).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_receive_rejects_invalid_initial_state() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let epcs = make_epcs(2);
        let repo = StubPoolRepo::with_pool(pool);
        let service = PoolService::new(repo, StubEsmClient);

        let result = service
            .receive_numbers(&pool_id, &epcs, None, Some("GARBAGE"))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid initial_state"), "got: {err}");
    }

    #[tokio::test]
    async fn test_receive_accepts_valid_initial_state() {
        let pool = make_pool();
        let pool_id = pool.id.clone();
        let epcs = make_epcs(2);
        let repo = StubPoolRepo::with_pool(pool);
        let service = PoolService::new(repo, StubEsmClient);

        let count = service
            .receive_numbers(&pool_id, &epcs, None, Some("unallocated"))
            .await
            .unwrap();
        assert_eq!(count, 2);
    }
}

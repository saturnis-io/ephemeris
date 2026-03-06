use crate::domain::{Epc, PoolId, PoolQuery, PoolStats, SerialNumberPool};
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

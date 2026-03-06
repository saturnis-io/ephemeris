use crate::domain::{Epc, PoolSelectionCriteria};
use crate::error::EsmError;
use crate::repository::EsmClient;

/// No-op ESM client for deployments without upstream ESM connectivity.
/// All upstream operations return `EsmError::NotConfigured`.
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

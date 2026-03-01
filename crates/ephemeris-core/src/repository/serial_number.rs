use crate::domain::{Epc, SerialNumber, SerialNumberQuery, SnState, SnTransition};
use crate::error::RepoError;

/// Repository for serial number state tracking and audit history.
///
/// Implementations store current SN state and a transition audit log.
/// Business logic (validation, state machine) lives in SerialNumberService, not here.
#[trait_variant::make(Send)]
pub trait SerialNumberRepository: Sync {
    /// Upsert a serial number's current state.
    /// Creates the record on first encounter, updates on subsequent calls.
    async fn upsert_state(
        &self,
        epc: &Epc,
        state: SnState,
        sid_class: Option<&str>,
        pool_id: Option<&str>,
    ) -> Result<(), RepoError>;

    /// Get current state of a serial number. Returns None if never tracked.
    async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError>;

    /// Query serial numbers with filters.
    async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError>;

    /// Record a state transition in the audit log.
    async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError>;

    /// Get transition history for an EPC, newest-first.
    async fn get_history(&self, epc: &Epc, limit: u32) -> Result<Vec<SnTransition>, RepoError>;
}

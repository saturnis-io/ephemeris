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

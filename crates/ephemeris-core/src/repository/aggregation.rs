use crate::domain::{AggregationTree, Epc, EventId};
use crate::error::RepoError;

/// Repository for managing the packaging aggregation hierarchy.
///
/// Models parent-child relationships (Pallet -> Case -> Carton -> Unit).
/// The event_id links each relationship back to the EPCIS AggregationEvent that created it.
#[trait_variant::make(Send)]
pub trait AggregationRepository: Sync {
    /// Record that parent contains child, linked to the source event.
    async fn add_child(
        &self,
        parent: &Epc,
        child: &Epc,
        event_id: &EventId,
    ) -> Result<(), RepoError>;

    /// Remove a child from its parent (for disaggregation/unpack events).
    async fn remove_child(&self, parent: &Epc, child: &Epc) -> Result<(), RepoError>;

    /// Get direct children of a parent.
    async fn get_children(&self, parent: &Epc) -> Result<Vec<Epc>, RepoError>;

    /// Get all ancestors of a child, from immediate parent to root.
    async fn get_ancestors(&self, child: &Epc) -> Result<Vec<Epc>, RepoError>;

    /// Get the full hierarchy tree rooted at the given EPC.
    async fn get_full_hierarchy(&self, root: &Epc) -> Result<AggregationTree, RepoError>;
}

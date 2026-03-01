use crate::domain::{EpcisEvent, EventId, EventQuery};
use crate::error::RepoError;

/// Repository for storing and querying EPCIS events.
///
/// Implementations must be idempotent on store_event — duplicate
/// event IDs should be silently ignored (return existing EventId).
#[trait_variant::make(Send)]
pub trait EventRepository: Sync {
    /// Store an EPCIS event. Returns the assigned EventId.
    /// Idempotent: duplicate event_id returns Ok with existing id.
    async fn store_event(&self, event: &EpcisEvent) -> Result<EventId, RepoError>;

    /// Retrieve a single event by its ID.
    async fn get_event(&self, id: &EventId) -> Result<Option<EpcisEvent>, RepoError>;

    /// Query events with filters. Returns matching events.
    async fn query_events(&self, query: &EventQuery) -> Result<Vec<EpcisEvent>, RepoError>;
}

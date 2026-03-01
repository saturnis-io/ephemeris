use ephemeris_core::repository::{AggregationRepository, EventRepository};

/// Shared application state holding repository implementations.
///
/// Generic over the concrete repository types to allow different backends
/// (PostgreSQL, ArangoDB, in-memory mocks) to be injected at startup.
pub struct AppState<E: EventRepository, A: AggregationRepository> {
    pub event_repo: E,
    pub agg_repo: A,
}

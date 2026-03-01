use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};
use ephemeris_core::service::SerialNumberService;

/// Shared application state holding repository implementations and services.
pub struct AppState<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository> {
    pub event_repo: E,
    pub agg_repo: A,
    pub sn_service: SerialNumberService<S>,
}

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

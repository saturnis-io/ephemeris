pub mod routes;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use ephemeris_core::repository::{AggregationRepository, EventRepository};

use crate::routes::{events, health, hierarchy};
pub use crate::state::AppState;

/// Build the Axum router with all API routes.
///
/// The router is generic over the repository implementations, allowing
/// any backend (PostgreSQL, ArangoDB, in-memory) to be plugged in.
pub fn create_router<E, A>(state: Arc<AppState<E, A>>) -> Router
where
    E: EventRepository + 'static,
    A: AggregationRepository + 'static,
{
    Router::new()
        .route("/health", get(health::health_check))
        .route("/events", get(events::query_events::<E, A>))
        .route("/events", post(events::capture_event::<E, A>))
        .route("/events/{event_id}", get(events::get_event::<E, A>))
        .route(
            "/hierarchy/{epc}",
            get(hierarchy::get_full_hierarchy::<E, A>),
        )
        .route(
            "/hierarchy/{epc}/children",
            get(hierarchy::get_children::<E, A>),
        )
        .route(
            "/hierarchy/{epc}/ancestors",
            get(hierarchy::get_ancestors::<E, A>),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ephemeris_core::domain::{AggregationTree, Epc, EpcisEvent, EventId, EventQuery};
    use ephemeris_core::error::RepoError;
    use tower::ServiceExt;

    /// In-memory stub that implements the repository traits for testing.
    struct StubEventRepo;

    impl ephemeris_core::repository::EventRepository for StubEventRepo {
        async fn store_event(&self, _event: &EpcisEvent) -> Result<EventId, RepoError> {
            Ok(EventId::new())
        }

        async fn get_event(&self, _id: &EventId) -> Result<Option<EpcisEvent>, RepoError> {
            Ok(None)
        }

        async fn query_events(&self, _query: &EventQuery) -> Result<Vec<EpcisEvent>, RepoError> {
            Ok(vec![])
        }
    }

    struct StubAggRepo;

    impl ephemeris_core::repository::AggregationRepository for StubAggRepo {
        async fn add_child(
            &self,
            _parent: &Epc,
            _child: &Epc,
            _event_id: &EventId,
        ) -> Result<(), RepoError> {
            Ok(())
        }

        async fn remove_child(&self, _parent: &Epc, _child: &Epc) -> Result<(), RepoError> {
            Ok(())
        }

        async fn get_children(&self, _parent: &Epc) -> Result<Vec<Epc>, RepoError> {
            Ok(vec![])
        }

        async fn get_ancestors(&self, _child: &Epc) -> Result<Vec<Epc>, RepoError> {
            Ok(vec![])
        }

        async fn get_full_hierarchy(&self, root: &Epc) -> Result<AggregationTree, RepoError> {
            Ok(AggregationTree {
                root: root.clone(),
                nodes: vec![],
            })
        }
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"status": "ok"}));
    }
}

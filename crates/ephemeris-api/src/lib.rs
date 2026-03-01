pub mod routes;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};

use crate::routes::{events, health, hierarchy, serial_numbers};
pub use crate::state::AppState;

/// Build the Axum router with all API routes.
pub fn create_router<E, A, S>(state: Arc<AppState<E, A, S>>) -> Router
where
    E: EventRepository + 'static,
    A: AggregationRepository + 'static,
    S: SerialNumberRepository + 'static,
{
    Router::new()
        .route("/health", get(health::health_check))
        .route("/events", get(events::query_events::<E, A, S>))
        .route("/events", post(events::capture_event::<E, A, S>))
        .route("/events/{event_id}", get(events::get_event::<E, A, S>))
        .route(
            "/hierarchy/{epc}",
            get(hierarchy::get_full_hierarchy::<E, A, S>),
        )
        .route(
            "/hierarchy/{epc}/children",
            get(hierarchy::get_children::<E, A, S>),
        )
        .route(
            "/hierarchy/{epc}/ancestors",
            get(hierarchy::get_ancestors::<E, A, S>),
        )
        .route(
            "/serial-numbers",
            get(serial_numbers::query_serial_numbers::<E, A, S>),
        )
        .route(
            "/serial-numbers/{epc}",
            get(serial_numbers::get_sn_state::<E, A, S>),
        )
        .route(
            "/serial-numbers/{epc}/history",
            get(serial_numbers::get_sn_history::<E, A, S>),
        )
        .route(
            "/serial-numbers/{epc}/transition",
            post(serial_numbers::manual_transition::<E, A, S>),
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
    use ephemeris_core::domain::{
        AggregationTree, Epc, EpcisEvent, EventId, EventQuery, SerialNumber, SerialNumberQuery,
        SnState, SnTransition,
    };
    use ephemeris_core::error::RepoError;
    use ephemeris_core::service::SerialNumberService;
    use tower::ServiceExt;

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

    struct StubSnRepo;

    impl ephemeris_core::repository::SerialNumberRepository for StubSnRepo {
        async fn upsert_state(
            &self,
            _epc: &Epc,
            _state: SnState,
            _sid_class: Option<&str>,
            _pool_id: Option<&str>,
        ) -> Result<(), RepoError> {
            Ok(())
        }

        async fn get_state(&self, _epc: &Epc) -> Result<Option<SerialNumber>, RepoError> {
            Ok(None)
        }

        async fn query(
            &self,
            _query: &SerialNumberQuery,
        ) -> Result<Vec<SerialNumber>, RepoError> {
            Ok(vec![])
        }

        async fn record_transition(&self, _transition: &SnTransition) -> Result<(), RepoError> {
            Ok(())
        }

        async fn get_history(
            &self,
            _epc: &Epc,
            _limit: u32,
        ) -> Result<Vec<SnTransition>, RepoError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(StubSnRepo),
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

    #[tokio::test]
    async fn sn_not_found_returns_404() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(StubSnRepo),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/serial-numbers/urn:epc:id:sgtin:0614141.107346.2017")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

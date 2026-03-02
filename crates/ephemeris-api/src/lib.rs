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

    use std::sync::Mutex;

    /// In-memory stub that supports upsert/get/query/transition for API tests.
    struct StubSnRepo {
        state: Mutex<std::collections::HashMap<String, SerialNumber>>,
        transitions: Mutex<Vec<SnTransition>>,
    }

    impl StubSnRepo {
        fn new() -> Self {
            Self {
                state: Mutex::new(std::collections::HashMap::new()),
                transitions: Mutex::new(Vec::new()),
            }
        }
    }

    impl ephemeris_core::repository::SerialNumberRepository for StubSnRepo {
        async fn upsert_state(
            &self,
            epc: &Epc,
            state: SnState,
            sid_class: Option<&str>,
            pool_id: Option<&str>,
        ) -> Result<(), RepoError> {
            let now = chrono::Utc::now().fixed_offset();
            let mut map = self.state.lock().unwrap();
            let existing = map.get(epc.as_str());
            let sn = SerialNumber {
                epc: epc.clone(),
                state,
                sid_class: sid_class
                    .map(String::from)
                    .or_else(|| existing.and_then(|s| s.sid_class.clone())),
                pool_id: pool_id
                    .map(String::from)
                    .or_else(|| existing.and_then(|s| s.pool_id.clone())),
                created_at: existing.map(|s| s.created_at).unwrap_or(now),
                updated_at: now,
            };
            map.insert(epc.as_str().to_string(), sn);
            Ok(())
        }

        async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError> {
            Ok(self.state.lock().unwrap().get(epc.as_str()).cloned())
        }

        async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError> {
            let map = self.state.lock().unwrap();
            let results: Vec<SerialNumber> = map
                .values()
                .filter(|sn| query.state.as_ref().is_none_or(|s| sn.state == *s))
                .cloned()
                .collect();
            Ok(results)
        }

        async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError> {
            self.transitions.lock().unwrap().push(transition.clone());
            Ok(())
        }

        async fn get_history(&self, epc: &Epc, limit: u32) -> Result<Vec<SnTransition>, RepoError> {
            let transitions = self.transitions.lock().unwrap();
            let results: Vec<SnTransition> = transitions
                .iter()
                .filter(|t| t.epc == *epc)
                .rev()
                .take(limit as usize)
                .cloned()
                .collect();
            Ok(results)
        }
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(StubSnRepo::new()),
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
            sn_service: SerialNumberService::new(StubSnRepo::new()),
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

    #[tokio::test]
    async fn query_serial_numbers_by_state() {
        let sn_repo = StubSnRepo::new();
        // Pre-populate some state
        sn_repo
            .upsert_state(
                &Epc::new("urn:epc:id:sgtin:0614141.107346.001"),
                SnState::Commissioned,
                None,
                None,
            )
            .await
            .unwrap();
        sn_repo
            .upsert_state(
                &Epc::new("urn:epc:id:sgtin:0614141.107346.002"),
                SnState::Released,
                None,
                None,
            )
            .await
            .unwrap();

        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(sn_repo),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/serial-numbers?state=commissioned")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["state"], "commissioned");
    }

    #[tokio::test]
    async fn get_sn_history_returns_transitions() {
        use ephemeris_core::domain::TransitionSource;

        let sn_repo = StubSnRepo::new();
        // Record a transition directly
        sn_repo
            .record_transition(&SnTransition {
                epc: Epc::new("urn:epc:id:sgtin:0614141.107346.001"),
                from_state: SnState::Encoded,
                to_state: SnState::Commissioned,
                biz_step: "commissioning".to_string(),
                event_id: None,
                source: TransitionSource::Mqtt,
                timestamp: chrono::Utc::now().fixed_offset(),
            })
            .await
            .unwrap();

        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(sn_repo),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/serial-numbers/urn:epc:id:sgtin:0614141.107346.001/history")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let history: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["biz_step"], "commissioning");
    }

    #[tokio::test]
    async fn manual_transition_updates_state() {
        let sn_repo = StubSnRepo::new();
        // Pre-populate a commissioned SN
        sn_repo
            .upsert_state(
                &Epc::new("urn:epc:id:sgtin:0614141.107346.001"),
                SnState::Commissioned,
                None,
                None,
            )
            .await
            .unwrap();

        let state = Arc::new(AppState {
            event_repo: StubEventRepo,
            agg_repo: StubAggRepo,
            sn_service: SerialNumberService::new(sn_repo),
        });
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/serial-numbers/urn:epc:id:sgtin:0614141.107346.001/transition")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"targetState": "destroyed", "reason": "damaged on line"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["state"], "destroyed");
    }
}

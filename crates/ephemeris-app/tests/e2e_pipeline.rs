//! E2E Pipeline Test: Event ingestion → PostgreSQL → REST API
//!
//! This test proves the full Ephemeris pipeline works end-to-end:
//! 1. Create PostgreSQL via testcontainers
//! 2. Wire up PgEventRepository + PgAggregationRepository
//! 3. Process EPCIS events through the EventHandler (same path as MQTT)
//! 4. Query results through the REST API
//! 5. Verify events stored and aggregation hierarchy correct

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use ephemeris_api::{AppState, create_router};
use ephemeris_core::domain::{EpcisEvent, EventQuery};
use ephemeris_core::repository::{AggregationRepository, EventRepository};
use ephemeris_core::service::SerialNumberService;
use ephemeris_mqtt::EventHandler;
use ephemeris_pg::{PgAggregationRepository, PgEventRepository, PgSerialNumberRepository};

use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};
use tokio_postgres::NoTls;

async fn setup() -> (
    PgEventRepository,
    PgAggregationRepository,
    PgSerialNumberRepository,
    impl std::any::Any, // container handle
) {
    let container = Postgres::default().start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let conn_str = format!(
        "host={} port={} user=postgres password=postgres dbname=postgres",
        host, port
    );

    let event_repo = PgEventRepository::connect(&conn_str).await.unwrap();
    event_repo.run_migrations().await.unwrap();

    let mut cfg = Config::new();
    cfg.host = Some(host.to_string());
    cfg.port = Some(port);
    cfg.user = Some("postgres".to_string());
    cfg.password = Some("postgres".to_string());
    cfg.dbname = Some("postgres".to_string());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();
    let agg_repo = PgAggregationRepository::new(pool.clone());
    let sn_repo = PgSerialNumberRepository::new(pool);

    (event_repo, agg_repo, sn_repo, container)
}

#[tokio::test]
async fn e2e_object_event_ingest_and_query() {
    let (event_repo, agg_repo, sn_repo, _container) = setup().await;

    // 1. Ingest an ObjectEvent through the handler (same path as MQTT)
    let sn_service = SerialNumberService::new(sn_repo.clone());
    let handler = EventHandler::new(event_repo.clone(), agg_repo.clone(), sn_service);

    let object_event: EpcisEvent = serde_json::from_str(
        r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2024-09-15T14:30:00.000+02:00",
            "eventTimeZoneOffset": "+02:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"],
            "bizStep": "urn:epcglobal:cbv:bizstep:shipping"
        }"#,
    )
    .unwrap();

    handler.handle_event(&object_event).await.unwrap();

    // 2. Query via repository directly — verify stored
    let query = EventQuery {
        eq_biz_step: Some("urn:epcglobal:cbv:bizstep:shipping".to_string()),
        ..Default::default()
    };
    let results = event_repo.query_events(&query).await.unwrap();
    assert_eq!(results.len(), 1, "Expected 1 event stored");

    // 3. Query via REST API
    let state = Arc::new(AppState {
        event_repo: event_repo.clone(),
        agg_repo: agg_repo.clone(),
        sn_service: SerialNumberService::new(sn_repo.clone()),
    });
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/events?eq_biz_step=urn:epcglobal:cbv:bizstep:shipping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(events.len(), 1, "API should return 1 event");
}

#[tokio::test]
async fn e2e_aggregation_hierarchy_ingest_and_query() {
    let (event_repo, agg_repo, sn_repo, _container) = setup().await;

    let sn_service = SerialNumberService::new(sn_repo.clone());
    let handler = EventHandler::new(event_repo.clone(), agg_repo.clone(), sn_service);

    // 1. Ingest AggregationEvent: pallet contains 2 cases
    let agg_event: EpcisEvent = serde_json::from_str(
        r#"{
            "type": "AggregationEvent",
            "action": "ADD",
            "eventTime": "2024-09-15T15:00:00.000+02:00",
            "eventTimeZoneOffset": "+02:00",
            "parentID": "urn:epc:id:sscc:0614141.P001",
            "childEPCs": [
                "urn:epc:id:sscc:0614141.C001",
                "urn:epc:id:sscc:0614141.C002"
            ]
        }"#,
    )
    .unwrap();

    handler.handle_event(&agg_event).await.unwrap();

    // 2. Ingest another AggregationEvent: case1 contains 2 units
    let agg_event2: EpcisEvent = serde_json::from_str(
        r#"{
            "type": "AggregationEvent",
            "action": "ADD",
            "eventTime": "2024-09-15T15:01:00.000+02:00",
            "eventTimeZoneOffset": "+02:00",
            "parentID": "urn:epc:id:sscc:0614141.C001",
            "childEPCs": [
                "urn:epc:id:sgtin:0614141.107346.001",
                "urn:epc:id:sgtin:0614141.107346.002"
            ]
        }"#,
    )
    .unwrap();

    handler.handle_event(&agg_event2).await.unwrap();

    // 3. Verify hierarchy via repository
    let children = agg_repo
        .get_children(&ephemeris_core::domain::Epc::new(
            "urn:epc:id:sscc:0614141.P001",
        ))
        .await
        .unwrap();
    assert_eq!(children.len(), 2, "Pallet should have 2 cases");

    let ancestors = agg_repo
        .get_ancestors(&ephemeris_core::domain::Epc::new(
            "urn:epc:id:sgtin:0614141.107346.001",
        ))
        .await
        .unwrap();
    assert_eq!(
        ancestors.len(),
        2,
        "Unit should have 2 ancestors (case + pallet)"
    );

    // 4. Verify hierarchy via REST API
    let state = Arc::new(AppState {
        event_repo: event_repo.clone(),
        agg_repo: agg_repo.clone(),
        sn_service: SerialNumberService::new(sn_repo),
    });
    let app = create_router(state.clone());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/hierarchy/urn:epc:id:sscc:0614141.P001")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let tree: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify tree structure: root has nodes (cases), and first case has children (units)
    let nodes = tree["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2, "Tree should have 2 top-level nodes (cases)");

    // Find case C001 and verify it has 2 children
    let case1 = nodes
        .iter()
        .find(|n| n["epc"].as_str() == Some("urn:epc:id:sscc:0614141.C001"))
        .expect("Case C001 should be in hierarchy");
    let case1_children = case1["children"].as_array().unwrap();
    assert_eq!(case1_children.len(), 2, "Case C001 should have 2 units");

    // 5. Verify children endpoint via REST API
    let app2 = create_router(state.clone());
    let response = app2
        .oneshot(
            Request::builder()
                .uri("/hierarchy/urn:epc:id:sscc:0614141.P001/children")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let children_api: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        children_api.len(),
        2,
        "API children endpoint should return 2 cases"
    );

    // 6. Verify ancestors endpoint via REST API
    let app3 = create_router(state);
    let response = app3
        .oneshot(
            Request::builder()
                .uri("/hierarchy/urn:epc:id:sgtin:0614141.107346.001/ancestors")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let ancestors_api: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        ancestors_api.len(),
        2,
        "API ancestors endpoint should return 2 ancestors"
    );
}

#[tokio::test]
async fn e2e_event_capture_via_api() {
    let (event_repo, agg_repo, sn_repo, _container) = setup().await;

    let state = Arc::new(AppState {
        event_repo: event_repo.clone(),
        agg_repo: agg_repo.clone(),
        sn_service: SerialNumberService::new(sn_repo),
    });
    let app = create_router(state.clone());

    // 1. POST an event through the REST API
    let event_json = r#"{
        "type": "ObjectEvent",
        "action": "OBSERVE",
        "eventTime": "2024-09-15T16:00:00.000+00:00",
        "eventTimeZoneOffset": "+00:00",
        "epcList": ["urn:epc:id:sgtin:0614141.107346.9999"]
    }"#;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/events")
                .header("content-type", "application/json")
                .body(Body::from(event_json))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    // 2. Verify it was stored via direct query
    let all = event_repo
        .query_events(&EventQuery::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 1, "Event should be stored via API capture");
}

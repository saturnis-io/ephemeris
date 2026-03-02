use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use ephemeris_core::domain::{Action, Epc, EpcisEvent, EventId, EventQuery, TransitionSource};
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};

use crate::state::AppState;

/// GET /events — query events with optional filters.
pub async fn query_events<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Query(query): Query<EventQuery>,
) -> Result<Json<Vec<EpcisEvent>>, (StatusCode, Json<Value>)> {
    state
        .event_repo
        .query_events(&query)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("Failed to query events: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// GET /events/:event_id — retrieve a single event by UUID.
pub async fn get_event<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(event_id): Path<Uuid>,
) -> Result<Json<EpcisEvent>, (StatusCode, Json<Value>)> {
    let id = EventId(event_id);
    match state.event_repo.get_event(&id).await {
        Ok(Some(event)) => Ok(Json(event)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("event {event_id} not found")})),
        )),
        Err(e) => {
            tracing::error!("Failed to get event {event_id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// POST /events — capture a new EPCIS event.
///
/// Stores the event, then processes aggregation hierarchy and SN state
/// transitions — the same pipeline as the MQTT ingestion path.
pub async fn capture_event<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Json(event): Json<EpcisEvent>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let stored_id = state.event_repo.store_event(&event).await.map_err(|e| {
        tracing::error!("Failed to capture event: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    // Process aggregation hierarchy
    if let EpcisEvent::AggregationEvent(data) = &event
        && let Some(ref parent_id_str) = data.parent_id
    {
        let parent = Epc::new(parent_id_str);
        match data.action {
            Action::Add | Action::Observe => {
                for child_epc_str in &data.child_epcs {
                    let child = Epc::new(child_epc_str);
                    if let Err(e) = state.agg_repo.add_child(&parent, &child, &stored_id).await {
                        tracing::warn!(parent = %parent, child = %child, error = %e, "failed to add child");
                    }
                }
            }
            Action::Delete => {
                for child_epc_str in &data.child_epcs {
                    let child = Epc::new(child_epc_str);
                    if let Err(e) = state.agg_repo.remove_child(&parent, &child).await {
                        tracing::warn!(parent = %parent, child = %child, error = %e, "failed to remove child");
                    }
                }
            }
        }
    }

    // Drive SN state transitions from bizStep
    if let Some(biz_step) = event.common().biz_step.as_deref() {
        let epcs = extract_epcs(&event);
        for epc in epcs {
            if let Err(e) = state
                .sn_service
                .process_transition(&epc, biz_step, Some(&stored_id), TransitionSource::RestApi)
                .await
            {
                tracing::warn!(epc = %epc, error = %e, "failed to update SN state");
            }
        }
    }

    Ok((StatusCode::CREATED, Json(json!({"eventId": stored_id}))))
}

/// Extract all EPCs from an event for SN state tracking.
fn extract_epcs(event: &EpcisEvent) -> Vec<Epc> {
    match event {
        EpcisEvent::ObjectEvent(data) => data.epc_list.iter().map(Epc::new).collect(),
        EpcisEvent::AggregationEvent(data) => {
            let mut epcs: Vec<Epc> = data.child_epcs.iter().map(Epc::new).collect();
            if let Some(ref parent) = data.parent_id {
                epcs.push(Epc::new(parent));
            }
            epcs
        }
        EpcisEvent::TransformationEvent(data) => {
            let mut epcs: Vec<Epc> = data.input_epc_list.iter().map(Epc::new).collect();
            epcs.extend(data.output_epc_list.iter().map(Epc::new));
            epcs
        }
    }
}

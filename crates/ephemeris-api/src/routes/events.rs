use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use ephemeris_core::domain::{EpcisEvent, EventId, EventQuery};
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};

use crate::state::AppState;

/// GET /events — query events with optional filters.
pub async fn query_events<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository>(
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
pub async fn capture_event<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Json(event): Json<EpcisEvent>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    match state.event_repo.store_event(&event).await {
        Ok(event_id) => Ok((StatusCode::CREATED, Json(json!({"eventId": event_id})))),
        Err(e) => {
            tracing::error!("Failed to capture event: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

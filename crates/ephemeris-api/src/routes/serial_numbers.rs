use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use ephemeris_core::domain::{Epc, SerialNumberQuery, SnState};
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};

/// Query parameters for history pagination.
#[derive(Deserialize)]
pub struct HistoryParams {
    pub limit: Option<u32>,
}

use crate::state::AppState;

/// GET /serial-numbers/{epc} — get current SN state.
pub async fn get_sn_state<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    match state.sn_service.get_state(&epc).await {
        Ok(Some(sn)) => Ok(Json(serde_json::to_value(sn).unwrap())),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "serial number not tracked"})),
        )),
        Err(e) => {
            tracing::error!("Failed to get SN state: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

/// GET /serial-numbers/{epc}/history — get transition audit trail.
pub async fn get_sn_history<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    let limit = params.limit.unwrap_or(100);
    state
        .sn_service
        .get_history(&epc, limit)
        .await
        .map(|h| Json(serde_json::to_value(h).unwrap()))
        .map_err(|e| {
            tracing::error!("Failed to get SN history: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// GET /serial-numbers — query serial numbers by state/filters.
pub async fn query_serial_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Query(query): Query<SerialNumberQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .sn_service
        .query(&query)
        .await
        .map(|sns| Json(serde_json::to_value(sns).unwrap()))
        .map_err(|e| {
            tracing::error!("Failed to query serial numbers: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// POST body for manual state override.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionRequest {
    pub target_state: SnState,
    #[serde(default)]
    pub reason: String,
    pub sid_class: Option<String>,
    pub pool_id: Option<String>,
}

/// POST /serial-numbers/{epc}/transition — manual state override.
pub async fn manual_transition<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
    Json(req): Json<TransitionRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    match state
        .sn_service
        .manual_override(
            &epc,
            req.target_state,
            &req.reason,
            req.sid_class.as_deref(),
            req.pool_id.as_deref(),
        )
        .await
    {
        Ok(new_state) => Ok((
            StatusCode::OK,
            Json(json!({"epc": epc.as_str(), "state": new_state.to_string()})),
        )),
        Err(e) => {
            tracing::error!("Failed to override SN state: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    }
}

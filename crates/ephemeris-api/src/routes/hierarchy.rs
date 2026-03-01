use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use ephemeris_core::domain::{AggregationTree, Epc};
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};

use crate::state::AppState;

/// GET /hierarchy/:epc — get the full aggregation tree rooted at the given EPC.
pub async fn get_full_hierarchy<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
) -> Result<Json<AggregationTree>, (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    state
        .agg_repo
        .get_full_hierarchy(&epc)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("Failed to get hierarchy for {epc}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// GET /hierarchy/:epc/children — get direct children of the given EPC.
pub async fn get_children<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
) -> Result<Json<Vec<Epc>>, (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    state
        .agg_repo
        .get_children(&epc)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("Failed to get children for {epc}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

/// GET /hierarchy/:epc/ancestors — get all ancestors of the given EPC.
pub async fn get_ancestors<E: EventRepository, A: AggregationRepository, S: SerialNumberRepository>(
    State(state): State<Arc<AppState<E, A, S>>>,
    Path(epc): Path<String>,
) -> Result<Json<Vec<Epc>>, (StatusCode, Json<Value>)> {
    let epc = Epc::new(epc);
    state
        .agg_repo
        .get_ancestors(&epc)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("Failed to get ancestors for {epc}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })
}

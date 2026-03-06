use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use ephemeris_core::domain::{
    PoolCriterionKey, PoolId, PoolQuery, PoolReceiveRequest, PoolRequest, PoolReturnRequest,
    PoolSelectionCriteria, SerialNumberPool,
};
use ephemeris_core::error::EsmError;
use ephemeris_core::repository::{
    AggregationRepository, EsmClient, EventRepository, PoolRepository, SerialNumberRepository,
};

use crate::state::AppState;

/// Request body for creating a new pool.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePoolRequest {
    pub name: String,
    pub sid_class: Option<String>,
    pub criteria: Option<Vec<(PoolCriterionKey, String)>>,
    pub esm_endpoint: Option<String>,
}

/// POST /pools — create a new serial number pool.
pub async fn create_pool<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let now = chrono::Utc::now().fixed_offset();
    let pool = SerialNumberPool {
        id: PoolId::new(),
        name: req.name,
        sid_class: req.sid_class,
        criteria: PoolSelectionCriteria {
            criteria: req.criteria.unwrap_or_default(),
        },
        esm_endpoint: req.esm_endpoint,
        created_at: now,
        updated_at: now,
    };

    match state.pool_service.create_pool(&pool).await {
        Ok(id) => Ok((StatusCode::CREATED, Json(json!({"poolId": id.0})))),
        Err(e) => {
            tracing::error!("Failed to create pool: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to create pool"})),
            ))
        }
    }
}

/// GET /pools — list pools with optional filters.
pub async fn list_pools<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Query(query): Query<PoolQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .pool_service
        .list_pools(&query)
        .await
        .map(|pools| Json(serde_json::to_value(pools).unwrap()))
        .map_err(|e| {
            tracing::error!("Failed to list pools: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to list pools"})),
            )
        })
}

/// GET /pools/{id} — get a single pool by ID, including stats.
pub async fn get_pool<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state.pool_service.get_pool(&pool_id).await {
        Ok(Some(pool)) => {
            let stats = state.pool_service.get_pool_stats(&pool_id).await.ok();
            let mut value = serde_json::to_value(pool).unwrap();
            if let Some(stats) = stats {
                value["stats"] = serde_json::to_value(stats).unwrap();
            }
            Ok(Json(value))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("pool {id} not found")})),
        )),
        Err(e) => {
            tracing::error!("Failed to get pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to get pool"})),
            ))
        }
    }
}

/// DELETE /pools/{id} — delete an empty pool.
pub async fn delete_pool<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state.pool_service.delete_pool(&pool_id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("still assigned") {
                Err((StatusCode::CONFLICT, Json(json!({"error": msg}))))
            } else {
                tracing::error!("Failed to delete pool {id}: {e}");
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "failed to delete pool"})),
                ))
            }
        }
    }
}

/// POST /pools/{id}/request — allocate serial numbers from a pool.
pub async fn request_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .request_numbers(&pool_id, req.count)
        .await
    {
        Ok(response) => Ok(Json(serde_json::to_value(response).unwrap())),
        Err(e) => {
            tracing::error!("Failed to request numbers from pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to request numbers"})),
            ))
        }
    }
}

/// POST /pools/{id}/return — return serial numbers back to a pool.
pub async fn return_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolReturnRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .return_numbers(&pool_id, &req.serial_numbers)
        .await
    {
        Ok(count) => Ok(Json(json!({"poolId": id, "returned": count}))),
        Err(e) => {
            tracing::error!("Failed to return numbers to pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to return numbers"})),
            ))
        }
    }
}

/// POST /pools/{id}/receive — receive serial numbers into a pool.
pub async fn receive_numbers<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolReceiveRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .receive_numbers(
            &pool_id,
            &req.serial_numbers,
            req.sid_class.as_deref(),
            req.initial_state.as_deref(),
        )
        .await
    {
        Ok(count) => Ok(Json(json!({"poolId": id, "received": count}))),
        Err(e) => {
            tracing::error!("Failed to receive numbers into pool {id}: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to receive numbers"})),
            ))
        }
    }
}

/// POST /pools/{id}/request-upstream — request serial numbers from the upstream ESM.
pub async fn request_upstream<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .request_upstream(&pool_id, req.count, &req.criteria)
        .await
    {
        Ok(response) => Ok(Json(serde_json::to_value(response).unwrap())),
        Err(e) => {
            tracing::error!("Failed to request upstream for pool {id}: {e}");
            match &e {
                EsmError::NotConfigured => Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "ESM not configured"})),
                )),
                _ => Err((
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error": "upstream ESM request failed"})),
                )),
            }
        }
    }
}

/// POST /pools/{id}/return-upstream — return serial numbers to the upstream ESM.
pub async fn return_upstream<
    E: EventRepository,
    A: AggregationRepository,
    S: SerialNumberRepository,
    P: PoolRepository,
    C: EsmClient,
>(
    State(state): State<Arc<AppState<E, A, S, P, C>>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PoolReturnRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool_id = PoolId(id);
    match state
        .pool_service
        .return_upstream(&pool_id, &req.serial_numbers)
        .await
    {
        Ok(count) => Ok(Json(json!({"poolId": id, "returned": count}))),
        Err(e) => {
            tracing::error!("Failed to return upstream for pool {id}: {e}");
            match &e {
                EsmError::NotConfigured => Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "ESM not configured"})),
                )),
                _ => Err((
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error": "upstream ESM return failed"})),
                )),
            }
        }
    }
}

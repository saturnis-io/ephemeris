use axum::Json;
use serde_json::{Value, json};

/// GET /health — returns service health status.
pub async fn health_check() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

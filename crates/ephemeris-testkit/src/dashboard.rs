use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{AsyncClient, MqttOptions};
use std::sync::Arc;
use tracing::{error, info};

use ephemeris_core::domain::EpcisEvent;

/// Shared state for the dashboard server.
struct DashboardState {
    mqtt_client: AsyncClient,
    mqtt_topic: String,
    api_base_url: String,
}

/// Configuration for the dashboard server.
pub struct DashboardConfig {
    /// Address to bind the dashboard HTTP server (e.g., "0.0.0.0:3001").
    pub listen_addr: String,
    /// MQTT broker host.
    pub mqtt_host: String,
    /// MQTT broker port.
    pub mqtt_port: u16,
    /// MQTT topic to publish events to.
    pub mqtt_topic: String,
    /// Base URL of the Ephemeris REST API for the live feed (e.g., "http://localhost:3000").
    pub api_base_url: String,
}

/// Build the dashboard Axum router.
pub fn build_router(mqtt_client: AsyncClient, mqtt_topic: String, api_base_url: String) -> Router {
    let state = Arc::new(DashboardState {
        mqtt_client,
        mqtt_topic,
        api_base_url,
    });

    Router::new()
        .route("/", get(index_handler))
        .route("/send", post(send_handler))
        .route("/config", get(config_handler))
        .with_state(state)
}

/// Start the dashboard server. This blocks until the server shuts down.
pub async fn run(config: DashboardConfig) -> Result<(), Box<dyn std::error::Error>> {
    let client_id = format!("ephemeris-dashboard-{}", uuid::Uuid::new_v4());
    let mut options = MqttOptions::new(&client_id, &config.mqtt_host, config.mqtt_port);
    options.set_keep_alive(std::time::Duration::from_secs(30));

    let (client, mut eventloop) = AsyncClient::new(options, 100);

    let router = build_router(
        client,
        config.mqtt_topic.clone(),
        config.api_base_url.clone(),
    );

    // Spawn MQTT eventloop driver
    tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(_) => {}
                Err(e) => {
                    error!(error = %e, "dashboard MQTT eventloop error");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!(addr = %config.listen_addr, "dashboard server started");
    axum::serve(listener, router).await?;

    Ok(())
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn send_handler(State(state): State<Arc<DashboardState>>, body: String) -> impl IntoResponse {
    let event: EpcisEvent = match serde_json::from_str(&body) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid EPCIS event JSON: {e}"),
            );
        }
    };

    let payload = match serde_json::to_vec(&event) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Serialization error: {e}"),
            );
        }
    };

    match state
        .mqtt_client
        .publish(&state.mqtt_topic, QoS::AtLeastOnce, false, payload)
        .await
    {
        Ok(_) => (StatusCode::OK, "Event published to MQTT".to_string()),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("MQTT publish error: {e}"),
        ),
    }
}

async fn config_handler(State(state): State<Arc<DashboardState>>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "apiBaseUrl": state.api_base_url,
        "mqttTopic": state.mqtt_topic,
    }))
}

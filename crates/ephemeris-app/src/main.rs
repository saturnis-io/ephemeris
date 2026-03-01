use std::sync::Arc;

use clap::Parser;
#[allow(unused_imports)]
use tracing::{error, info};

mod config;
use config::{AppConfig, Cli};

use ephemeris_api::AppState;
use ephemeris_core::repository::{AggregationRepository, EventRepository, SerialNumberRepository};
use ephemeris_core::service::SerialNumberService;
use ephemeris_mqtt::{EventHandler, MqttSubscriber};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let app_config = AppConfig::load(&cli)?;

    info!(backend = %app_config.database.backend, "starting Ephemeris");

    match app_config.database.backend.as_str() {
        "postgres" => {
            let pg_cfg = app_config
                .database
                .postgres
                .as_ref()
                .expect("database.postgres config required when backend=postgres");

            // Convert URL to connection string format if needed
            let conn_str = pg_url_to_conn_str(&pg_cfg.url);

            let event_repo = ephemeris_pg::PgEventRepository::connect(&conn_str).await?;
            event_repo.run_migrations().await?;
            info!("PostgreSQL connected and migrations applied");

            // Build aggregation repo from the same connection parameters
            let agg_pool = build_pg_pool(&conn_str, pg_cfg.pool_size)?;
            let agg_repo = ephemeris_pg::PgAggregationRepository::new(agg_pool);

            let sn_pool = build_pg_pool(&conn_str, pg_cfg.pool_size)?;
            let sn_repo = ephemeris_pg::PgSerialNumberRepository::new(sn_pool);

            run_app(event_repo, agg_repo, sn_repo, app_config).await
        }
        #[cfg(feature = "enterprise-arango")]
        "arango" => {
            // Enterprise mode: PG for events, ArangoDB for aggregation
            let pg_cfg = app_config
                .database
                .postgres
                .as_ref()
                .expect("database.postgres config required (events always use PostgreSQL)");
            let arango_cfg = app_config
                .database
                .arango
                .as_ref()
                .expect("database.arango config required when backend=arango");

            let conn_str = pg_url_to_conn_str(&pg_cfg.url);
            let event_repo = ephemeris_pg::PgEventRepository::connect(&conn_str).await?;
            event_repo.run_migrations().await?;
            info!("PostgreSQL connected for event storage");

            let username = arango_cfg.username.as_deref().unwrap_or("root");
            let password = arango_cfg.password.as_deref().unwrap_or("");
            let arango_client = ephemeris_arango::ArangoClient::connect(
                &arango_cfg.url,
                &arango_cfg.database,
                username,
                password,
            )
            .await
            .map_err(|e| format!("ArangoDB connection failed: {e}"))?;
            info!("ArangoDB connected for aggregation");

            let agg_repo = ephemeris_arango::ArangoAggregationRepository::new(
                arango_client,
                "packaging_hierarchy".to_string(),
            );

            let sn_pool = build_pg_pool(&conn_str, pg_cfg.pool_size)?;
            let sn_repo = ephemeris_pg::PgSerialNumberRepository::new(sn_pool);

            run_app(event_repo, agg_repo, sn_repo, app_config).await
        }
        other => {
            #[cfg(not(feature = "enterprise-arango"))]
            if other == "arango" {
                error!("ArangoDB backend requires the enterprise build");
                eprintln!("ERROR: ArangoDB backend requires the enterprise build.");
                eprintln!("This binary was compiled without enterprise features.");
                eprintln!("Contact sales@saturnis.io for enterprise licensing.");
                std::process::exit(1);
            }
            eprintln!("Unknown database backend: {other}");
            std::process::exit(1);
        }
    }
}

async fn run_app<E, A, S>(
    event_repo: E,
    agg_repo: A,
    sn_repo: S,
    app_config: AppConfig,
) -> Result<(), Box<dyn std::error::Error>>
where
    E: EventRepository + Clone + 'static,
    A: AggregationRepository + Clone + 'static,
    S: SerialNumberRepository + Clone + 'static,
{
    let sn_service = SerialNumberService::new(sn_repo.clone());

    let state = Arc::new(AppState {
        event_repo: event_repo.clone(),
        agg_repo: agg_repo.clone(),
        sn_service: SerialNumberService::new(sn_repo),
    });

    // Build API router
    let router = ephemeris_api::create_router(state);
    let api_bind = app_config.api.bind.clone();

    // Set up MQTT subscriber
    let mqtt_config = app_config.mqtt;
    let parts: Vec<&str> = mqtt_config.broker_url.split("://").collect();
    let host_port = if parts.len() == 2 { parts[1] } else { parts[0] };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(1883)),
        None => (host_port, 1883),
    };

    let subscriber = MqttSubscriber::new(host, port, &mqtt_config.client_id);
    subscriber
        .subscribe(&mqtt_config.topics)
        .await
        .map_err(|e| format!("MQTT subscribe failed: {e}"))?;
    info!(topics = ?mqtt_config.topics, "MQTT subscriber started");

    let handler = EventHandler::new(event_repo, agg_repo, sn_service);

    // Run MQTT event loop in background task
    let mqtt_handle = tokio::spawn(async move {
        subscriber.run(handler).await;
    });

    // Start API server
    let listener = tokio::net::TcpListener::bind(&api_bind).await?;
    info!(bind = %api_bind, "API server listening");

    axum::serve(listener, router).await?;

    // If API server stops, abort MQTT task
    mqtt_handle.abort();
    Ok(())
}

/// Convert a PostgreSQL URL to a key=value connection string.
/// Accepts both `postgresql://user:pass@host:port/db` and `host=... port=...` formats.
fn pg_url_to_conn_str(url: &str) -> String {
    if url.starts_with("postgresql://") || url.starts_with("postgres://") {
        // Parse URL format
        let without_scheme = url
            .strip_prefix("postgresql://")
            .or_else(|| url.strip_prefix("postgres://"))
            .unwrap();

        let (userinfo, rest) = without_scheme
            .split_once('@')
            .unwrap_or(("postgres:postgres", without_scheme));
        let (user, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
        let (host_port, dbname) = rest.split_once('/').unwrap_or((rest, "postgres"));
        let (host, port) = host_port.split_once(':').unwrap_or((host_port, "5432"));

        format!("host={host} port={port} user={user} password={password} dbname={dbname}")
    } else {
        // Already in key=value format
        url.to_string()
    }
}

/// Build a deadpool-postgres pool from a key=value connection string.
fn build_pg_pool(
    conn_str: &str,
    pool_size: Option<u32>,
) -> Result<deadpool_postgres::Pool, Box<dyn std::error::Error>> {
    let mut cfg = deadpool_postgres::Config::new();
    for part in conn_str.split_whitespace() {
        if let Some((key, val)) = part.split_once('=') {
            match key {
                "host" => cfg.host = Some(val.to_string()),
                "port" => cfg.port = val.parse().ok(),
                "user" => cfg.user = Some(val.to_string()),
                "password" => cfg.password = Some(val.to_string()),
                "dbname" => cfg.dbname = Some(val.to_string()),
                _ => {}
            }
        }
    }
    cfg.manager = Some(deadpool_postgres::ManagerConfig {
        recycling_method: deadpool_postgres::RecyclingMethod::Fast,
    });
    if let Some(size) = pool_size {
        cfg.pool = Some(deadpool_postgres::PoolConfig::new(size as usize));
    }

    Ok(cfg.create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?)
}

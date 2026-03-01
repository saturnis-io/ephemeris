use clap::Parser;
use config::{Config, Environment, File};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    name = "ephemeris",
    version,
    about = "Saturnis Ephemeris — Track & Trace Engine"
)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "ephemeris.toml")]
    pub config: String,

    /// Database backend (postgres, arango)
    #[arg(long)]
    pub database_backend: Option<String>,

    /// API bind address (e.g. 0.0.0.0:8080)
    #[arg(long)]
    pub api_bind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub mqtt: MqttConfig,
    pub database: DatabaseConfig,
    pub api: ApiConfig,
}

#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    pub broker_url: String,
    pub client_id: String,
    pub topics: Vec<String>,
    #[allow(dead_code)]
    pub qos: u8,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub backend: String,
    pub postgres: Option<PostgresConfig>,
    #[cfg(feature = "enterprise-arango")]
    pub arango: Option<ArangoConfig>,
}

#[derive(Debug, Deserialize)]
pub struct PostgresConfig {
    pub url: String,
    pub pool_size: Option<u32>,
}

#[cfg(feature = "enterprise-arango")]
#[derive(Debug, Deserialize)]
pub struct ArangoConfig {
    pub url: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiConfig {
    pub bind: String,
}

impl AppConfig {
    pub fn load(cli: &Cli) -> Result<Self, config::ConfigError> {
        let mut builder = Config::builder()
            .add_source(File::with_name(&cli.config).required(false))
            .add_source(Environment::with_prefix("EPHEMERIS").separator("__"));

        // Apply CLI overrides
        if let Some(ref backend) = cli.database_backend {
            builder = builder.set_override("database.backend", backend.as_str())?;
        }
        if let Some(ref bind) = cli.api_bind {
            builder = builder.set_override("api.bind", bind.as_str())?;
        }

        builder.build()?.try_deserialize()
    }
}

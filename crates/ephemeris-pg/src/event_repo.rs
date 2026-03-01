use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use ephemeris_core::domain::{EpcisEvent, EventId, EventQuery};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::EventRepository;
use serde_json::Value;
use tokio_postgres::NoTls;
use uuid::Uuid;

use crate::schema::INIT_SCHEMA;

#[derive(Clone)]
pub struct PgEventRepository {
    pool: Pool,
}

impl PgEventRepository {
    pub fn from_pool(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn connect(conn_str: &str) -> Result<Self, RepoError> {
        let mut cfg = Config::new();
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
        cfg.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;
        client
            .batch_execute(INIT_SCHEMA)
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;
        Ok(())
    }

    fn hash_event(event: &EpcisEvent) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let json = serde_json::to_string(event).unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        json.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn event_type_name(event: &EpcisEvent) -> &'static str {
        match event {
            EpcisEvent::ObjectEvent(_) => "ObjectEvent",
            EpcisEvent::AggregationEvent(_) => "AggregationEvent",
            EpcisEvent::TransformationEvent(_) => "TransformationEvent",
        }
    }

    fn event_time(event: &EpcisEvent) -> chrono::DateTime<chrono::FixedOffset> {
        match event {
            EpcisEvent::ObjectEvent(d) => d.common.event_time,
            EpcisEvent::AggregationEvent(d) => d.common.event_time,
            EpcisEvent::TransformationEvent(d) => d.common.event_time,
        }
    }
}

impl EventRepository for PgEventRepository {
    async fn store_event(&self, event: &EpcisEvent) -> Result<EventId, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let hash = Self::hash_event(event);
        let event_data: Value =
            serde_json::to_value(event).map_err(|e| RepoError::Serialization(e.to_string()))?;

        // Check for duplicate (idempotent)
        let existing = client
            .query_opt(
                "SELECT id FROM epcis_events WHERE event_hash = $1",
                &[&hash],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        if let Some(row) = existing {
            let id: Uuid = row.get(0);
            return Ok(EventId(id));
        }

        let id = Uuid::new_v4();
        let event_type = Self::event_type_name(event);
        let event_time = Self::event_time(event);

        client
            .execute(
                "INSERT INTO epcis_events (id, event_type, event_time, event_data, event_hash) VALUES ($1, $2, $3, $4, $5)",
                &[&id, &event_type, &event_time, &event_data, &hash],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(EventId(id))
    }

    async fn get_event(&self, id: &EventId) -> Result<Option<EpcisEvent>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let row = client
            .query_opt(
                "SELECT event_data FROM epcis_events WHERE id = $1",
                &[&id.0],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        match row {
            Some(row) => {
                let data: Value = row.get(0);
                let event: EpcisEvent = serde_json::from_value(data)
                    .map_err(|e| RepoError::Serialization(e.to_string()))?;
                Ok(Some(event))
            }
            None => Ok(None),
        }
    }

    async fn query_events(&self, query: &EventQuery) -> Result<Vec<EpcisEvent>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let mut sql = String::from("SELECT event_data FROM epcis_events WHERE 1=1");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        let mut idx = 1;

        if let Some(ref ge) = query.ge_event_time {
            sql.push_str(&format!(" AND event_time >= ${idx}"));
            params.push(Box::new(*ge));
            idx += 1;
        }
        if let Some(ref lt) = query.lt_event_time {
            sql.push_str(&format!(" AND event_time < ${idx}"));
            params.push(Box::new(*lt));
            idx += 1;
        }
        if let Some(ref biz_step) = query.eq_biz_step {
            sql.push_str(&format!(" AND event_data->>'bizStep' = ${idx}"));
            params.push(Box::new(biz_step.clone()));
            idx += 1;
        }
        if let Some(ref match_epc) = query.match_epc {
            sql.push_str(&format!(" AND event_data->'epcList' ? ${idx}"));
            params.push(Box::new(match_epc.clone()));
            idx += 1;
        }

        let limit = query.per_page.unwrap_or(100);
        sql.push_str(&format!(" ORDER BY event_time DESC LIMIT ${idx}"));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref() as _).collect();

        let rows = client
            .query(&sql, &param_refs)
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let mut events = Vec::new();
        for row in rows {
            let data: Value = row.get(0);
            let event: EpcisEvent = serde_json::from_value(data)
                .map_err(|e| RepoError::Serialization(e.to_string()))?;
            events.push(event);
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    async fn setup_test_db() -> (PgEventRepository, impl Drop) {
        let container = Postgres::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let url = format!(
            "host={} port={} user=postgres password=postgres dbname=postgres",
            host, port
        );

        let repo = PgEventRepository::connect(&url).await.unwrap();
        repo.run_migrations().await.unwrap();
        (repo, container)
    }

    #[tokio::test]
    async fn test_store_and_retrieve_event() {
        let (repo, _container) = setup_test_db().await;

        let json: serde_json::Value = serde_json::from_str(
            r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2005-04-03T20:33:31.116-06:00",
            "eventTimeZoneOffset": "-06:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"],
            "bizStep": "shipping"
        }"#,
        )
        .unwrap();

        let event: EpcisEvent = serde_json::from_value(json).unwrap();
        let event_id = repo.store_event(&event).await.unwrap();
        let retrieved = repo.get_event(&event_id).await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_idempotent_store() {
        let (repo, _container) = setup_test_db().await;

        let json: serde_json::Value = serde_json::from_str(
            r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2005-04-03T20:33:31.116-06:00",
            "eventTimeZoneOffset": "-06:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2017"]
        }"#,
        )
        .unwrap();
        let event: EpcisEvent = serde_json::from_value(json).unwrap();

        let id1 = repo.store_event(&event).await.unwrap();
        let id2 = repo.store_event(&event).await.unwrap();
        assert_eq!(id1.0, id2.0);
    }

    #[tokio::test]
    async fn test_query_by_biz_step() {
        let (repo, _container) = setup_test_db().await;

        let shipping_event: EpcisEvent = serde_json::from_str(
            r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2020-01-01T10:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.1001"],
            "bizStep": "shipping"
        }"#,
        )
        .unwrap();

        let receiving_event: EpcisEvent = serde_json::from_str(
            r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2020-01-02T10:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.1002"],
            "bizStep": "receiving"
        }"#,
        )
        .unwrap();

        repo.store_event(&shipping_event).await.unwrap();
        repo.store_event(&receiving_event).await.unwrap();

        let query = EventQuery {
            eq_biz_step: Some("shipping".to_string()),
            ..Default::default()
        };

        let results = repo.query_events(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        match &results[0] {
            EpcisEvent::ObjectEvent(data) => {
                assert_eq!(data.common.biz_step.as_deref(), Some("shipping"));
            }
            _ => panic!("Expected ObjectEvent"),
        }
    }

    #[tokio::test]
    async fn test_query_by_time_range() {
        let (repo, _container) = setup_test_db().await;

        let early_event: EpcisEvent = serde_json::from_str(
            r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2020-01-01T10:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2001"]
        }"#,
        )
        .unwrap();

        let mid_event: EpcisEvent = serde_json::from_str(
            r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2020-06-15T10:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2002"]
        }"#,
        )
        .unwrap();

        let late_event: EpcisEvent = serde_json::from_str(
            r#"{
            "type": "ObjectEvent",
            "action": "OBSERVE",
            "eventTime": "2020-12-31T10:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "epcList": ["urn:epc:id:sgtin:0614141.107346.2003"]
        }"#,
        )
        .unwrap();

        repo.store_event(&early_event).await.unwrap();
        repo.store_event(&mid_event).await.unwrap();
        repo.store_event(&late_event).await.unwrap();

        let query = EventQuery {
            ge_event_time: Some(
                chrono::DateTime::parse_from_rfc3339("2020-03-01T00:00:00+00:00").unwrap(),
            ),
            lt_event_time: Some(
                chrono::DateTime::parse_from_rfc3339("2020-09-01T00:00:00+00:00").unwrap(),
            ),
            ..Default::default()
        };

        let results = repo.query_events(&query).await.unwrap();
        assert_eq!(results.len(), 1);
    }
}

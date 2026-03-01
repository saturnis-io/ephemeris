use std::str::FromStr;

use deadpool_postgres::Pool;
use ephemeris_core::domain::{
    Epc, EventId, SerialNumber, SerialNumberQuery, SnState, SnTransition, TransitionSource,
};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::SerialNumberRepository;

/// PostgreSQL-backed serial number state repository.
#[derive(Clone)]
pub struct PgSerialNumberRepository {
    pool: Pool,
}

impl PgSerialNumberRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl SerialNumberRepository for PgSerialNumberRepository {
    async fn upsert_state(
        &self,
        epc: &Epc,
        state: SnState,
        sid_class: Option<&str>,
        pool_id: Option<&str>,
    ) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        client
            .execute(
                "INSERT INTO serial_numbers (epc, state, sid_class, pool_id)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (epc) DO UPDATE SET
                     state = EXCLUDED.state,
                     sid_class = COALESCE(EXCLUDED.sid_class, serial_numbers.sid_class),
                     pool_id = COALESCE(EXCLUDED.pool_id, serial_numbers.pool_id),
                     updated_at = now()",
                &[&epc.as_str(), &state.to_string(), &sid_class, &pool_id],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_state(&self, epc: &Epc) -> Result<Option<SerialNumber>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let row = client
            .query_opt(
                "SELECT epc, state, sid_class, pool_id, created_at, updated_at
                 FROM serial_numbers WHERE epc = $1",
                &[&epc.as_str()],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        match row {
            Some(row) => {
                let state_str: String = row.get(1);
                Ok(Some(SerialNumber {
                    epc: Epc::new(row.get::<_, &str>(0)),
                    state: SnState::from_str(&state_str).map_err(RepoError::Serialization)?,
                    sid_class: row.get(2),
                    pool_id: row.get(3),
                    created_at: row.get(4),
                    updated_at: row.get(5),
                }))
            }
            None => Ok(None),
        }
    }

    async fn query(&self, query: &SerialNumberQuery) -> Result<Vec<SerialNumber>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let mut sql = String::from(
            "SELECT epc, state, sid_class, pool_id, created_at, updated_at FROM serial_numbers WHERE 1=1",
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        let mut idx = 1;

        if let Some(ref state) = query.state {
            sql.push_str(&format!(" AND state = ${idx}"));
            params.push(Box::new(state.to_string()));
            idx += 1;
        }
        if let Some(ref sid_class) = query.sid_class {
            sql.push_str(&format!(" AND sid_class = ${idx}"));
            params.push(Box::new(sid_class.clone()));
            idx += 1;
        }
        if let Some(ref pool_id) = query.pool_id {
            sql.push_str(&format!(" AND pool_id = ${idx}"));
            params.push(Box::new(pool_id.clone()));
            idx += 1;
        }

        let limit = query.limit.unwrap_or(100) as i64;
        let offset = query.offset.unwrap_or(0) as i64;
        sql.push_str(&format!(
            " ORDER BY updated_at DESC LIMIT ${idx} OFFSET ${}",
            idx + 1
        ));
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref() as _).collect();

        let rows = client
            .query(&sql, &param_refs)
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        rows.iter()
            .map(|row| {
                let state_str: String = row.get(1);
                Ok(SerialNumber {
                    epc: Epc::new(row.get::<_, &str>(0)),
                    state: SnState::from_str(&state_str).map_err(RepoError::Serialization)?,
                    sid_class: row.get(2),
                    pool_id: row.get(3),
                    created_at: row.get(4),
                    updated_at: row.get(5),
                })
            })
            .collect()
    }

    async fn record_transition(&self, transition: &SnTransition) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let event_id = transition.event_id.as_ref().map(|e| e.0);
        let source = match transition.source {
            TransitionSource::Mqtt => "mqtt",
            TransitionSource::RestApi => "rest_api",
            TransitionSource::System => "system",
        };

        client
            .execute(
                "INSERT INTO sn_transitions (epc, from_state, to_state, biz_step, event_id, source, timestamp)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &transition.epc.as_str(),
                    &transition.from_state.to_string(),
                    &transition.to_state.to_string(),
                    &transition.biz_step,
                    &event_id,
                    &source,
                    &transition.timestamp,
                ],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_history(&self, epc: &Epc, limit: u32) -> Result<Vec<SnTransition>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let rows = client
            .query(
                "SELECT epc, from_state, to_state, biz_step, event_id, source, timestamp
                 FROM sn_transitions WHERE epc = $1
                 ORDER BY timestamp DESC LIMIT $2",
                &[&epc.as_str(), &(limit as i64)],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        rows.iter()
            .map(|row| {
                let from_str: String = row.get(1);
                let to_str: String = row.get(2);
                let source_str: String = row.get(5);
                Ok(SnTransition {
                    epc: Epc::new(row.get::<_, &str>(0)),
                    from_state: SnState::from_str(&from_str).map_err(RepoError::Serialization)?,
                    to_state: SnState::from_str(&to_str).map_err(RepoError::Serialization)?,
                    biz_step: row.get(3),
                    event_id: row.get::<_, Option<uuid::Uuid>>(4).map(EventId),
                    source: match source_str.as_str() {
                        "mqtt" => TransitionSource::Mqtt,
                        "rest_api" => TransitionSource::RestApi,
                        _ => TransitionSource::System,
                    },
                    timestamp: row.get(6),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    async fn setup_test_db() -> (PgSerialNumberRepository, impl std::any::Any) {
        let container = Postgres::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();

        let mut cfg = deadpool_postgres::Config::new();
        cfg.host = Some(host.to_string());
        cfg.port = Some(port);
        cfg.user = Some("postgres".to_string());
        cfg.password = Some("postgres".to_string());
        cfg.dbname = Some("postgres".to_string());
        let pool = cfg
            .create_pool(
                Some(deadpool_postgres::Runtime::Tokio1),
                tokio_postgres::NoTls,
            )
            .unwrap();

        // Run migrations
        let client = pool.get().await.unwrap();
        client
            .batch_execute(crate::schema::INIT_SCHEMA)
            .await
            .unwrap();

        let repo = PgSerialNumberRepository::new(pool);
        (repo, container)
    }

    #[tokio::test]
    async fn test_upsert_and_get_state() {
        let (repo, _container) = setup_test_db().await;
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");

        // Initially no state
        assert!(repo.get_state(&epc).await.unwrap().is_none());

        // Insert
        repo.upsert_state(&epc, SnState::Commissioned, Some("sgtin"), None)
            .await
            .unwrap();

        let sn = repo.get_state(&epc).await.unwrap().unwrap();
        assert_eq!(sn.state, SnState::Commissioned);
        assert_eq!(sn.sid_class.as_deref(), Some("sgtin"));

        // Update
        repo.upsert_state(&epc, SnState::Released, None, None)
            .await
            .unwrap();

        let sn = repo.get_state(&epc).await.unwrap().unwrap();
        assert_eq!(sn.state, SnState::Released);
        // sid_class should be preserved (COALESCE)
        assert_eq!(sn.sid_class.as_deref(), Some("sgtin"));
    }

    #[tokio::test]
    async fn test_query_by_state() {
        let (repo, _container) = setup_test_db().await;

        repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.001"),
            SnState::Commissioned,
            None,
            None,
        )
        .await
        .unwrap();
        repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.002"),
            SnState::Released,
            None,
            None,
        )
        .await
        .unwrap();
        repo.upsert_state(
            &Epc::new("urn:epc:id:sgtin:0614141.107346.003"),
            SnState::Commissioned,
            None,
            None,
        )
        .await
        .unwrap();

        let query = SerialNumberQuery {
            state: Some(SnState::Commissioned),
            ..Default::default()
        };
        let results = repo.query(&query).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_record_and_get_history() {
        let (repo, _container) = setup_test_db().await;
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.2017");

        let t1 = SnTransition {
            epc: epc.clone(),
            from_state: SnState::Unassigned,
            to_state: SnState::Commissioned,
            biz_step: "commissioning".to_string(),
            event_id: None,
            source: TransitionSource::Mqtt,
            timestamp: chrono::Utc::now().fixed_offset(),
        };
        repo.record_transition(&t1).await.unwrap();

        let t2 = SnTransition {
            epc: epc.clone(),
            from_state: SnState::Commissioned,
            to_state: SnState::Released,
            biz_step: "shipping".to_string(),
            event_id: None,
            source: TransitionSource::Mqtt,
            timestamp: chrono::Utc::now().fixed_offset(),
        };
        repo.record_transition(&t2).await.unwrap();

        let history = repo.get_history(&epc, 10).await.unwrap();
        assert_eq!(history.len(), 2);
        // Newest first
        assert_eq!(history[0].to_state, SnState::Released);
        assert_eq!(history[1].to_state, SnState::Commissioned);
    }
}

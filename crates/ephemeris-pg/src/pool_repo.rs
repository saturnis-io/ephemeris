use std::str::FromStr;

use deadpool_postgres::Pool;
use ephemeris_core::domain::{
    Epc, PoolCriterionKey, PoolId, PoolQuery, PoolSelectionCriteria, PoolStats, SerialNumberPool,
    SnState,
};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::PoolRepository;

/// PostgreSQL-backed serial number pool repository.
#[derive(Clone)]
pub struct PgPoolRepository {
    pool: Pool,
}

impl PgPoolRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

/// Serialize a `PoolCriterionKey` to its snake_case DB string.
///
/// Standard variants serialize via serde_json (e.g. `Gtin` -> `"gtin"`).
/// The `Custom` variant extracts its inner string directly.
fn criterion_key_to_str(key: &PoolCriterionKey) -> Result<String, RepoError> {
    let value = serde_json::to_value(key).map_err(|e| RepoError::Serialization(e.to_string()))?;

    match value.as_str() {
        Some(s) => Ok(s.to_string()),
        None => {
            // Custom("x") serializes as {"custom": "x"} — extract the inner value.
            if let PoolCriterionKey::Custom(s) = key {
                Ok(s.clone())
            } else {
                Err(RepoError::Serialization(format!(
                    "unexpected criterion key format: {value}"
                )))
            }
        }
    }
}

/// Parse a DB string back into a `PoolCriterionKey`.
fn parse_criterion_key(s: &str) -> PoolCriterionKey {
    match s {
        "gtin" => PoolCriterionKey::Gtin,
        "sscc_gcp" => PoolCriterionKey::SsccGcp,
        "sscc_extension" => PoolCriterionKey::SsccExtension,
        "country_code" => PoolCriterionKey::CountryCode,
        "location" => PoolCriterionKey::Location,
        "sublocation" => PoolCriterionKey::Sublocation,
        "lot_number" => PoolCriterionKey::LotNumber,
        "pool_id" => PoolCriterionKey::PoolId,
        "sid_class_id" => PoolCriterionKey::SidClassId,
        "order_id" => PoolCriterionKey::OrderId,
        other => PoolCriterionKey::Custom(other.to_string()),
    }
}

impl PoolRepository for PgPoolRepository {
    async fn create_pool(&self, pool: &SerialNumberPool) -> Result<PoolId, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        client
            .execute(
                "INSERT INTO sn_pools (id, name, sid_class, esm_endpoint, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)",
                &[
                    &pool.id.0,
                    &pool.name,
                    &pool.sid_class,
                    &pool.esm_endpoint,
                    &pool.created_at,
                    &pool.updated_at,
                ],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        for (key, value) in &pool.criteria.criteria {
            let key_str = criterion_key_to_str(key)?;
            client
                .execute(
                    "INSERT INTO pool_criteria (pool_id, key, value) VALUES ($1, $2, $3)",
                    &[&pool.id.0, &key_str, value],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;
        }

        Ok(pool.id.clone())
    }

    async fn get_pool(&self, id: &PoolId) -> Result<Option<SerialNumberPool>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let row = client
            .query_opt(
                "SELECT id, name, sid_class, esm_endpoint, created_at, updated_at
                 FROM sn_pools WHERE id = $1",
                &[&id.0],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        let criteria_rows = client
            .query(
                "SELECT key, value FROM pool_criteria WHERE pool_id = $1",
                &[&id.0],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let criteria = criteria_rows
            .iter()
            .map(|r| {
                let key_str: String = r.get(0);
                let value: String = r.get(1);
                (parse_criterion_key(&key_str), value)
            })
            .collect();

        Ok(Some(SerialNumberPool {
            id: PoolId(row.get(0)),
            name: row.get(1),
            sid_class: row.get(2),
            esm_endpoint: row.get(3),
            criteria: PoolSelectionCriteria { criteria },
            created_at: row.get(4),
            updated_at: row.get(5),
        }))
    }

    async fn list_pools(&self, filter: &PoolQuery) -> Result<Vec<SerialNumberPool>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let mut sql = String::from(
            "SELECT id, name, sid_class, esm_endpoint, created_at, updated_at FROM sn_pools WHERE 1=1",
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        let mut idx = 1;

        if let Some(ref sid_class) = filter.sid_class {
            sql.push_str(&format!(" AND sid_class = ${idx}"));
            params.push(Box::new(sid_class.clone()));
            idx += 1;
        }
        if let Some(ref name_contains) = filter.name_contains {
            sql.push_str(&format!(" AND name ILIKE ${idx}"));
            params.push(Box::new(format!("%{name_contains}%")));
            idx += 1;
        }

        let limit = filter.limit.unwrap_or(100) as i64;
        let offset = filter.offset.unwrap_or(0) as i64;
        sql.push_str(&format!(
            " ORDER BY created_at DESC LIMIT ${idx} OFFSET ${}",
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

        let mut pools = Vec::with_capacity(rows.len());
        for row in &rows {
            let pool_id: uuid::Uuid = row.get(0);

            let criteria_rows = client
                .query(
                    "SELECT key, value FROM pool_criteria WHERE pool_id = $1",
                    &[&pool_id],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;

            let criteria = criteria_rows
                .iter()
                .map(|r| {
                    let key_str: String = r.get(0);
                    let value: String = r.get(1);
                    (parse_criterion_key(&key_str), value)
                })
                .collect();

            pools.push(SerialNumberPool {
                id: PoolId(pool_id),
                name: row.get(1),
                sid_class: row.get(2),
                esm_endpoint: row.get(3),
                criteria: PoolSelectionCriteria { criteria },
                created_at: row.get(4),
                updated_at: row.get(5),
            });
        }

        Ok(pools)
    }

    async fn delete_pool(&self, id: &PoolId) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let pool_id_str = id.0.to_string();
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM serial_numbers WHERE pool_id = $1",
                &[&pool_id_str],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let count: i64 = row.get(0);
        if count > 0 {
            return Err(RepoError::Query(format!(
                "cannot delete pool {id}: {count} serial numbers still assigned"
            )));
        }

        client
            .execute("DELETE FROM sn_pools WHERE id = $1", &[&id.0])
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(())
    }

    async fn assign_to_pool(
        &self,
        pool_id: &PoolId,
        epcs: &[Epc],
        initial_state: Option<&str>,
    ) -> Result<u32, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let state = initial_state.unwrap_or("unallocated");
        let pool_id_str = pool_id.0.to_string();
        let mut count = 0u32;

        for epc in epcs {
            client
                .execute(
                    "INSERT INTO serial_numbers (epc, state, pool_id)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (epc) DO UPDATE SET
                         state = EXCLUDED.state,
                         pool_id = EXCLUDED.pool_id,
                         updated_at = now()",
                    &[&epc.as_str(), &state, &pool_id_str],
                )
                .await
                .map_err(|e| RepoError::Query(e.to_string()))?;
            count += 1;
        }

        Ok(count)
    }

    async fn request_numbers(&self, pool_id: &PoolId, count: u32) -> Result<Vec<Epc>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let pool_id_str = pool_id.0.to_string();
        let limit = count as i64;

        let rows = client
            .query(
                "UPDATE serial_numbers SET state = 'allocated', updated_at = now()
                 WHERE epc IN (
                     SELECT epc FROM serial_numbers
                     WHERE pool_id = $1 AND state = 'unallocated'
                     FOR UPDATE SKIP LOCKED
                     LIMIT $2
                 )
                 RETURNING epc",
                &[&pool_id_str, &limit],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let epcs = rows
            .iter()
            .map(|row| Epc::new(row.get::<_, &str>(0)))
            .collect();

        Ok(epcs)
    }

    async fn return_numbers(&self, pool_id: &PoolId, epcs: &[Epc]) -> Result<u32, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let pool_id_str = pool_id.0.to_string();
        let epc_strings: Vec<&str> = epcs.iter().map(|e| e.as_str()).collect();

        let result = client
            .execute(
                "UPDATE serial_numbers SET state = 'unallocated', updated_at = now()
                 WHERE epc = ANY($1) AND pool_id = $2",
                &[&epc_strings, &pool_id_str],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        Ok(result as u32)
    }

    async fn get_pool_stats(&self, pool_id: &PoolId) -> Result<PoolStats, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let pool_id_str = pool_id.0.to_string();

        let rows = client
            .query(
                "SELECT state, COUNT(*) FROM serial_numbers WHERE pool_id = $1 GROUP BY state",
                &[&pool_id_str],
            )
            .await
            .map_err(|e| RepoError::Query(e.to_string()))?;

        let mut stats = PoolStats {
            pool_id: pool_id.clone(),
            total: 0,
            unassigned: 0,
            unallocated: 0,
            allocated: 0,
            encoded: 0,
            commissioned: 0,
            other: 0,
        };

        for row in &rows {
            let state_str: String = row.get(0);
            let count: i64 = row.get(1);
            let count = count as u64;

            stats.total += count;

            match SnState::from_str(&state_str) {
                Ok(SnState::Unassigned) => stats.unassigned += count,
                Ok(SnState::Unallocated) => stats.unallocated += count,
                Ok(SnState::Allocated) => stats.allocated += count,
                Ok(SnState::Encoded) => stats.encoded += count,
                Ok(SnState::Commissioned) => stats.commissioned += count,
                _ => stats.other += count,
            }
        }

        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial_number_repo::PgSerialNumberRepository;
    use ephemeris_core::repository::SerialNumberRepository;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    async fn setup_test_db() -> (
        PgPoolRepository,
        PgSerialNumberRepository,
        impl std::any::Any,
    ) {
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

        let pool_repo = PgPoolRepository::new(pool.clone());
        let sn_repo = PgSerialNumberRepository::new(pool);
        (pool_repo, sn_repo, container)
    }

    fn make_pool(name: &str, sid_class: Option<&str>) -> SerialNumberPool {
        let now = chrono::Utc::now().fixed_offset();
        SerialNumberPool {
            id: PoolId::new(),
            name: name.to_string(),
            sid_class: sid_class.map(|s| s.to_string()),
            criteria: PoolSelectionCriteria {
                criteria: vec![(PoolCriterionKey::Gtin, "09521568251204".to_string())],
            },
            esm_endpoint: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn test_create_and_get_pool() {
        let (repo, _sn_repo, _container) = setup_test_db().await;

        let pool = make_pool("Test Pool", Some("sgtin"));
        let id = repo.create_pool(&pool).await.unwrap();

        let fetched = repo.get_pool(&id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test Pool");
        assert_eq!(fetched.sid_class.as_deref(), Some("sgtin"));
        assert_eq!(fetched.criteria.criteria.len(), 1);
        assert_eq!(fetched.criteria.criteria[0].0, PoolCriterionKey::Gtin);
        assert_eq!(fetched.criteria.criteria[0].1, "09521568251204");
    }

    #[tokio::test]
    async fn test_list_pools() {
        let (repo, _sn_repo, _container) = setup_test_db().await;

        repo.create_pool(&make_pool("Pool A", Some("sgtin")))
            .await
            .unwrap();
        repo.create_pool(&make_pool("Pool B", Some("sscc")))
            .await
            .unwrap();

        let all = repo.list_pools(&PoolQuery::default()).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_empty_pool() {
        let (repo, _sn_repo, _container) = setup_test_db().await;

        let pool = make_pool("Deletable", None);
        let id = repo.create_pool(&pool).await.unwrap();

        repo.delete_pool(&id).await.unwrap();

        let fetched = repo.get_pool(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_assign_and_request_numbers() {
        let (repo, sn_repo, _container) = setup_test_db().await;

        let pool = make_pool("Alloc Pool", None);
        let id = repo.create_pool(&pool).await.unwrap();

        // Pre-create 5 SNs as unallocated in the pool
        for i in 0..5 {
            let epc = Epc::new(format!("urn:epc:id:sgtin:0614141.107346.{i:03}"));
            sn_repo
                .upsert_state(&epc, SnState::Unallocated, None, Some(&id.0.to_string()))
                .await
                .unwrap();
        }

        // Request 3
        let allocated = repo.request_numbers(&id, 3).await.unwrap();
        assert_eq!(allocated.len(), 3);

        // Verify allocated SNs have correct state
        for epc in &allocated {
            let sn = sn_repo.get_state(epc).await.unwrap().unwrap();
            assert_eq!(sn.state, SnState::Allocated);
        }
    }

    #[tokio::test]
    async fn test_return_numbers() {
        let (repo, sn_repo, _container) = setup_test_db().await;

        let pool = make_pool("Return Pool", None);
        let id = repo.create_pool(&pool).await.unwrap();

        // Create an allocated SN in the pool
        let epc = Epc::new("urn:epc:id:sgtin:0614141.107346.900");
        sn_repo
            .upsert_state(&epc, SnState::Allocated, None, Some(&id.0.to_string()))
            .await
            .unwrap();

        // Return it
        let returned = repo.return_numbers(&id, &[epc.clone()]).await.unwrap();
        assert_eq!(returned, 1);

        // Verify state is now unallocated
        let sn = sn_repo.get_state(&epc).await.unwrap().unwrap();
        assert_eq!(sn.state, SnState::Unallocated);
    }

    #[tokio::test]
    async fn test_get_pool_stats() {
        let (repo, sn_repo, _container) = setup_test_db().await;

        let pool = make_pool("Stats Pool", None);
        let id = repo.create_pool(&pool).await.unwrap();
        let pid_str = id.0.to_string();

        // Create SNs with mixed states
        sn_repo
            .upsert_state(
                &Epc::new("urn:epc:id:sgtin:0614141.107346.S01"),
                SnState::Unallocated,
                None,
                Some(&pid_str),
            )
            .await
            .unwrap();
        sn_repo
            .upsert_state(
                &Epc::new("urn:epc:id:sgtin:0614141.107346.S02"),
                SnState::Allocated,
                None,
                Some(&pid_str),
            )
            .await
            .unwrap();
        sn_repo
            .upsert_state(
                &Epc::new("urn:epc:id:sgtin:0614141.107346.S03"),
                SnState::Commissioned,
                None,
                Some(&pid_str),
            )
            .await
            .unwrap();

        let stats = repo.get_pool_stats(&id).await.unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.unallocated, 1);
        assert_eq!(stats.allocated, 1);
        assert_eq!(stats.commissioned, 1);
    }

    #[tokio::test]
    async fn test_request_more_than_available() {
        let (repo, sn_repo, _container) = setup_test_db().await;

        let pool = make_pool("Scarce Pool", None);
        let id = repo.create_pool(&pool).await.unwrap();

        // Only 2 unallocated SNs
        for i in 0..2 {
            let epc = Epc::new(format!("urn:epc:id:sgtin:0614141.107346.M{i:02}"));
            sn_repo
                .upsert_state(&epc, SnState::Unallocated, None, Some(&id.0.to_string()))
                .await
                .unwrap();
        }

        // Request 10 — should only get 2 (no error)
        let allocated = repo.request_numbers(&id, 10).await.unwrap();
        assert_eq!(allocated.len(), 2);
    }

    #[tokio::test]
    async fn test_assign_to_pool() {
        let (repo, sn_repo, _container) = setup_test_db().await;

        let pool = make_pool("Assign Pool", None);
        let id = repo.create_pool(&pool).await.unwrap();

        let epcs = vec![
            Epc::new("urn:epc:id:sgtin:0614141.107346.A01"),
            Epc::new("urn:epc:id:sgtin:0614141.107346.A02"),
        ];

        let count = repo
            .assign_to_pool(&id, &epcs, Some("unallocated"))
            .await
            .unwrap();
        assert_eq!(count, 2);

        // Verify each SN exists with correct state and pool_id
        for epc in &epcs {
            let sn = sn_repo.get_state(epc).await.unwrap().unwrap();
            assert_eq!(sn.state, SnState::Unallocated);
            assert_eq!(sn.pool_id.as_deref(), Some(id.0.to_string().as_str()));
        }
    }
}

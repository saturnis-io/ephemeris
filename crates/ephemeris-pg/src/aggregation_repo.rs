use std::collections::HashMap;

use deadpool_postgres::Pool;
use ephemeris_core::domain::{AggregationNode, AggregationTree, Epc, EventId};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::AggregationRepository;

#[derive(Clone)]
pub struct PgAggregationRepository {
    pool: Pool,
}

impl PgAggregationRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Convert an EPC URI to a valid ltree label (alphanumeric + underscore only).
    fn epc_to_label(epc: &Epc) -> String {
        epc.as_str()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect()
    }

    /// Build an AggregationTree from flat database rows.
    fn build_tree(root: &Epc, rows: &[tokio_postgres::Row]) -> AggregationTree {
        // Collect parent -> children mapping
        let mut children_map: HashMap<String, Vec<Epc>> = HashMap::new();
        for row in rows {
            let child_epc: String = row.get(0);
            let parent_epc: String = row.get(1);
            children_map
                .entry(parent_epc)
                .or_default()
                .push(Epc::new(child_epc));
        }

        // Recursively build nodes
        fn build_nodes(
            epc: &Epc,
            children_map: &HashMap<String, Vec<Epc>>,
        ) -> Vec<AggregationNode> {
            let Some(children) = children_map.get(epc.as_str()) else {
                return Vec::new();
            };
            children
                .iter()
                .map(|child| AggregationNode {
                    epc: child.clone(),
                    children: build_nodes(child, children_map),
                })
                .collect()
        }

        let nodes = build_nodes(root, &children_map);
        AggregationTree {
            root: root.clone(),
            nodes,
        }
    }
}

impl AggregationRepository for PgAggregationRepository {
    async fn add_child(
        &self,
        parent: &Epc,
        child: &Epc,
        event_id: &EventId,
    ) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let parent_label = Self::epc_to_label(parent);
        let child_label = Self::epc_to_label(child);

        // Find parent's path, or use parent label as root
        let parent_path_row = client
            .query_opt(
                "SELECT path::text FROM aggregation WHERE child_epc = $1",
                &[&parent.as_str()],
            )
            .await
            .map_err(|e| RepoError::Query(format!("{e:?}")))?;

        let parent_path_str = match parent_path_row {
            Some(row) => {
                let p: String = row.get(0);
                p
            }
            None => parent_label, // Root node -- path is just its own label
        };

        let child_path = format!("{parent_path_str}.{child_label}");

        // Use parameterized query for safe values, but handle ltree via
        // text_to_ltree() function to avoid binary protocol issues
        client
            .execute(
                "INSERT INTO aggregation (child_epc, parent_epc, path, event_id) \
                 VALUES ($1, $2, text2ltree($3), $4) \
                 ON CONFLICT (child_epc) DO UPDATE SET parent_epc = $2, path = text2ltree($3), event_id = $4",
                &[&child.as_str(), &parent.as_str(), &child_path, &event_id.0],
            )
            .await
            .map_err(|e| RepoError::Query(format!("{e:?}")))?;

        Ok(())
    }

    async fn remove_child(&self, _parent: &Epc, child: &Epc) -> Result<(), RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        // Find child's path to remove it and all its descendants
        let child_path_row = client
            .query_opt(
                "SELECT path::text FROM aggregation WHERE child_epc = $1",
                &[&child.as_str()],
            )
            .await
            .map_err(|e| RepoError::Query(format!("{e:?}")))?;

        if let Some(row) = child_path_row {
            let path: String = row.get(0);
            // Use text2ltree() to avoid binary protocol issues with ltree
            client
                .execute(
                    "DELETE FROM aggregation WHERE path <@ text2ltree($1)",
                    &[&path],
                )
                .await
                .map_err(|e| RepoError::Query(format!("{e:?}")))?;
        }

        Ok(())
    }

    async fn get_children(&self, parent: &Epc) -> Result<Vec<Epc>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let rows = client
            .query(
                "SELECT child_epc FROM aggregation WHERE parent_epc = $1",
                &[&parent.as_str()],
            )
            .await
            .map_err(|e| RepoError::Query(format!("{e:?}")))?;

        Ok(rows
            .iter()
            .map(|r| Epc::new(r.get::<_, String>(0)))
            .collect())
    }

    async fn get_ancestors(&self, child: &Epc) -> Result<Vec<Epc>, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        // Walk up the parent chain
        let mut ancestors = Vec::new();
        let mut current = child.clone();

        loop {
            let row = client
                .query_opt(
                    "SELECT parent_epc FROM aggregation WHERE child_epc = $1",
                    &[&current.as_str()],
                )
                .await
                .map_err(|e| RepoError::Query(format!("{e:?}")))?;

            match row {
                Some(r) => {
                    let parent_epc: String = r.get(0);
                    let parent = Epc::new(&parent_epc);
                    ancestors.push(parent.clone());
                    current = parent;
                }
                None => break,
            }
        }

        Ok(ancestors)
    }

    async fn get_full_hierarchy(&self, root: &Epc) -> Result<AggregationTree, RepoError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Connection(e.to_string()))?;

        let root_label = Self::epc_to_label(root);

        let rows = client
            .query(
                "SELECT child_epc, parent_epc, path::text FROM aggregation \
                 WHERE path <@ text2ltree($1) ORDER BY path",
                &[&root_label],
            )
            .await
            .map_err(|e| RepoError::Query(format!("{e:?}")))?;

        Ok(Self::build_tree(root, &rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_repo::PgEventRepository;
    use ephemeris_core::domain::EpcisEvent;
    use ephemeris_core::repository::EventRepository;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    async fn setup_test_db() -> (
        PgEventRepository,
        PgAggregationRepository,
        impl std::any::Any,
    ) {
        let container = Postgres::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let conn_str = format!(
            "host={} port={} user=postgres password=postgres dbname=postgres",
            host, port
        );

        let event_repo = PgEventRepository::connect(&conn_str).await.unwrap();
        event_repo.run_migrations().await.unwrap();

        let agg_repo = {
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
            let pool = cfg
                .create_pool(
                    Some(deadpool_postgres::Runtime::Tokio1),
                    tokio_postgres::NoTls,
                )
                .unwrap();
            PgAggregationRepository::new(pool)
        };

        (event_repo, agg_repo, container)
    }

    async fn store_dummy_event(event_repo: &PgEventRepository) -> EventId {
        let event: EpcisEvent = serde_json::from_str(
            r#"{
            "type": "AggregationEvent",
            "action": "ADD",
            "eventTime": "2020-01-01T10:00:00.000+00:00",
            "eventTimeZoneOffset": "+00:00",
            "parentID": "urn:epc:id:sscc:0614141.P001",
            "childEPCs": ["urn:epc:id:sscc:0614141.C001"]
        }"#,
        )
        .unwrap();
        event_repo.store_event(&event).await.unwrap()
    }

    #[tokio::test]
    async fn test_add_and_get_children() {
        let (event_repo, agg_repo, _container) = setup_test_db().await;
        let event_id = store_dummy_event(&event_repo).await;

        let pallet = Epc::new("urn:epc:id:sscc:0614141.0000001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.0000002");
        let case2 = Epc::new("urn:epc:id:sscc:0614141.0000003");

        agg_repo
            .add_child(&pallet, &case1, &event_id)
            .await
            .unwrap();
        agg_repo
            .add_child(&pallet, &case2, &event_id)
            .await
            .unwrap();

        let children = agg_repo.get_children(&pallet).await.unwrap();
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_get_ancestors() {
        let (event_repo, agg_repo, _container) = setup_test_db().await;
        let event_id = store_dummy_event(&event_repo).await;

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C001");
        let unit1 = Epc::new("urn:epc:id:sgtin:0614141.107346.001");

        agg_repo
            .add_child(&pallet, &case1, &event_id)
            .await
            .unwrap();
        agg_repo.add_child(&case1, &unit1, &event_id).await.unwrap();

        let ancestors = agg_repo.get_ancestors(&unit1).await.unwrap();
        assert_eq!(ancestors.len(), 2); // case1, pallet
    }

    #[tokio::test]
    async fn test_full_hierarchy() {
        let (event_repo, agg_repo, _container) = setup_test_db().await;
        let event_id = store_dummy_event(&event_repo).await;

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C001");
        let unit1 = Epc::new("urn:epc:id:sgtin:0614141.107346.001");
        let unit2 = Epc::new("urn:epc:id:sgtin:0614141.107346.002");

        agg_repo
            .add_child(&pallet, &case1, &event_id)
            .await
            .unwrap();
        agg_repo.add_child(&case1, &unit1, &event_id).await.unwrap();
        agg_repo.add_child(&case1, &unit2, &event_id).await.unwrap();

        let tree = agg_repo.get_full_hierarchy(&pallet).await.unwrap();
        assert_eq!(tree.root, pallet);
        assert_eq!(tree.nodes.len(), 1); // case1
        assert_eq!(tree.nodes[0].children.len(), 2); // unit1, unit2
    }

    #[tokio::test]
    async fn test_remove_child() {
        let (event_repo, agg_repo, _container) = setup_test_db().await;
        let event_id = store_dummy_event(&event_repo).await;

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C001");
        let unit1 = Epc::new("urn:epc:id:sgtin:0614141.107346.001");

        agg_repo
            .add_child(&pallet, &case1, &event_id)
            .await
            .unwrap();
        agg_repo.add_child(&case1, &unit1, &event_id).await.unwrap();

        // Remove case1 -- should also remove unit1 (descendant)
        agg_repo.remove_child(&pallet, &case1).await.unwrap();

        let children = agg_repo.get_children(&pallet).await.unwrap();
        assert_eq!(children.len(), 0);
    }
}

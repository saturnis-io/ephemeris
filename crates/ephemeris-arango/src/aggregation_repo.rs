use ephemeris_core::domain::{AggregationNode, AggregationTree, Epc, EventId};
use ephemeris_core::error::RepoError;
use ephemeris_core::repository::AggregationRepository;
use serde_json::{Value, json};

use crate::client::ArangoClient;

/// ArangoDB-backed implementation of [`AggregationRepository`].
///
/// Uses the ArangoDB graph API (HTTP REST) to manage parent-child packaging
/// relationships. Vertices live in the `packaging` collection and edges in `contains`.
#[derive(Clone)]
pub struct ArangoAggregationRepository {
    client: ArangoClient,
    graph_name: String,
}

impl ArangoAggregationRepository {
    pub fn new(client: ArangoClient, graph_name: String) -> Self {
        Self { client, graph_name }
    }
}

/// Encode an EPC string as a safe ArangoDB document key.
/// ArangoDB keys allow only `[a-zA-Z0-9_:.@()+,=;!*'-]`.
/// We percent-encode `/` as `_S_` since EPC URIs contain slashes.
fn epc_to_key(epc: &Epc) -> String {
    epc.as_str().replace('/', "_S_")
}

/// Decode an ArangoDB key back to an EPC string.
fn key_to_epc(key: &str) -> Epc {
    Epc::new(key.replace("_S_", "/"))
}

/// Build the full document handle for a packaging vertex.
fn vertex_id(epc: &Epc) -> String {
    format!("packaging/{}", epc_to_key(epc))
}

fn map_err(e: crate::client::ArangoError) -> RepoError {
    match &e {
        crate::client::ArangoError::Http(_) => RepoError::Connection(e.to_string()),
        crate::client::ArangoError::Arango { status, .. } if *status == 404 => {
            RepoError::NotFound(e.to_string())
        }
        crate::client::ArangoError::Arango { .. } => RepoError::Query(e.to_string()),
        crate::client::ArangoError::AuthFailed => RepoError::Connection(e.to_string()),
    }
}

impl AggregationRepository for ArangoAggregationRepository {
    async fn add_child(
        &self,
        parent: &Epc,
        child: &Epc,
        event_id: &EventId,
    ) -> Result<(), RepoError> {
        // Ensure both vertices exist
        self.client
            .ensure_vertex(
                &self.graph_name,
                "packaging",
                &epc_to_key(parent),
                &json!({"epc": parent.as_str()}),
            )
            .await
            .map_err(map_err)?;

        self.client
            .ensure_vertex(
                &self.graph_name,
                "packaging",
                &epc_to_key(child),
                &json!({"epc": child.as_str()}),
            )
            .await
            .map_err(map_err)?;

        // Insert edge from parent to child
        self.client
            .insert_edge(
                &self.graph_name,
                "contains",
                &vertex_id(parent),
                &vertex_id(child),
            )
            .await
            .map_err(map_err)?;

        tracing::debug!(
            parent = parent.as_str(),
            child = child.as_str(),
            event_id = %event_id.0,
            "added aggregation edge"
        );

        Ok(())
    }

    async fn remove_child(&self, parent: &Epc, child: &Epc) -> Result<(), RepoError> {
        // Find the edge between parent and child
        let edges = self
            .client
            .execute_aql(
                "FOR e IN contains FILTER e._from == @from AND e._to == @to RETURN e._key",
                &json!({
                    "from": vertex_id(parent),
                    "to": vertex_id(child),
                }),
            )
            .await
            .map_err(map_err)?;

        // Delete all matching edges
        for edge_key in &edges {
            if let Some(key) = edge_key.as_str() {
                self.client
                    .delete_edge(&self.graph_name, "contains", key)
                    .await
                    .map_err(map_err)?;
            }
        }

        // Check if child is now orphaned (no inbound edges)
        let inbound = self
            .client
            .execute_aql(
                "FOR e IN contains FILTER e._to == @to RETURN 1",
                &json!({"to": vertex_id(child)}),
            )
            .await
            .map_err(map_err)?;

        if inbound.is_empty() {
            // Also check outbound — only delete if truly isolated
            let outbound = self
                .client
                .execute_aql(
                    "FOR e IN contains FILTER e._from == @from RETURN 1",
                    &json!({"from": vertex_id(child)}),
                )
                .await
                .map_err(map_err)?;

            if outbound.is_empty() {
                self.client
                    .delete_vertex(&self.graph_name, "packaging", &epc_to_key(child))
                    .await
                    .map_err(map_err)?;
            }
        }

        Ok(())
    }

    async fn get_children(&self, parent: &Epc) -> Result<Vec<Epc>, RepoError> {
        let results = self
            .client
            .execute_aql(
                "FOR v IN 1..1 OUTBOUND @start GRAPH @g RETURN v._key",
                &json!({
                    "start": vertex_id(parent),
                    "g": self.graph_name,
                }),
            )
            .await
            .map_err(map_err)?;

        let children = results
            .into_iter()
            .filter_map(|v| v.as_str().map(key_to_epc))
            .collect();

        Ok(children)
    }

    async fn get_ancestors(&self, child: &Epc) -> Result<Vec<Epc>, RepoError> {
        let results = self
            .client
            .execute_aql(
                "FOR v IN 1..100 INBOUND @start GRAPH @g RETURN v._key",
                &json!({
                    "start": vertex_id(child),
                    "g": self.graph_name,
                }),
            )
            .await
            .map_err(map_err)?;

        let ancestors = results
            .into_iter()
            .filter_map(|v| v.as_str().map(key_to_epc))
            .collect();

        Ok(ancestors)
    }

    async fn get_full_hierarchy(&self, root: &Epc) -> Result<AggregationTree, RepoError> {
        let results = self
            .client
            .execute_aql(
                "FOR v, e, p IN 0..100 OUTBOUND @start GRAPH @g \
                 RETURN {key: v._key, depth: LENGTH(p.edges), \
                         parent_key: LENGTH(p.edges) > 0 ? \
                         p.vertices[LENGTH(p.vertices) - 2]._key : null}",
                &json!({
                    "start": vertex_id(root),
                    "g": self.graph_name,
                }),
            )
            .await
            .map_err(map_err)?;

        let nodes = build_tree(root, &results);

        Ok(AggregationTree {
            root: root.clone(),
            nodes,
        })
    }
}

/// Build a tree of `AggregationNode` from the flat AQL traversal results.
fn build_tree(root: &Epc, results: &[Value]) -> Vec<AggregationNode> {
    // Collect parent -> children mapping
    let root_key = epc_to_key(root);
    let mut children_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for row in results {
        let key = match row["key"].as_str() {
            Some(k) => k.to_string(),
            None => continue,
        };
        let parent_key = row["parent_key"].as_str().map(|s| s.to_string());

        if let Some(pk) = parent_key {
            children_map.entry(pk).or_default().push(key);
        }
    }

    // Recursively build nodes from root's children
    fn build_nodes(
        parent_key: &str,
        children_map: &std::collections::HashMap<String, Vec<String>>,
    ) -> Vec<AggregationNode> {
        let Some(child_keys) = children_map.get(parent_key) else {
            return vec![];
        };
        child_keys
            .iter()
            .map(|ck| AggregationNode {
                epc: key_to_epc(ck),
                children: build_nodes(ck, children_map),
            })
            .collect()
    }

    build_nodes(&root_key, &children_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemeris_core::domain::{Epc, EventId};
    use testcontainers::{GenericImage, ImageExt, core::IntoContainerPort, runners::AsyncRunner};

    /// Start an ArangoDB container and return a connected repository.
    async fn setup_arango() -> Option<(
        ArangoAggregationRepository,
        testcontainers::ContainerAsync<GenericImage>,
    )> {
        let container = GenericImage::new("arangodb", "3.12")
            .with_exposed_port(8529.tcp())
            .with_env_var("ARANGO_NO_AUTH", "1")
            .start()
            .await;

        let container = match container {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Skipping ArangoDB test (Docker unavailable): {e}");
                return None;
            }
        };

        let port = container.get_host_port_ipv4(8529).await.unwrap();
        let host = container.get_host().await.unwrap();
        let base_url = format!("http://{host}:{port}");

        // Wait for ArangoDB to become ready
        let client = reqwest::Client::new();
        for _ in 0..60 {
            let result = client.get(format!("{base_url}/_api/version")).send().await;
            if let Ok(resp) = result {
                if resp.status().is_success() {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        let arango = ArangoClient::connect_no_auth(&base_url, "_system");
        arango.create_graph("packaging_graph").await.unwrap();

        let repo = ArangoAggregationRepository::new(arango, "packaging_graph".to_string());
        Some((repo, container))
    }

    #[tokio::test]
    async fn test_add_and_get_children() {
        let Some((repo, _container)) = setup_arango().await else {
            return;
        };

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P001");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C001");
        let case2 = Epc::new("urn:epc:id:sscc:0614141.C002");
        let event_id = EventId::new();

        repo.add_child(&pallet, &case1, &event_id).await.unwrap();
        repo.add_child(&pallet, &case2, &event_id).await.unwrap();

        let children = repo.get_children(&pallet).await.unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&case1));
        assert!(children.contains(&case2));
    }

    #[tokio::test]
    async fn test_get_ancestors() {
        let Some((repo, _container)) = setup_arango().await else {
            return;
        };

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P002");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C010");
        let unit1 = Epc::new("urn:epc:id:sgtin:0614141.107346.2099");
        let event_id = EventId::new();

        repo.add_child(&pallet, &case1, &event_id).await.unwrap();
        repo.add_child(&case1, &unit1, &event_id).await.unwrap();

        let ancestors = repo.get_ancestors(&unit1).await.unwrap();
        assert_eq!(ancestors.len(), 2);
        assert!(ancestors.contains(&case1));
        assert!(ancestors.contains(&pallet));
    }

    #[tokio::test]
    async fn test_full_hierarchy() {
        let Some((repo, _container)) = setup_arango().await else {
            return;
        };

        let pallet = Epc::new("urn:epc:id:sscc:0614141.P003");
        let case1 = Epc::new("urn:epc:id:sscc:0614141.C020");
        let case2 = Epc::new("urn:epc:id:sscc:0614141.C021");
        let unit1 = Epc::new("urn:epc:id:sgtin:0614141.107346.3001");
        let unit2 = Epc::new("urn:epc:id:sgtin:0614141.107346.3002");
        let event_id = EventId::new();

        repo.add_child(&pallet, &case1, &event_id).await.unwrap();
        repo.add_child(&pallet, &case2, &event_id).await.unwrap();
        repo.add_child(&case1, &unit1, &event_id).await.unwrap();
        repo.add_child(&case2, &unit2, &event_id).await.unwrap();

        let tree = repo.get_full_hierarchy(&pallet).await.unwrap();
        assert_eq!(tree.root, pallet);
        assert_eq!(tree.nodes.len(), 2); // two cases under pallet

        // Each case has one unit
        for node in &tree.nodes {
            assert_eq!(node.children.len(), 1);
        }
    }
}

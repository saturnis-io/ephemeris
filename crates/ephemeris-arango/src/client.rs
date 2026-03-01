use reqwest::Client;
use serde_json::{Value, json};
use thiserror::Error;

/// Errors from the ArangoDB HTTP client.
#[derive(Error, Debug)]
pub enum ArangoError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("ArangoDB error: {status} — {message}")]
    Arango { status: u16, message: String },

    #[error("authentication failed")]
    AuthFailed,
}

/// HTTP client wrapper for ArangoDB REST API.
///
/// All communication goes over HTTP — no embedded database engine.
pub struct ArangoClient {
    client: Client,
    base_url: String,
    database: String,
    auth_header: Option<String>,
}

impl ArangoClient {
    /// Connect with username/password authentication (JWT).
    pub async fn connect(
        base_url: &str,
        database: &str,
        username: &str,
        password: &str,
    ) -> Result<Self, ArangoError> {
        let client = Client::new();

        let auth_resp: Value = client
            .post(format!("{base_url}/_open/auth"))
            .json(&json!({"username": username, "password": password}))
            .send()
            .await?
            .json()
            .await?;

        let token = auth_resp["jwt"]
            .as_str()
            .ok_or(ArangoError::AuthFailed)?
            .to_string();

        Ok(Self {
            client,
            base_url: base_url.to_string(),
            database: database.to_string(),
            auth_header: Some(format!("bearer {token}")),
        })
    }

    /// Connect without authentication (for dev/test with ARANGO_NO_AUTH=1).
    pub fn connect_no_auth(base_url: &str, database: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            database: database.to_string(),
            auth_header: None,
        }
    }

    /// Build the database-scoped URL for a given path.
    fn db_url(&self, path: &str) -> String {
        format!("{}/_db/{}{}", self.base_url, self.database, path)
    }

    /// Build an authenticated request.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = self.db_url(path);
        let mut req = self.client.request(method, &url);
        if let Some(ref auth) = self.auth_header {
            req = req.header("Authorization", auth);
        }
        req
    }

    /// Create a named graph with a `contains` edge collection between `packaging` vertices.
    pub async fn create_graph(&self, name: &str) -> Result<(), ArangoError> {
        let resp = self
            .request(reqwest::Method::POST, "/_api/gharial")
            .json(&json!({
                "name": name,
                "edgeDefinitions": [{
                    "collection": "contains",
                    "from": ["packaging"],
                    "to": ["packaging"]
                }]
            }))
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 && status != 409 {
            let body: Value = resp.json().await.unwrap_or_default();
            return Err(ArangoError::Arango {
                status,
                message: body["errorMessage"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }
        Ok(())
    }

    /// Insert or update a vertex in the graph. Handles 409 conflict gracefully (vertex exists).
    pub async fn ensure_vertex(
        &self,
        graph: &str,
        collection: &str,
        key: &str,
        data: &Value,
    ) -> Result<(), ArangoError> {
        let mut doc = data.clone();
        doc["_key"] = json!(key);

        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/_api/gharial/{graph}/vertex/{collection}"),
            )
            .json(&doc)
            .send()
            .await?;

        let status = resp.status().as_u16();
        // 409 = conflict (already exists) — that's fine
        if status >= 400 && status != 409 {
            let body: Value = resp.json().await.unwrap_or_default();
            return Err(ArangoError::Arango {
                status,
                message: body["errorMessage"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }
        Ok(())
    }

    /// Insert an edge in the graph.
    pub async fn insert_edge(
        &self,
        graph: &str,
        collection: &str,
        from: &str,
        to: &str,
    ) -> Result<Value, ArangoError> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/_api/gharial/{graph}/edge/{collection}"),
            )
            .json(&json!({"_from": from, "_to": to}))
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 && status != 409 {
            let body: Value = resp.json().await.unwrap_or_default();
            return Err(ArangoError::Arango {
                status,
                message: body["errorMessage"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }

        let body: Value = resp.json().await?;
        Ok(body)
    }

    /// Delete an edge by its document handle (e.g. "contains/12345").
    pub async fn delete_edge(
        &self,
        graph: &str,
        collection: &str,
        edge_key: &str,
    ) -> Result<(), ArangoError> {
        let resp = self
            .request(
                reqwest::Method::DELETE,
                &format!("/_api/gharial/{graph}/edge/{collection}/{edge_key}"),
            )
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 && status != 404 {
            let body: Value = resp.json().await.unwrap_or_default();
            return Err(ArangoError::Arango {
                status,
                message: body["errorMessage"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }
        Ok(())
    }

    /// Delete a vertex by key.
    pub async fn delete_vertex(
        &self,
        graph: &str,
        collection: &str,
        key: &str,
    ) -> Result<(), ArangoError> {
        let resp = self
            .request(
                reqwest::Method::DELETE,
                &format!("/_api/gharial/{graph}/vertex/{collection}/{key}"),
            )
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 && status != 404 {
            let body: Value = resp.json().await.unwrap_or_default();
            return Err(ArangoError::Arango {
                status,
                message: body["errorMessage"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }
        Ok(())
    }

    /// Execute an AQL query and return the result set.
    pub async fn execute_aql(
        &self,
        query: &str,
        bind_vars: &Value,
    ) -> Result<Vec<Value>, ArangoError> {
        let resp = self
            .request(reqwest::Method::POST, "/_api/cursor")
            .json(&json!({
                "query": query,
                "bindVars": bind_vars,
                "batchSize": 1000
            }))
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body: Value = resp.json().await?;

        if status >= 400 {
            return Err(ArangoError::Arango {
                status,
                message: body["errorMessage"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }

        let results = body["result"].as_array().cloned().unwrap_or_default();
        Ok(results)
    }

    /// GET a URL (used for health checks).
    pub async fn get(&self, path: &str) -> Result<Value, ArangoError> {
        let resp = self.request(reqwest::Method::GET, path).send().await?;

        let status = resp.status().as_u16();
        let body: Value = resp.json().await?;

        if status >= 400 {
            return Err(ArangoError::Arango {
                status,
                message: body["errorMessage"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }

        Ok(body)
    }
}

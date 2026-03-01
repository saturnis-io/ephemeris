use thiserror::Error;

/// Errors that can occur in repository operations.
#[derive(Error, Debug)]
pub enum RepoError {
    #[error("event not found: {0}")]
    NotFound(String),

    #[error("duplicate event: {0}")]
    Duplicate(String),

    #[error("connection error: {0}")]
    Connection(String),

    #[error("query error: {0}")]
    Query(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("internal error: {0}")]
    Internal(String),
}

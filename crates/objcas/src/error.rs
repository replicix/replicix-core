// SPDX-License-Identifier: MPL-2.0
use thiserror::Error;

/// Errors of the object-store layer. The variants and messages mirror the
/// corresponding `StoreError` variants of the project this code was
/// extracted from, so a caller can map them one to one (see MIGRATION.md).
#[derive(Debug, Error)]
pub enum Error {
    #[error("object store: {0}")]
    ObjectStore(#[from] object_store::Error),

    #[error("conditional write refused: the object changed since it was read")]
    CasConflict,

    /// A control object that is present but unusable (an implausible
    /// lease epoch, ...).
    #[error("meta.json: {0}")]
    Meta(String),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("AWS credentials: {0}")]
    AwsCredentials(String),
}

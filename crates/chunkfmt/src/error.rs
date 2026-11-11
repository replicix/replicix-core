// SPDX-License-Identifier: MPL-2.0
use thiserror::Error;

/// Errors of the chunk object format, the codecs and the E2E key hierarchy.
///
/// The variants and messages mirror the corresponding `StoreError` variants
/// of the project this code was extracted from, so a caller can map them
/// one to one (see MIGRATION.md).
#[derive(Debug, Error)]
pub enum Error {
    #[error("corrupt chunk object: {0}")]
    CorruptObject(String),

    #[error("unknown codec id {0}; upgrade this software to read this object")]
    UnknownCodec(u16),

    #[error("compression: {0}")]
    Compression(String),

    /// A keyring block that cannot be created or opened (bad Argon2
    /// parameters, KDF failure).
    #[error("keyring: {0}")]
    Keyring(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

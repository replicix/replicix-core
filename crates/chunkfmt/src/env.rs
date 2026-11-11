// SPDX-License-Identifier: MPL-2.0
//! Environment overrides: `REPLICIX_<NAME>`.

/// `REPLICIX_<name>`.
pub(crate) fn var(name: &str) -> Option<String> {
    std::env::var(format!("REPLICIX_{name}")).ok()
}

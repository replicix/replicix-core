// SPDX-License-Identifier: MPL-2.0
//! Environment overrides: `REPLICIX_<NAME>`, and optionally the same name
//! under one legacy prefix a host application registers so that its
//! existing deployments keep working.

use std::sync::OnceLock;

static LEGACY_PREFIX: OnceLock<&'static str> = OnceLock::new();

/// Also honour `<prefix>_<NAME>` (after `REPLICIX_<NAME>`). Call once,
/// before the first use of any setting; returns `false` if a prefix was
/// already registered.
pub fn set_legacy_env_prefix(prefix: &'static str) -> bool {
    LEGACY_PREFIX.set(prefix).is_ok()
}

/// The legacy prefix, if one is registered.
pub fn legacy_env_prefix() -> Option<&'static str> {
    LEGACY_PREFIX.get().copied()
}

/// `REPLICIX_<name>`, else `<legacy>_<name>`.
pub(crate) fn var(name: &str) -> Option<String> {
    std::env::var(format!("REPLICIX_{name}")).ok().or_else(|| {
        LEGACY_PREFIX
            .get()
            .and_then(|p| std::env::var(format!("{p}_{name}")).ok())
    })
}

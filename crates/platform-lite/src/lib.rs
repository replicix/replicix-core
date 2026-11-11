// SPDX-License-Identifier: MPL-2.0
//! The host primitives a daemon needs and nothing else: advisory file locks
//! ([`lock`]), local-filesystem primitives (hole punching, preallocation,
//! fsync strength, page-cache advice, extended attributes; [`fs`]) and
//! core-dump suppression ([`coredump`]).
//!
//! Secret stores, D-Bus/logind lifecycle sources, directory conventions,
//! daemonizing and the mount table are *not* here; they stay with the
//! application (see README.md).

pub mod coredump;
pub mod fs;
pub mod lock;

pub use coredump::{core_dumps_forbidden, forbid_core_dumps};
pub use fs::FsPrimitives;
#[cfg(target_os = "linux")]
pub use fs::NativeFs;
#[cfg(unix)]
pub use lock::FlockLock;
pub use lock::{open_lock_file, FileLock, LockGuard};

/// `io::ErrorKind::Unsupported` naming what this host lacks.
pub fn unsupported(what: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("{what} is not supported on this host"),
    )
}

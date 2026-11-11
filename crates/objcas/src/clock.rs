// SPDX-License-Identifier: MPL-2.0
//! The wall clock the lease timestamps read, injectable.
//!
//! [`SystemClock`] is the default. An application that has its own notion
//! of protocol time (or a test that wants a manual clock) installs one with
//! [`set_clock`]; [`now_unix_ms`] and everything that stamps a lease reads
//! it.

use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// A source of wall-clock time in milliseconds since the unix epoch.
pub trait Clock: Send + Sync {
    fn now_unix_ms(&self) -> i64;
}

/// `SystemTime::now()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            Err(e) => -(e.duration().as_millis() as i64),
        }
    }
}

static CLOCK: RwLock<Option<Arc<dyn Clock>>> = RwLock::new(None);

/// Install `clock` as the process clock, returning the previous one
/// (`None`: the system clock).
pub fn set_clock(clock: Arc<dyn Clock>) -> Option<Arc<dyn Clock>> {
    CLOCK.write().unwrap().replace(clock)
}

/// Back to the system clock.
pub fn reset_clock() {
    *CLOCK.write().unwrap() = None;
}

/// The installed clock's reading.
pub fn now_unix_ms() -> i64 {
    match CLOCK.read().unwrap().as_ref() {
        Some(c) => c.now_unix_ms(),
        None => SystemClock.now_unix_ms(),
    }
}

// SPDX-License-Identifier: MPL-2.0
//! Conditional-write (CAS) primitives over `object_store`: what a
//! conditional PUT's error codes mean, the error classifier (transient or
//! permanent), the S3 client construction (credential chain bridge, 30 s
//! retry cap, error-body redaction), partition leases, the backend probes
//! and the preflight checks.
//!
//! Nothing here knows about a particular file system or volume layout;
//! application errors join the classifier through
//! [`classify::register_error_classifier`] and application time through
//! [`clock::set_clock`].

pub mod aws_auth;
pub mod cas;
pub mod classify;
pub mod clock;
pub mod control;
pub mod env;
pub mod error;
#[cfg(test)]
pub(crate) mod faulty;
pub mod lease;
pub mod preflight;
pub mod probe;
#[cfg(test)]
pub(crate) mod scripted_http;

pub use aws_auth::{
    amazon_s3_builder, amazon_s3_builder_resolved, amazon_s3_builder_resolved_with,
    configure_s3_client, S3Resolution, SuppliedCredentials,
};
pub use cas::{CasCode, CasPut, Verify};
pub use classify::{
    classify, classify_chain, classify_message, register_error_classifier, ErrorClass,
};
pub use clock::{Clock, SystemClock};
pub use error::Error;
pub use lease::{
    fence_retired, live_leases_held_by, AckPolicy, InheritedTail, Lease, LeaseMode, LeaseStore,
    LeaseTag, RenewalKey,
};
pub use preflight::{Capabilities, PreflightCheck};
pub use probe::{probe_cas_semantics, CasProbe, CasProbeReport, Versioning};

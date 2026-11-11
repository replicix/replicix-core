// SPDX-License-Identifier: MPL-2.0
//! A lease object written by the originating project's code parses here
//! and serializes back to the same bytes (tests/fixtures/README.md).

use replicix_objcas::lease::{AckPolicy, Lease};

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/lease.json"
    ))
    .unwrap()
}

#[test]
fn lease_bytes_round_trip() {
    let bytes = fixture();
    let lease: Lease = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        (lease.holder, lease.epoch, lease.ack_policy),
        (7, 3, AckPolicy::S3)
    );
    assert_eq!(lease.retired, [13]);
    assert_eq!(lease.inherited_tail.as_ref().unwrap().through, 99);
    assert!(lease.inherited_tail.as_ref().unwrap().kept);
    assert_eq!(lease.retired_tenure(), None);
    assert!(lease.singleton && lease.granted_delegations);
    assert_eq!(serde_json::to_vec(&lease).unwrap(), bytes);
}

/// An older body without the optional fields still reads, with their
/// defaults.
#[test]
fn an_older_body_reads_with_defaults() {
    let old = r#"{"v":1,"partition":"p","holder":1,"epoch":1,"expires_unix_ms":5,"released":false,"wanted_by":[],"backups":[],"config_version":0,"ack_policy":"Local","granted_delegations":false}"#;
    let l: Lease = serde_json::from_str(old).unwrap();
    assert_eq!((l.ttl_ms, l.holder_clock, l.singleton), (0, 0, false));
    assert!(l.retired.is_empty() && l.inherited_tail.is_none());
}

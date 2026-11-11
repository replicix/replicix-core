// SPDX-License-Identifier: MPL-2.0
//! Writes a lease built here, for the originating project's code to verify:
//! `cargo run -p replicix-objcas --example golden_out -- <outdir>`
//! (see tests/fixtures/README.md).

use replicix_objcas::lease::{AckPolicy, InheritedTail, Lease};

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("outdir"));
    std::fs::create_dir_all(&out).unwrap();
    let mut l = Lease::granted("p0", 7, 3, 60_000);
    l.expires_unix_ms = 1_800_000_000_000;
    l.holder_expires_ms = 1_800_000_000_500;
    l.holder_clock = 11;
    l.holder_wait_ms = 42;
    l.wanted_by = vec![2, 9];
    l.backups = vec![4, 5];
    l.config_version = 6;
    l.ack_policy = AckPolicy::S3;
    l.granted_delegations = true;
    l.retired = vec![13];
    l.inherited_tail = Some(Box::new(InheritedTail {
        epoch: 2,
        config_version: 5,
        members: vec![3, 4],
        through: 99,
        kept: true,
        holder: 7,
    }));
    l.singleton = true;
    std::fs::write(out.join("lease.json"), serde_json::to_vec(&l).unwrap()).unwrap();
}

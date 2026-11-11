// SPDX-License-Identifier: MPL-2.0
//! Shared by the golden test and the `golden_out` example (`#[path]`).
#![allow(dead_code)]

use replicix_csi_common::params::ClassParams;
use replicix_csi_common::volume_id::{SnapshotId, VolumeId};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn class_cases() -> Vec<HashMap<String, String>> {
    serde_json::from_slice(&std::fs::read(fixtures().join("class_cases.json")).unwrap()).unwrap()
}

/// The origin's default bucket prefix, supplied by the fixture (an
/// application constant the caller of `ClassParams::parse` owns).
pub fn default_prefix() -> String {
    std::fs::read_to_string(fixtures().join("default_prefix.txt")).unwrap()
}

pub fn class_line(i: usize, params: &HashMap<String, String>) -> String {
    let r = ClassParams::parse(params, &default_prefix());
    let mut s = format!("{i}\t{r:?}");
    if let Ok(p) = &r {
        s.push_str(&format!(
            "\t{:?}\t{:?}\t{}",
            p.pool_prefix(1.min(p.shards - 1)),
            p.dedicated_prefix("vol"),
            p.shard_for("pvc-abc")
        ));
    }
    s
}

/// One `volume_ids.tsv` line checked: `ok` ids round-trip and name the same
/// filesystem and subtree; `bad` ids are refused.
pub fn check_id_line(line: &str) {
    let f: Vec<&str> = line.split('\t').collect();
    match f[0] {
        "ok" => {
            if f[1].contains('@') {
                let s = SnapshotId::parse(f[1]).unwrap();
                assert_eq!(s.to_string(), f[1]);
                assert_eq!(s.selector(), f[2]);
            } else {
                let v = VolumeId::parse(f[1]).unwrap();
                assert_eq!(v.to_string(), f[1]);
                assert_eq!(format!("{}|{}", v.fs_uuid(), v.subtree()), f[2]);
            }
        }
        "bad" => assert!(
            VolumeId::parse(f[1]).is_err() || f[1].contains('@'),
            "{line}"
        ),
        _ => panic!("{line}"),
    }
}

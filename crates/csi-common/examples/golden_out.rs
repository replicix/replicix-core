// SPDX-License-Identifier: MPL-2.0
//! Writes this crate's reading of the golden inputs for the originating
//! project's code to verify: `cargo run -p replicix-csi-common --example
//! golden_out -- <outdir>` (see tests/fixtures/README.md).

#[path = "../tests/common/mod.rs"]
mod common;

use common::*;

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("outdir"));
    std::fs::create_dir_all(&out).unwrap();
    let lines: Vec<String> = class_cases()
        .iter()
        .enumerate()
        .map(|(i, c)| class_line(i, c))
        .collect();
    std::fs::write(out.join("class_params.tsv"), lines.join("\n") + "\n").unwrap();
    // The ok ids are minted by this crate's types; the refused ones are
    // the fixture's own list, which must stay refused.
    use replicix_csi_common::volume_id::{SnapshotId, VolumeId};
    let fs_uuid = "4f9c1e2a-0b1c-4d5e-8f90-1a2b3c4d5e6f";
    let pool = VolumeId::Pool {
        shard: 2,
        fs_uuid: fs_uuid.into(),
        name: "pvc-1a2b".into(),
    };
    let mut ids = vec![];
    for v in [
        pool.clone(),
        VolumeId::Dedicated {
            fs_uuid: fs_uuid.into(),
        },
        VolumeId::Static {
            fs_uuid: fs_uuid.into(),
            path: "/a/b".into(),
        },
        VolumeId::Static {
            fs_uuid: fs_uuid.into(),
            path: "/".into(),
        },
    ] {
        ids.push(format!("ok\t{v}\t{}|{}", v.fs_uuid(), v.subtree()));
    }
    let snap = SnapshotId {
        volume: pool,
        name: "snap-1".into(),
    };
    ids.push(format!("ok\t{snap}\t{}", snap.selector()));
    for line in std::fs::read_to_string(fixtures().join("volume_ids.tsv"))
        .unwrap()
        .lines()
    {
        if line.starts_with("bad") {
            check_id_line(line);
            ids.push(line.to_string());
        }
    }
    std::fs::write(out.join("volume_ids.tsv"), ids.join("\n") + "\n").unwrap();
}

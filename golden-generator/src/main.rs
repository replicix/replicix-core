// SPDX-License-Identifier: MPL-2.0
//! Generates (`gen`) and verifies (`verify`) the golden fixtures that pin
//! replicix-core's formats to Constellation's own code.
//!
//! `golden-gen gen <outdir>`: runs Constellation's encode/encrypt/derive/
//! volume-id/class-parameter/lease code over fixed inputs and writes the
//! results; replicix-core's tests decode them byte for byte.
//!
//! `golden-gen verify <dir>`: reads what replicix-core's `golden_out`
//! examples wrote and checks it with Constellation's code (the other
//! direction).

use constellation_csi::params::ClassParams;
use constellation_csi::volume_id::{SnapshotId, VolumeId};
use constellation_fs_core::{ChunkHash, ChunkLayout};
use constellation_store_s3::codec::CompressionSetting;
use constellation_store_s3::e2e::{
    create_keyring_block, decrypt_object, encrypt_object, unlock, KeyringBlock,
};
use constellation_store_s3::format::{decode_object, encode_object};
use constellation_store_s3::lease::{AckPolicy, InheritedTail, Lease};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const PASSPHRASE: &str = "golden passphrase";
const UUID: &str = "00000000-0000-4000-8000-000000000001";
const SETTINGS: [(&str, &str); 3] = [("raw", "raw"), ("zstd3", "zstd:3"), ("zstd19", "zstd:19")];

fn plains() -> Vec<(&'static str, Vec<u8>)> {
    let noise: Vec<u8> = (0..1024u32)
        .flat_map(|i| i.wrapping_mul(2654435761).to_le_bytes())
        .collect();
    let text = "the quick brown fox jumps over the lazy dog\n".repeat(900);
    vec![
        ("zeros64k", vec![0u8; 65536]),
        ("text", text.into_bytes()),
        ("noise", noise),
        ("empty", Vec::new()),
        ("single", b"x".to_vec()),
    ]
}

fn write(dir: &Path, name: &str, data: impl AsRef<[u8]>) {
    let p = dir.join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, data).unwrap();
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn class_cases() -> Vec<HashMap<String, String>> {
    let c = |kv: &[(&str, &str)]| -> HashMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    };
    vec![
        c(&[("bucket", "b")]),
        c(&[
            ("bucket", "b"),
            ("prefix", "/team/x/"),
            ("shards", "4"),
            ("chunkSize", "4MiB"),
            ("e2e", "true"),
            ("writeMode", "strict"),
            ("endpoint", "http://s3.example:9000"),
            ("region", "eu-west-1"),
            ("compression", "zstd:7"),
            ("csi.storage.k8s.io/pvc/name", "ignored"),
        ]),
        c(&[("bucket", "b"), ("layout", "dedicated"), ("credentialSource", "aws-default-chain")]),
        c(&[
            ("bucket", "b"),
            ("credentialSource", "refreshing"),
            ("credentialSecretName", "creds"),
            ("credentialSecretNamespace", "ns"),
        ]),
        c(&[("bucket", "b"), ("shards", "0")]),
        c(&[("bucket", "a/b")]),
        c(&[("bucket", "b"), ("layout", "zfs")]),
        c(&[("bucket", "b"), ("chunkSize", "12parsecs")]),
        c(&[("bucket", "b"), ("surprise", "1")]),
    ]
}

fn class_line(i: usize, params: &HashMap<String, String>) -> String {
    let r = ClassParams::parse(params);
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

fn lease_value() -> Lease {
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
    l
}

fn gen(out: &Path) {
    // ---- chunkfmt: objects, hashes, slices
    let cf = out.join("chunkfmt");
    let key = [7u8; 32];
    let mut hashes = serde_json::Map::new();
    for (name, plain) in plains() {
        write(&cf, &format!("plain/{name}.bin"), &plain);
        for (suffix, setting) in SETTINGS {
            let s: CompressionSetting = setting.parse().unwrap();
            write(
                &cf,
                &format!("objects/{name}.{suffix}.cch1"),
                encode_object(&plain, s).unwrap(),
            );
        }
        hashes.insert(
            name.into(),
            json!({"plain": ChunkHash::of(&plain).to_hex(), "keyed7": ChunkHash::keyed(&key, &plain).to_hex()}),
        );
    }
    write(&cf, "hashes.json", serde_json::to_vec_pretty(&Value::Object(hashes)).unwrap());
    let mut slices = vec![];
    for (cs, off, len) in [
        (4u32 << 20, 0u64, 0u64),
        (4 << 20, (4 << 20) - 10, 20),
        (4 << 20, 100, 50),
        (4 << 20, 4 << 20, 4 << 20),
        (65536, 10, 300_000),
        (1 << 20, 3 * (1 << 20) + 5, 1),
    ] {
        let l = ChunkLayout::new(cs);
        let v: Vec<Value> = l
            .slices(off, len)
            .map(|s| json!([s.index, s.offset, s.len]))
            .collect();
        slices.push(json!({"chunk_size": cs, "offset": off, "len": len, "slices": v,
            "count": l.chunk_count(off + len), "last_len": if off + len > 0 { l.chunk_len(off + len, l.chunk_count(off + len) - 1) } else { 0 }}));
    }
    write(&cf, "slices.json", serde_json::to_vec_pretty(&slices).unwrap());

    // ---- chunkfmt: e2e
    let block = create_keyring_block(PASSPHRASE).unwrap();
    let keys = unlock(&block, PASSPHRASE).unwrap();
    write(&cf, "e2e/keyring.json", serde_json::to_vec_pretty(&block).unwrap());
    write(&cf, "e2e/passphrase.txt", PASSPHRASE);
    // The labels as bytes, for the replicix test domain (not edited by hand).
    write(&cf, "e2e/kdf_domain.bin", b"constellation/e2e/v1/");
    write(&cf, "e2e/pin_context.txt", "constellation 2026 e2e master pin v1");
    let text = &plains()[1].1;
    let h = keys.hash(text);
    write(
        &cf,
        "e2e/derived.json",
        serde_json::to_vec_pretty(&derived(&keys.addressing_key()[..], &keys.gossip_secret()[..], &|n| keys.dek(n).to_vec(), &keys.pin_fingerprint(UUID), &h.to_hex())).unwrap(),
    );
    let sealed = encrypt_object(
        &keys.dek("p0"),
        &h.0,
        &encode_object(text, "zstd:3".parse().unwrap()).unwrap(),
    )
    .unwrap();
    write(&cf, "e2e/sealed_text.bin", sealed);

    // ---- objcas: a lease
    write(
        &out.join("objcas"),
        "lease.json",
        serde_json::to_vec(&lease_value()).unwrap(),
    );

    // ---- csi-common: volume ids and class parameters
    let cc = out.join("csi-common");
    let fs_uuid = "4f9c1e2a-0b1c-4d5e-8f90-1a2b3c4d5e6f";
    let pool = VolumeId::Pool { shard: 2, fs_uuid: fs_uuid.into(), name: "pvc-1a2b".into() };
    let ids = [
        pool.to_string(),
        VolumeId::Dedicated { fs_uuid: fs_uuid.into() }.to_string(),
        VolumeId::Static { fs_uuid: fs_uuid.into(), path: "/a/b".into() }.to_string(),
        VolumeId::Static { fs_uuid: fs_uuid.into(), path: "/".into() }.to_string(),
        SnapshotId { volume: pool, name: "snap-1".into() }.to_string(),
    ];
    let mut lines = vec![];
    for id in ids {
        let sub = if id.contains('@') {
            SnapshotId::parse(&id).map(|s| s.selector()).unwrap()
        } else {
            let v = VolumeId::parse(&id).unwrap();
            format!("{}|{}", v.fs_uuid(), v.subtree())
        };
        lines.push(format!("ok\t{id}\t{sub}"));
    }
    for bad in [
        "", "nopath", "v2/pool/0/x/volumes/n", "v1/pool/01/x/volumes/n", "v1/pool/0/x/other/n",
        "v1/pool/0/x/volumes/..", "v1/dedicated/x", "v1/weird/x", "u/../x", "a b/c", "x/@y",
    ] {
        assert!(
            VolumeId::parse(bad).is_err() || bad.contains('@'),
            "{bad} unexpectedly parses"
        );
        lines.push(format!("bad\t{bad}\t"));
    }
    write(&cc, "volume_ids.tsv", lines.join("\n") + "\n");
    let cases = class_cases();
    write(&cc, "class_cases.json", serde_json::to_vec_pretty(&cases).unwrap());
    write(&cc, "default_prefix.txt", constellation_csi::params::DEFAULT_PREFIX);
    let cl: Vec<String> = cases.iter().enumerate().map(|(i, c)| class_line(i, c)).collect();
    write(&cc, "class_params.tsv", cl.join("\n") + "\n");
    println!("fixtures written to {}", out.display());
}

fn derived(
    addr: &[u8],
    gossip: &[u8],
    dek: &dyn Fn(&str) -> Vec<u8>,
    pin: &str,
    keyed: &str,
) -> Value {
    json!({
        "addressing": hex(addr), "gossip": hex(gossip),
        "dek_p0": hex(&dek("p0")), "dek_empty": hex(&dek("")),
        "dek_mtree_nodes": hex(&dek("mtree/nodes")), "dek_long": hex(&dek(&"p".repeat(300))),
        "pin": pin, "pin_uuid": UUID, "keyed_hash_text": keyed,
    })
}

fn verify(dir: &Path) {
    let mut checked = 0;
    // chunkfmt objects: replicix encoded, Constellation decodes (and the
    // deterministic encodings are byte-equal to Constellation's own).
    let cf = dir.join("chunkfmt");
    for (name, plain) in plains() {
        for (suffix, setting) in SETTINGS {
            let obj = fs::read(cf.join(format!("objects/{name}.{suffix}.cch1"))).unwrap();
            assert_eq!(decode_object(&obj).unwrap(), plain, "{name}.{suffix}");
            let ours = encode_object(&plain, setting.parse().unwrap()).unwrap();
            assert_eq!(obj, ours, "{name}.{suffix} bytes differ");
            checked += 1;
        }
    }
    // e2e: replicix made the keyring and sealed object.
    let block: KeyringBlock = serde_json::from_slice(&fs::read(cf.join("e2e/keyring.json")).unwrap()).unwrap();
    let keys = unlock(&block, PASSPHRASE).unwrap();
    let text = &plains()[1].1;
    let h = keys.hash(text);
    let want = derived(&keys.addressing_key()[..], &keys.gossip_secret()[..], &|n| keys.dek(n).to_vec(), &keys.pin_fingerprint(UUID), &h.to_hex());
    let got: Value = serde_json::from_slice(&fs::read(cf.join("e2e/derived.json")).unwrap()).unwrap();
    assert_eq!(want, got, "derived keys differ");
    let sealed = fs::read(cf.join("e2e/sealed_text.bin")).unwrap();
    let opened = decrypt_object(&keys.dek("p0"), &h.0, &sealed).unwrap();
    assert_eq!(decode_object(&opened).unwrap(), *text);
    checked += 2;
    // lease
    let lease: Lease = serde_json::from_slice(&fs::read(dir.join("objcas/lease.json")).unwrap()).unwrap();
    assert_eq!(lease, lease_value());
    assert_eq!(serde_json::to_vec(&lease).unwrap(), fs::read(dir.join("objcas/lease.json")).unwrap());
    checked += 1;
    // csi-common: class parameter lines and volume ids
    let cc = dir.join("csi-common");
    let theirs = fs::read_to_string(cc.join("class_params.tsv")).unwrap();
    let ours: Vec<String> = class_cases().iter().enumerate().map(|(i, c)| class_line(i, c)).collect();
    assert_eq!(theirs.trim_end().split('\n').collect::<Vec<_>>(), ours);
    for line in fs::read_to_string(cc.join("volume_ids.tsv")).unwrap().lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "ok" => {
                if f[1].contains('@') {
                    let s = SnapshotId::parse(f[1]).unwrap();
                    assert_eq!((s.to_string(), s.selector()), (f[1].to_string(), f[2].to_string()));
                } else {
                    let v = VolumeId::parse(f[1]).unwrap();
                    assert_eq!(v.to_string(), f[1]);
                    assert_eq!(format!("{}|{}", v.fs_uuid(), v.subtree()), f[2]);
                }
            }
            "bad" => assert!(VolumeId::parse(f[1]).is_err() || f[1].contains('@')),
            _ => panic!("{line}"),
        }
        checked += 1;
    }
    println!("verified {checked} replicix-produced fixtures with Constellation's code");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(args.get(2).expect("usage: golden-gen gen|verify <dir>"));
    match args.get(1).map(String::as_str) {
        Some("gen") => gen(&dir),
        Some("verify") => verify(&dir),
        _ => panic!("usage: golden-gen gen|verify <dir>"),
    }
}

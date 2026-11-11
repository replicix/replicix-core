// SPDX-License-Identifier: MPL-2.0
//! Shared by the golden test and the `golden_out` example (included with
//! `#[path]`): the fixture inputs and the key domain the fixtures were made
//! under, read from fixture files so no source file spells the labels.
#![allow(dead_code)]

use replicix_chunkfmt::KeyDomain;
use std::path::PathBuf;

pub const PASSPHRASE: &str = "golden passphrase";
pub const UUID: &str = "00000000-0000-4000-8000-000000000001";
pub const SETTINGS: [(&str, &str); 3] =
    [("raw", "raw"), ("zstd3", "zstd:3"), ("zstd19", "zstd:19")];
pub const NAMES: [&str; 5] = ["zeros64k", "text", "noise", "empty", "single"];

/// The key domain of the fixtures' origin.
pub struct OriginDomain;

impl KeyDomain for OriginDomain {
    const KDF_DOMAIN: &'static [u8] = include_bytes!("../fixtures/e2e/kdf_domain.bin");
    const PIN_CONTEXT: &'static str = include_str!("../fixtures/e2e/pin_context.txt");
}

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn plain(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(format!("plain/{name}.bin"))).unwrap()
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn derived(
    addr: &[u8],
    gossip: &[u8],
    dek: &dyn Fn(&str) -> Vec<u8>,
    pin: &str,
    keyed: &str,
) -> serde_json::Value {
    serde_json::json!({
        "addressing": hex(addr), "gossip": hex(gossip),
        "dek_p0": hex(&dek("p0")), "dek_empty": hex(&dek("")),
        "dek_mtree_nodes": hex(&dek("mtree/nodes")), "dek_long": hex(&dek(&"p".repeat(300))),
        "pin": pin, "pin_uuid": UUID, "keyed_hash_text": keyed,
    })
}

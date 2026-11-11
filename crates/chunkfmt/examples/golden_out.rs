// SPDX-License-Identifier: MPL-2.0
//! Writes what this crate produces for the golden inputs, for the
//! originating project's code to verify (the "vice versa" direction):
//! `cargo run -p replicix-chunkfmt --example golden_out -- <outdir>`
//! (see tests/fixtures/README.md).

#[path = "../tests/common/mod.rs"]
mod common;

use common::*;
use replicix_chunkfmt::e2e::encrypt_object;
use replicix_chunkfmt::format::encode_object;
use replicix_chunkfmt::{create_keyring_block, unlock, CompressionSetting};
use std::fs;

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("outdir"));
    let write = |name: &str, data: &[u8]| {
        let p = out.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    };
    for name in NAMES {
        let plain = plain(name);
        for (suffix, setting) in SETTINGS {
            let obj =
                encode_object(&plain, setting.parse::<CompressionSetting>().unwrap()).unwrap();
            write(&format!("objects/{name}.{suffix}.cch1"), &obj);
        }
    }
    let block = create_keyring_block(PASSPHRASE).unwrap();
    let keys = unlock::<OriginDomain>(&block, PASSPHRASE).unwrap();
    write(
        "e2e/keyring.json",
        &serde_json::to_vec_pretty(&block).unwrap(),
    );
    let text = plain("text");
    let h = keys.hash(&text);
    let d = derived(
        &keys.addressing_key()[..],
        &keys.gossip_secret()[..],
        &|n| keys.dek(n).to_vec(),
        &keys.pin_fingerprint(UUID),
        &h.to_hex(),
    );
    write("e2e/derived.json", &serde_json::to_vec_pretty(&d).unwrap());
    let sealed = encrypt_object(
        &keys.dek("p0"),
        &h.0,
        &encode_object(&text, "zstd:3".parse().unwrap()).unwrap(),
    )
    .unwrap();
    write("e2e/sealed_text.bin", &sealed);
}

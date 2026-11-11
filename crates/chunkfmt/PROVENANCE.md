# Provenance: replicix-chunkfmt

Extracted from the Constellation repository (https://github.com/replicix/constellation) at commit
`15ea0c51` (the exact commit the files were copied from).
Constellation's later changes to these files are not included; diff the sources
below against this crate to find them.

## Source files

- `crates/fs-core/src/chunk.rs` -> `src/chunk.rs` (`ChunkHash`, `ChunkSlice`, `ChunkLayout`)
- `crates/store-s3/src/format.rs` -> `src/format.rs`
- `crates/store-s3/src/codec.rs` -> `src/codec.rs`
- `crates/store-s3/src/e2e.rs` -> `src/e2e.rs`
- `crates/store-s3/src/error.rs`: the variants these modules use -> `src/error.rs`

## What changed

- `StoreError` -> `Error` (`CorruptObject`, `UnknownCodec`, `Compression`, `Keyring` (was `Meta`), `Io`). Two messages say "upgrade this software" instead of naming the product.
- `e2e`: the derivation labels are a type parameter (`KeyDomain`) instead of constants; `TreeSealing` stays in Constellation. Derived bytes are identical for a domain carrying Constellation's labels (golden tests).
- `codec`: `CODEC_LZ4 = 2` and `Codec::Lz4` behind the `lz4` feature (off by default; with it off, id 2 is an unknown codec as before).
- `codec::MAX_DECOMPRESSED_ENV` is `REPLICIX_MAX_DECOMPRESSED_BYTES`; a legacy prefix can be registered.
- `ChunkHash::keyed` etc. unchanged.

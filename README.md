# replicix-core

Shared Rust crates of the Replicix projects (MPL-2.0): Constellation (the distributed file
system) and Lunix (the ublk block-device daemon) depend on them instead of keeping copies.
The crates were extracted without behavioural change from Constellation at commit
`15ea0c51` (each crate's `PROVENANCE.md` lists the source files and what changed); golden
tests pin the formats against fixtures produced by Constellation's own code.
`MIGRATION.md` is the plan for switching Constellation over.

| crate | what it is |
|---|---|
| `replicix-upload-concurrency` | AIMD search for upload concurrency (`lib`) and the elastic `Gate` that enforces it |
| `replicix-objcas` | conditional writes over `object_store`: `cas` (what 412/409/404 mean, `put_conditional`, `create_content_addressed`), `classify` (transient or permanent), `aws_auth` (SDK credential chain bridge, 30 s retry cap, `RedactingConnector`), `lease` (CAS partition leases), `probe` (CAS semantics), `preflight` (backend capability checks), `control` (torn-read-safe JSON GET), `clock` |
| `replicix-chunkfmt` | `ChunkHash`/`ChunkLayout`, the `CCH1` object (`encode_object`, `decode_object`, `StreamingDecoder`), the codec registry (raw, zstd, lz4 with the `lz4` feature) with bounded decompression, the E2E key hierarchy |
| `replicix-platform-lite` | advisory file locks (`FileLock`, `FlockLock`), local fs primitives (`FsPrimitives`, `NativeFs`), `forbid_core_dumps` |
| `replicix-csi-common` | CSI v1 protobuf/gRPC bindings, StorageClass parameter helpers and `ClassParams`, volume-id codecs (`VolumeId`, `SnapshotId`, generic `IdPrefix`), the credentials Secret watch (`Refresher`) |

## Public API in brief

**objcas.** `cas::put_conditional(store, path, body, PutMode, Verify) -> CasPut` (`Won`/`Lost`/`Missing`;
409 retried, our own landed write detected by read-back); `classify(&dyn Error) -> ErrorClass`
(applications add their own error types with `register_error_classifier`);
`amazon_s3_builder*` / `configure_s3_client`; `Lease`, `LeaseStore::{get, try_create, try_swap}`,
`LeaseMode::{Cas, SingleWriter}`; `probe_cas_semantics`; `preflight::{preflight, probe_conditional_writes}`;
`Clock`/`set_clock` (the system clock unless injected); `Error`. Environment overrides:
`REPLICIX_LEASE_TTL_MS`, `REPLICIX_CAS_BUSY_RETRIES`, `REPLICIX_S3_MAX_RETRIES`,
`REPLICIX_S3_RETRY_TIMEOUT_MS`.

**chunkfmt.** `ChunkHash::{of, keyed, to_hex, from_hex}`, `ChunkLayout::{chunk_count, chunk_len, slices}`;
`format::{encode_object, decode_object, StreamingDecoder}`; `Codec`, `CompressionSetting` (`raw`, `zstd:N`,
`lz4` with the feature), `codec::{decompress, decompress_bounded, decompress_to_ceiling,
max_decompressed_len}` (`REPLICIX_MAX_DECOMPRESSED_BYTES`); `e2e::{E2eKeys<D>, KeyDomain,
ReplicixDomain, create_keyring_block, unlock::<D>, rewrap_master, encrypt_object, decrypt_object}`.
The key-derivation labels are the application's `KeyDomain` (a type parameter, never a default
shared between applications); `ReplicixDomain` is for new applications with no legacy bytes to keep.
Lunix's AES-GCM extent scheme is deliberately **not** here.

**platform-lite.** `lock::{FileLock, LockGuard, FlockLock, open_lock_file}`, `fs::{FsPrimitives, NativeFs}`
(punch hole, preallocate, fsync, `drop_cache`, xattrs; Linux), `coredump::{forbid_core_dumps,
core_dumps_forbidden}`. Feature-free: no zbus, no keyring.

**csi-common.** `volume_id::{VolumeId, SnapshotId, IdPrefix}` (`IdPrefix("lunix1:").encode(uuid)`);
`params::{ClassParams::parse(params, default_prefix), FsCreateSpec, volume_context, CredentialMode,
SecretRef, ...}`; `credentials::{Secrets, SecretSource, KubeSecrets (feature kube-secrets, default),
Refresher, fingerprint}`; `proto::csi::v1`.

## What stays private to each project

- **Constellation:** everything that needs `fs-core`/`mtree` types or its control protocol: the bucket
  layout (`layout.rs`), chunk/blob/pack/log/commit/inbox/node/designation/heartbeat/GC stores,
  `TreeSealing` (the metadata-tree sealing keys, built on `E2eKeys::dek`), `FsMeta`, `ChunkStore`, the
  protocol clock (it implements `objcas::Clock`), the platform host services (dirs, process facts,
  daemonizing, secret stores with D-Bus/keyring, lifecycle, mount table), the CSI driver's
  Controller/Node/engine-pod/purge code, `unlock_params`/`follow_rotations` (control-protocol types) and
  its `DEFAULT_PREFIX`.
- **Lunix:** its volume map and journal formats, the AES-GCM extent scheme, the ublk/io_uring daemon,
  its CSI services and `lunix1:` ids (built on `IdPrefix`), its `KeyDomain` if it ever shares a master
  with nothing else, its own `register_error_classifier` hook for `chunkfmt::Error` (permanent).

## Notes for users

- `ClassParams` is the file-system pool/dedicated StorageClass schema Constellation uses; Lunix uses the
  helpers (`parse_chunk_size`, `is_dns_subdomain`, `join`, `volume_context`, `SecretRef`, ...) and its own
  schema.
- `objcas::lease::Lease` carries Constellation's full lease protocol fields (backups, ack policy,
  inherited tail, honour clocks), all `serde(default)`-compatible; a single-writer user ignores them.
- The `object_store` version is pinned (`=0.14.1`): the classifier text-matches its error messages.
- Disk-format stability: changing a golden fixture is a format change; regenerate only on purpose
  (`golden-generator/README.md`).

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p replicix-chunkfmt --features lz4
```

`rust-toolchain.toml` pins the channel (1.98, as Constellation does). `Cargo.lock` is committed
and was seeded from Constellation's so shared dependencies resolve to the same versions.
`golden-generator/` is built inside a copy of the Constellation workspace, not here.

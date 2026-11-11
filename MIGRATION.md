# Switching Constellation to replicix-core

A direct switch, no compatibility layer: Constellation keeps no `pub use replicix_xxx::...`
modules. Every moved file is deleted and every call site imports from the replicix crate.
The extracted APIs are drop-in, so almost every rewrite is a path rename; the exceptions
are listed under "Signature and type differences" and are the only places that need
thought. Constellation's crate versions are **not** bumped.

Source of truth for what moved: each crate's `PROVENANCE.md` (Constellation commit
`15ea0c51`). Constellation's `main` moves daily: before starting, diff the moved files at
the migration base against `15ea0c51` (`git diff 15ea0c51 <base> -- <paths in the
PROVENANCE.md files>`) and port any change into replicix-core first.

**Synced so far:** every moved file matches Constellation `fc5d8be6`. The
only change since `15ea0c51` was `lease.rs` (ADR-51: `InheritedTail.kept`
and `holder`, `Lease::retired_tenure`). Diff from `fc5d8be6` next time.

## 0. Cargo changes (Constellation's workspace `Cargo.toml`)

Replace the moved path dependency with git dependencies pinned by `rev`:

```toml
[workspace.dependencies]
# removed: constellation-upload-concurrency = { path = "crates/upload-concurrency" }
replicix-upload-concurrency = { git = "https://github.com/replicix/replicix-core", rev = "<commit>" }
replicix-objcas             = { git = "https://github.com/replicix/replicix-core", rev = "<commit>" }
replicix-chunkfmt           = { git = "https://github.com/replicix/replicix-core", rev = "<commit>" }
replicix-platform-lite      = { git = "https://github.com/replicix/replicix-core", rev = "<commit>" }
replicix-csi-common         = { git = "https://github.com/replicix/replicix-core", rev = "<commit>" }

# Local development against a checkout of replicix-core (never commit this enabled):
# [patch."https://github.com/replicix/replicix-core"]
# replicix-upload-concurrency = { path = "../replicix-core/crates/upload-concurrency" }
# replicix-objcas             = { path = "../replicix-core/crates/objcas" }
# replicix-chunkfmt           = { path = "../replicix-core/crates/chunkfmt" }
# replicix-platform-lite      = { path = "../replicix-core/crates/platform-lite" }
# replicix-csi-common         = { path = "../replicix-core/crates/csi-common" }
```

Workspace `members` and `default-members`: remove `crates/upload-concurrency`.
`constellation-upload-concurrency` leaves `[workspace.dependencies]`.

Per-crate manifests (use `.workspace = true`):

| Constellation crate | add | drop (only if no use remains) |
|---|---|---|
| `store-s3` | `replicix-objcas`, `replicix-chunkfmt` | `argon2`, `chacha20poly1305`, `region`, `zstd`, `aws-config`, `aws-credential-types`, `aws-types`, `async-trait` |
| `fs-core` | `replicix-chunkfmt` | `blake3` |
| `platform` | `replicix-platform-lite` | nothing (zbus, zeroize stay) |
| `engine`, `authority`, `meta`, `cli`, `harness`, `bench/uploadbench` | whichever of the above they import (see the appendix) | `constellation-upload-concurrency` (engine, uploadbench) |
| `csi` | `replicix-csi-common`; dev-dependency `replicix-csi-common` with `features = ["test-support"]` | build deps `tonic-prost-build`, `protoc-bin-vendored`, and `prost`/`prost-types`/`tonic-prost` if only the generated code used them |

`csi` keeps `kube`/`k8s-openapi` (its own code uses them); replicix-csi-common brings the
same versions (`kube` 4.2, `k8s-openapi` 0.28 `v1_36`) through its default `kube-secrets`
feature. replicix-chunkfmt is used **without** its `lz4` feature: the codec registry then
behaves exactly as before (id 2 is an unknown codec).

### `[patch]` and vendor entries

None of Constellation's `[patch.crates-io]` entries (`fjall`, `lsm-tree`, `fuser`,
`netwatch`) touches a crate replicix-core depends on, so they neither apply to it nor
break it. The new git dependency adds a second git source next to the existing `fuser`
one: offline or container builds that pre-fetch git sources (the test Dockerfile, CI
caches) must fetch `replicix-core` too. `vendor/` is unaffected. `Cargo.lock` must keep
**`object_store` 0.14.1** (the classifier text-matches its errors; replicix-objcas pins
`=0.14.1`) and the other versions in replicix-core's `Cargo.lock`, which was seeded from
Constellation's: after the switch no existing package should need `cargo update`.

## 1. Files deleted outright

| Constellation file | now |
|---|---|
| `crates/upload-concurrency/` (whole crate) | `replicix-upload-concurrency` |
| `crates/store-s3/src/cas.rs` | `replicix_objcas::cas` |
| `crates/store-s3/src/classify.rs` | `replicix_objcas::classify` |
| `crates/store-s3/src/probe.rs` | `replicix_objcas::probe` |
| `crates/store-s3/src/aws_auth.rs` | `replicix_objcas::aws_auth` |
| `crates/store-s3/src/lease.rs` | `replicix_objcas::lease` |
| `crates/store-s3/src/control.rs` | `replicix_objcas::control` |
| `crates/store-s3/src/codec.rs` | `replicix_chunkfmt::codec` |
| `crates/store-s3/src/format.rs` | `replicix_chunkfmt::format` |
| `crates/fs-core/src/chunk.rs` | `replicix_chunkfmt::chunk` |
| `crates/platform/src/lock.rs` | `replicix_platform_lite::lock` |
| `crates/platform/src/fs.rs` | `replicix_platform_lite::fs` |
| `crates/csi/src/proto.rs`, `crates/csi/build.rs`, `crates/csi/proto/` | `replicix_csi_common::proto` (`csi.proto`, `LICENSE`, `README.md`, `VERSION` moved unmodified) |
| `crates/csi/src/volume_id.rs` | `replicix_csi_common::volume_id` |

`crates/store-s3/src/faulty.rs` and `scripted_http.rs` (`#[cfg(test)]`) are **kept**: the
tests of `inbox`, `log`, `store`, `designation` and others still use them (replicix-objcas
has its own crate-private copies).

Edited, not deleted (the moved part is cut out):

- `crates/store-s3/src/e2e.rs`: keeps `TreeSealing` and the adapters of 2.1; the rest moved.
- `crates/store-s3/src/error.rs`: the same variants, plus the two `From` impls of 2.2.
- `crates/store-s3/src/store.rs`: `preflight` and `probe_conditional_writes` become one-line
  wrappers; `PreflightCheck`, `Capabilities`, `concise_os_error` are cut (2.3).
  `is_missing_bucket` may become `replicix_objcas::preflight::is_missing_bucket`.
- `crates/store-s3/src/layout.rs`: `lease()` is cut (its one external user,
  `crates/engine/src/snapsched.rs`, uses `replicix_objcas::lease::lease_path`).
- `crates/store-s3/src/lib.rs`: drop the moved `pub mod` lines and the moved names from the
  `pub use` lists (the appendix shows which).
- `crates/fs-core/src/lib.rs`: drop `pub mod chunk;` and the `ChunkHash`/`ChunkLayout`/
  `ChunkSlice` re-exports (`DEFAULT_CHUNK_SIZE`, `validate_chunk_size` etc. stay).
- `crates/platform/src/{lib,secrets,unix,linux,macos,unsupported}.rs`: see 1.1.
- `crates/csi/src/{params,credentials,lib}.rs`: see 2.4.

### 1.1 platform

- `lock.rs` and `fs.rs` are deleted; `HostServices` and every implementor import
  `replicix_platform_lite::{FileLock, FsPrimitives, LockGuard}` instead of `crate::{lock, fs}`.
- `unix.rs`: delete `UnixFileLock` and its three tests. `linux.rs::file_lock()` becomes
  `Arc::new(replicix_platform_lite::FlockLock { holders: flock_holder_pids, opened_by: has_open })`;
  `macos.rs::file_lock()` likewise with its probes (or `FlockLock::new()`).
- `linux.rs`: delete `LinuxFs`, `fallocate` and the `FsPrimitives` impl; use
  `Arc::new(replicix_platform_lite::NativeFs)`. The xattr helpers in `unix.rs` stay only while
  `macos.rs::MacFs` uses them (drop their Linux `cfg` arms and the Linux xattr test; it moved).
- `secrets.rs`: delete `forbid_core_dumps` and `core_dumps_forbidden`; callers import them from
  `replicix_platform_lite` (appendix; also doc links such as
  `constellation_platform::forbid_core_dumps` in `crates/csi`).
- `lib.rs`: drop `pub mod fs; pub mod lock;` and the `pub use` of `FileLock`, `FsPrimitives`,
  `LockGuard`, `forbid_core_dumps`, `core_dumps_forbidden`. `unsupported()` stays (the other
  stubs use it).
- `try_lock`'s `Code::Again` test is `io::ErrorKind::WouldBlock` in replicix: the same errno on
  every supported unix, no behaviour change.

## 2. Adapter code Constellation keeps

### 2.1 E2E key domain (`store-s3/src/e2e.rs`)

The derivation labels are Constellation's own and must never change (they decide every derived
key of every existing filesystem). They live in Constellation now:

```rust
pub struct ConstellationDomain;
impl replicix_chunkfmt::KeyDomain for ConstellationDomain {
    const KDF_DOMAIN: &'static [u8] = b"constellation/e2e/v1/";
    const PIN_CONTEXT: &'static str = "constellation 2026 e2e master pin v1";
}
pub type E2eKeys = replicix_chunkfmt::E2eKeys<ConstellationDomain>;
pub type SharedE2eKeys = replicix_chunkfmt::SharedE2eKeys<ConstellationDomain>;
pub fn unlock(block: &KeyringBlock, passphrase: &str) -> Result<SharedE2eKeys, StoreError> {
    Ok(replicix_chunkfmt::unlock::<ConstellationDomain>(block, passphrase)?)
}
```

(The aliases name the generic type; they are not re-exports. If aliases are not wanted, write
`E2eKeys<ConstellationDomain>` and `unlock::<ConstellationDomain>` at the call sites: the same
mechanical rewrite.) `TreeSealing` stays unchanged (it calls `keys.dek("mtree/...")`). The golden
tests prove byte equality with exactly these two labels
(`crates/chunkfmt/tests/fixtures/e2e/kdf_domain.bin`, `pin_context.txt`).

### 2.2 Errors

`replicix_chunkfmt::Error` and `replicix_objcas::Error` replace `StoreError` as the error of the
moved functions. `StoreError` stays and gains these conversions (variant for variant, so `?`
keeps working and messages are unchanged):

| from | variant | `StoreError` |
|---|---|---|
| chunkfmt `Error` | `CorruptObject(s)` | `CorruptObject(s)` |
| | `UnknownCodec(id)` | `UnknownCodec(id)` |
| | `Compression(s)` | `Compression(s)` |
| | `Keyring(s)` | `Meta(s)` |
| | `Io(e)` | `Io(e)` |
| objcas `Error` | `ObjectStore(e)` | `ObjectStore(e)` |
| | `CasConflict` | `CasConflict` |
| | `Meta(s)` | `Meta(s)` |
| | `Json(e)` | `Json(e)` |
| | `Io(e)` | `Io(e)` |
| | `AwsCredentials(s)` | `AwsCredentials(s)` |

A call site that matches the *result of a moved function* against `StoreError::...` directly
(instead of propagating with `?`) must convert first or match the replicix variant. Find them
with `rg 'Err\(StoreError::(CasConflict|UnknownCodec|CorruptObject|Compression|Meta|AwsCredentials)'`
(e.g. `lease.rs` tests, `commits.rs`'s `CorruptObject` assertion on a decode).

**Classifier.** `classify` used to know `StoreError`; replicix-objcas cannot. Register
Constellation's table once per process (the arms of the removed block of `classify_typed`:
`CasConflict | Conflict` and `AwsCredentials` transient; `CorruptObject | UnknownCodec |
HashMismatch | AlreadyExists | InboxVersion | NotFound | Meta | Compression | Json | Parallel`
permanent; `ObjectStore | Io` defer to the wrapped link):

```rust
replicix_objcas::register_error_classifier(classify_store_error); // fn(&(dyn Error + 'static)) -> Option<ErrorClass>
```

Call it from `cli::main`, engine start-up and the test helpers that build engines (any process
that calls `replicix_objcas::classify`). The `classify.rs` unit-test rows built on `StoreError`
become a Constellation test of `classify_store_error`.

### 2.3 Preflight / probe

```rust
impl ChunkStore {
    pub async fn preflight(&self) -> Vec<PreflightCheck> {
        replicix_objcas::preflight::preflight(self.store.as_ref()).await
    }
    pub async fn probe_conditional_writes(&self) -> Result<Capabilities, StoreError> {
        Ok(replicix_objcas::preflight::probe_conditional_writes(self.store.as_ref()).await?)
    }
}
```

`PreflightCheck` and `Capabilities` are the replicix types (same fields; `PreflightCheck::new` is
public). The scratch keys (`.preflight/<uuid>`, `.doctor/<uuid>`) are unchanged.

### 2.4 csi

`crates/csi/src/params.rs` shrinks to Constellation's constant and the conversion that needs the
control protocol:

```rust
pub const DEFAULT_PREFIX: &str = "constellation-csi";   // data location: must not change
pub fn fs_create_params(class: &ClassParams, prefix: String) -> FsCreateParams {
    let s = class.fs_create_spec(prefix);
    FsCreateParams { name: s.name, bucket: s.bucket, prefix: s.prefix, endpoint: s.endpoint,
        region: s.region, chunk_size: s.chunk_size, compression: s.compression,
        e2e: s.e2e, write_mode: s.write_mode }
}
```

- `ClassParams::parse(&p)` -> `ClassParams::parse(&p, DEFAULT_PREFIX)` (about 13 sites:
  `controller.rs`, `engine_pods.rs`, `engine_pods/controller_pods.rs`, `node/mod.rs`, and tests in
  `node/tests.rs`, `controller/tests.rs`). The one-argument `parse` is gone by design: a silently
  different default prefix would relocate every filesystem; the compiler finds every site.
- `class.fs_create(prefix)` -> `fs_create_params(&class, prefix)` (`controller.rs`, two sites).
- `crates/csi/src/credentials.rs` keeps `unlock_params`, `is_awaiting_unlock`, `rotation_attribution`,
  `retry_push`, `follow_rotations` and the tests of those; the rest (`Secrets`, the key-name
  constants, `fingerprint`, `SecretEvent`, `SecretSource`, `KubeSecrets`, `Latest`, `Refresher`)
  is imported from `replicix_csi_common::credentials`. Tests using `crate::credentials::fake::FakeSecrets`
  use `replicix_csi_common::credentials::fake::FakeSecrets` (dev-dependency with `test-support`).
- `SecretRef`, `CredentialMode`, `Layout`, `ClassParams`, `volume_context`, `SHARD_KEY`, `MAX_SHARDS`,
  `parse_duration`, `parse_quantity`, `PVC_*_KEY`, `CREDENTIAL_SECRET_*` are imported from
  `replicix_csi_common::params`.
- `crates/csi/tests/identity.rs`: `constellation_csi::proto::...` -> `replicix_csi_common::proto::...`.
- Volume-id shapes (`v1/pool/...`, `v1/dedicated/...`, the unversioned static handle) are inside
  `VolumeId` and unchanged; nothing to keep. A Lunix-style scheme (`lunix1:<uuid>`) uses
  `volume_id::IdPrefix`.

### 2.5 Clock (objcas)

Lease stamps read `replicix_objcas::clock`. Constellation's protocol clock (with its test fault
offsets and steps) is injected once:

```rust
struct ProtocolWallClock;
impl replicix_objcas::Clock for ProtocolWallClock {
    fn now_unix_ms(&self) -> i64 { constellation_platform::clock::protocol().wall().0 }
}
replicix_objcas::clock::set_clock(std::sync::Arc::new(ProtocolWallClock));
```

Install it before the first `Lease::granted`/`renewed`/`now_unix_ms`: in `cli::main`, in engine
start-up, and in the authority simulation and engine test setup (a test that moves the protocol
clock must have installed the adapter in its process, or the fault offset is silently ignored). The
sites that call `constellation_store_s3::lease::now_unix_ms` (engine `gc`, `snapsched`, `shipper`,
`holds`, `leave`, `mtree_gc`, `control/snapsched*`) only change path.

### 2.6 Legacy environment names

The moved code reads `REPLICIX_*`. Constellation's documented `CONSTELLATION_LEASE_TTL_MS`,
`CONSTELLATION_CAS_BUSY_RETRIES`, `CONSTELLATION_S3_MAX_RETRIES`,
`CONSTELLATION_S3_RETRY_TIMEOUT_MS` and `CONSTELLATION_MAX_DECOMPRESSED_BYTES` (configuration
reference, harness scenarios, engine tests) keep working by registering the prefix, once, first
thing in every binary and in every test process that sets one of them in-process:

```rust
replicix_objcas::set_legacy_env_prefix("CONSTELLATION");
replicix_chunkfmt::set_legacy_env_prefix("CONSTELLATION");
```

`REPLICIX_*` wins if both are set. `codec::MAX_DECOMPRESSED_ENV` is now
`"REPLICIX_MAX_DECOMPRESSED_BYTES"` (the fuzz targets `store_object` and `store_inbox_pack` set it
and keep working apart from the path). The decompression ceiling is read once, so register before
the first decode.

## 3. Signature and type differences callers must adapt to

Everything not listed is identical (names, signatures, behaviour, bytes).

**replicix-chunkfmt**
1. Errors are `replicix_chunkfmt::Error`, not `StoreError` (2.2): `codec::*`, `format::*`,
   `e2e::{create_keyring_block, unlock, rewrap_master, encrypt_object, decrypt_object}`,
   `CompressionSetting::{zstd, from_str}`, `Codec::from_id`.
2. `E2eKeys`, `SharedE2eKeys` and `unlock` are generic over `KeyDomain` (2.1);
   `create_keyring_block`, `rewrap_master`, `encrypt_object`, `decrypt_object`, `Argon2Params`,
   `KeyringBlock` are not. `TreeSealing` did not move.
3. Message text: the two "unsupported format version N; upgrade constellation" `CorruptObject`
   messages say "upgrade this software"; the decompression-ceiling error names
   `REPLICIX_MAX_DECOMPRESSED_BYTES` (plus the `CONSTELLATION_` name once the prefix is registered).
   Check `rg 'upgrade constellation'` on the migration base for assertions.
4. `Codec` gains `Lz4` only under the `lz4` feature, which Constellation does not enable: no
   exhaustive `match` changes.
5. `ChunkHash`, `ChunkSlice`, `ChunkLayout`: same definitions (serde/postcard encoding, `Debug`,
   `Display`, hex). Path is `replicix_chunkfmt::{ChunkHash, ...}`.

**replicix-objcas**
1. Errors are `replicix_objcas::Error` (2.2): `aws_auth::*`, `control::get_json`,
   `lease::{LeaseStore::*, fence_retired, live_leases_held_by}`, `probe::probe_cas_semantics`,
   `preflight::*`. `cas::*` still returns `object_store::Error`.
2. `classify` needs the registration of 2.2; `now_unix_ms` and lease stamps need the clock of 2.5;
   the environment names need 2.6.
3. `preflight` and `probe_conditional_writes` are free functions over `&dyn ObjectStore` (2.3).
4. `layout::lease(p)` is `lease::lease_path(p)`.
5. The test doubles (`faulty`, `scripted_http`) are not exported.

**replicix-platform-lite**
1. `UnixFileLock` (crate-private) is `FlockLock` (public fields); `LinuxFs` is `NativeFs` (Linux
   only; macOS keeps `MacFs`).
2. `forbid_core_dumps` and `core_dumps_forbidden` come from `replicix_platform_lite` instead of
   `constellation_platform::secrets`.

**replicix-csi-common**
1. `ClassParams::parse` takes `default_prefix`; `DEFAULT_PREFIX` is gone (2.4).
2. `ClassParams::fs_create` is `fs_create_spec`, returning `FsCreateSpec` (2.4).
3. `credentials` is the control-protocol-free part only (2.4); `Refresher::with_backoff` and
   `credentials::fake` are public (the latter behind `test-support`).

**replicix-upload-concurrency**: none (rename).

## 4. Call sites to rewrite

Mechanical renames, grouped by moved crate. The appendix lists every `use` statement at `15ea0c51`
(regenerate on the migration base with `python3 tools/migration_sites.py <constellation checkout>`);
a statement that imports moved and kept names is split. Inline (non-`use`) paths are found with:

```sh
rg 'constellation_store_s3::(lease|cas|classify|codec|format|e2e|aws_auth|probe|control)::'
rg 'constellation_store_s3::(Lease|ErrorClass|classify|Codec|CompressionSetting|KeyringBlock|E2eKeys|SharedE2eKeys|unlock|create_keyring_block|Argon2Params)\b'
rg 'constellation_fs_core::(chunk::)?(ChunkHash|ChunkLayout|ChunkSlice)|constellation_upload_concurrency'
rg 'constellation_platform::(lock|fs|forbid_core_dumps|core_dumps_forbidden|FileLock|FsPrimitives|LockGuard)'
rg 'constellation_csi::(volume_id|params|credentials|proto)|crate::(volume_id|proto)::'
```

| old | new |
|---|---|
| `constellation_store_s3::{cas,classify,probe,aws_auth,lease,control}` (module or root item) | `replicix_objcas::{cas,classify,probe,aws_auth,lease,control}` / root items of `replicix_objcas` |
| `constellation_store_s3::{codec,format}` | `replicix_chunkfmt::{codec,format}` |
| `constellation_store_s3::e2e::{...}` except `TreeSealing` | `replicix_chunkfmt::e2e::{...}` (generics: 2.1) |
| `constellation_store_s3::{Capabilities, PreflightCheck}` | `replicix_objcas::{Capabilities, PreflightCheck}` |
| `constellation_fs_core::{ChunkHash, ChunkLayout, ChunkSlice}` | `replicix_chunkfmt::{...}` |
| `constellation_upload_concurrency` | `replicix_upload_concurrency` |
| `constellation_platform::{FileLock, LockGuard, FsPrimitives, forbid_core_dumps, core_dumps_forbidden}`, `::lock::open_lock_file` | `replicix_platform_lite::{...}` |
| `constellation_csi::{volume_id, params (moved items), credentials (moved items), proto}` and `crate::...` | `replicix_csi_common::{...}` |

`crate::x` inside `store-s3` follows the same table.

## 5. Gates the migration must pass

On the migration branch, before it merges:

1. **Builds:** debug and release (`cargo build --workspace`, and `cargo build --release` of the CI
   release set: `constellation`, `harness`, `chaos`); `cargo clippy --workspace --all-targets -- -D warnings`;
   `cargo fmt --all --check`; the cross-check (`make check-cross`), because `platform` changed;
   `cargo build -p constellation-csi`.
2. **Tests:** `cargo test --workspace` (the moved unit tests now run in replicix-core; the
   Constellation suites that remain must pass, including the `authority` simulation and the
   `store-s3` fault tests), doctests, and every `fuzz/` target that imports a moved path compiles.
3. **Harness smoke:** the containerized smoke suite and the harness's smoke scenarios, plus the
   CSI unit job.
4. **No performance regression on ext4, xfs, btrfs and tmpfs:** the same benchmark set before and
   after, same host and filesystems, numbers compared in the PR. The moved code is on the hot path of
   chunk encode/decode and hashing, lease and CAS calls, and the lock/fs primitives; the changes that
   could cost anything are the generic `E2eKeys<D>` (compile-time only), the `Clock` indirection (one
   `RwLock` read per lease stamp) and the classifier dispatch. The comparison must include at least
   small-file create/stat, large sequential write and read through the codecs, and a lease renew loop.
5. **Behaviour pins:** create a filesystem (E2E on) with the old binary and mount it with the new
   one, and the other way round (derived keys); a `meta.json` and a lease object written by one binary
   are read by the other; the `CONSTELLATION_*` variables are still honoured (2.6).
6. No version bumps in Constellation's crates; the `Cargo.lock` diff is limited to the added
   `replicix-*` packages and the removed `constellation-upload-concurrency`.

## Appendix: `use` statements to rewrite at `15ea0c51`

`path:line: statement`, grouped by moved crate and by Constellation crate; over-inclusive where one
statement imports moved and kept names.

```
## upload-concurrency -> replicix-upload-concurrency
# bench: 1 statements in 1 files
bench/uploadbench/src/run.rs:12: use constellation_upload_concurrency::ConcurrencyGate;
# engine: 2 statements in 2 files
crates/engine/src/prefetch.rs:7: use constellation_upload_concurrency::{AdaptiveConcurrency, ConcurrencyGate};
crates/engine/src/upload.rs:29: use constellation_upload_concurrency::{AdaptiveConcurrency, ConcurrencyGate, ConcurrencyPermit};
## fs-core chunk -> replicix-chunkfmt
# authority: 8 statements in 3 files
crates/authority/src/core/tests.rs:3879: use constellation_fs_core::{ChunkHash, Manifest};
crates/authority/src/core/tests.rs:3970: use constellation_fs_core::{ChunkHash, Manifest};
crates/authority/src/core/tests.rs:5010: use constellation_fs_core::{ChunkHash, Manifest};
crates/authority/src/core/tests.rs:5532: use constellation_fs_core::{ChunkHash, Manifest};
crates/authority/src/core/tests.rs:7490: use constellation_fs_core::{ChunkHash, Manifest};
crates/authority/src/core/tests.rs:56679: use constellation_fs_core::{ChunkHash, ChunkInfo, ChunkLayout, Ino, Manifest};
crates/authority/tests/sim/chunks.rs:38: use constellation_fs_core::{ChunkHash, ChunkInfo, ChunkLayout, Ino, Manifest};
crates/authority/tests/sim/run.rs:15: use constellation_fs_core::ChunkHash;
# engine: 32 statements in 28 files
crates/engine/src/fsck.rs:10: use constellation_fs_core::ChunkHash;
crates/engine/src/holds.rs:63: use constellation_fs_core::{ChunkHash, Ino};
crates/engine/src/holds.rs:498: use constellation_fs_core::ChunkLayout;
crates/engine/src/existence.rs:26: use constellation_fs_core::ChunkHash;
crates/engine/src/snapshot_batch.rs:2735: use constellation_fs_core::ChunkHash;
crates/engine/src/snapwalk.rs:96: use constellation_fs_core::{ChunkHash, Ino};
crates/engine/src/shipper.rs:526: use constellation_fs_core::{ChunkHash, ChunkLayout};
crates/engine/src/snapshot.rs:21: use constellation_fs_core::{ChunkHash, Ino, InodeKind};
crates/engine/src/fault.rs:6: use constellation_fs_core::ChunkHash;
crates/engine/src/prefetch.rs:5: use constellation_fs_core::{ChunkHash, Ino};
crates/engine/src/scan.rs:8: use constellation_fs_core::{ChunkHash, Ino, InodeKind};
crates/engine/src/scan.rs:266: use constellation_fs_core::{ChunkHash, DEFAULT_CHUNK_SIZE, INLINE_CHUNKS_MAX};
crates/engine/src/pin.rs:21: use constellation_fs_core::{ChunkHash, Ino};
crates/engine/src/snapsched.rs:3878: use constellation_fs_core::ChunkHash;
crates/engine/src/upload.rs:1201: use constellation_fs_core::ChunkHash;
crates/engine/src/sync.rs:13: use constellation_fs_core::{ChunkHash, Ino};
crates/engine/src/gc.rs:13: use constellation_fs_core::ChunkHash;
crates/engine/src/gc.rs:1126: use constellation_fs_core::ChunkLayout;
crates/engine/src/gc.rs:1573: use constellation_fs_core::{ChunkHash, ChunkLayout};
crates/engine/src/staging.rs:983: use constellation_fs_core::ChunkLayout;
crates/engine/src/coop.rs:40: use constellation_fs_core::ChunkHash;
crates/engine/src/snapacct/service_tests.rs:10: use constellation_fs_core::{ChunkHash, Ino, InodeKind};
crates/engine/src/snapacct/ops.rs:17: use constellation_fs_core::ChunkHash;
crates/engine/src/snapacct/mod.rs:142: use constellation_fs_core::ChunkHash;
crates/engine/src/snapacct/verify.rs:27: use constellation_fs_core::{ChunkHash, InodeKind};
crates/engine/src/snapacct/encoding.rs:25: use constellation_fs_core::ChunkHash;
crates/engine/src/snapacct/service.rs:147: use constellation_fs_core::{ChunkHash, Ino};
crates/engine/src/view/mod.rs:196: use constellation_fs_core::{ChunkHash, FileAttr, Ino, InodeKind, INLINE_CHUNKS_MAX};
crates/engine/src/view/fsync_drain_tests.rs:11: use constellation_fs_core::ChunkHash;
crates/engine/src/coop/exact.rs:688: use constellation_fs_core::ChunkHash;
crates/engine/src/coop/fresh.rs:23: use constellation_fs_core::ChunkHash;
crates/engine/src/control/ops.rs:771: use constellation_fs_core::ChunkHash;
# meta: 8 statements in 7 files
crates/meta/src/store/bootstrap.rs:26: use constellation_fs_core::{ChunkHash, Ino};
crates/meta/src/store/mod.rs:2485: use constellation_fs_core::{ChunkHash, ChunkInfo, Manifest};
crates/meta/src/store/remote.rs:52: use constellation_fs_core::{ChunkHash, ChunkInfo, Ino, Manifest};
crates/meta/src/store/held.rs:141: use constellation_fs_core::{ChunkHash, ChunkInfo, Ino, Manifest};
crates/meta/src/store/misc.rs:6: use constellation_fs_core::{ChunkHash, Ino};
crates/meta/src/store/misc.rs:867: use constellation_fs_core::ChunkHash;
crates/meta/tests/holder_capture.rs:1566: use constellation_fs_core::{ChunkHash, Manifest};
crates/meta/tests/held.rs:13: use constellation_fs_core::{ChunkHash, ChunkInfo, ChunkLayout, Ino, Manifest};
# store-s3: 4 statements in 4 files
crates/store-s3/src/gc.rs:12: use constellation_fs_core::ChunkHash;
crates/store-s3/src/node_cache.rs:72: use constellation_fs_core::ChunkHash;
crates/store-s3/src/layout.rs:3: use constellation_fs_core::ChunkHash;
crates/store-s3/src/store.rs:10: use constellation_fs_core::{ChunkHash, DEFAULT_CHUNK_SIZE};
## store-s3 -> replicix-objcas / replicix-chunkfmt
# authority: 19 statements in 10 files
crates/authority/src/action.rs:25: use constellation_store_s3::{Lease, LeaseTag};
crates/authority/src/event.rs:20: use constellation_store_s3::{Lease, LeaseTag};
crates/authority/src/core/backup.rs:368: use constellation_store_s3::{AckPolicy, Lease, LeaseTag};
crates/authority/src/core/tests.rs:12: use constellation_store_s3::{Lease, LeaseMode, LeaseStore};
crates/authority/src/core/tests.rs:5533: use constellation_store_s3::AckPolicy;
crates/authority/src/core/tests.rs:10155: use constellation_store_s3::AckPolicy;
crates/authority/src/core/tests.rs:14197: use constellation_store_s3::AckPolicy;
crates/authority/src/core/tests.rs:41412: use constellation_store_s3::AckPolicy;
crates/authority/src/core/tests.rs:47674: use constellation_store_s3::AckPolicy;
crates/authority/src/core/tests.rs:50750: use constellation_store_s3::{AckPolicy, InheritedTail};
crates/authority/src/core/seal_tail.rs:49: use constellation_store_s3::{AckPolicy, InheritedTail, Lease, LeaseTag};
crates/authority/src/core/lease.rs:12: use constellation_store_s3::lease::LEASE_VERSION;
crates/authority/src/core/lease.rs:13: use constellation_store_s3::{AckPolicy, Lease, LeaseTag, RenewalKey};
crates/authority/src/core/promise.rs:67: use constellation_store_s3::RenewalKey;
crates/authority/src/core/promise.rs:68: use constellation_store_s3::{AckPolicy, Lease, LeaseTag};
crates/authority/src/core/jobs.rs:24: use constellation_store_s3::{AckPolicy, Lease, LeaseTag};
crates/authority/tests/sim/oracle.rs:30: use constellation_store_s3::{AckPolicy, Lease, LogStore};
crates/authority/tests/sim/oracle.rs:2510: use constellation_store_s3::{LeaseMode, LeaseStore};
crates/authority/tests/sim/node.rs:26: use constellation_store_s3::{LeaseMode, LeaseStore, LogStore, StoreError};
# cli: 1 statements in 1 files
crates/cli/src/main.rs:29: use constellation_store_s3::{ChunkStore, CompressionSetting, FsMeta};
# engine: 42 statements in 26 files
crates/engine/src/fsck.rs:12: use constellation_store_s3::{CompressionSetting, GcJournalEntry, LeaseMode, LeaseStore, LogStore};
crates/engine/src/e2e_pin.rs:37: use constellation_store_s3::{E2eKeys, FsMeta};
crates/engine/src/e2e_pin.rs:421: use constellation_store_s3::e2e::rewrap_master;
crates/engine/src/e2e_pin.rs:422: use constellation_store_s3::{create_keyring_block, unlock, SharedE2eKeys};
crates/engine/src/node.rs:35: use constellation_store_s3::{ChunkStore, CompressionSetting, FsMeta};
crates/engine/src/singleton.rs:26: use constellation_store_s3::{Lease, LeaseMode, LeaseStore, LeaseTag, StoreError};
crates/engine/src/lease.rs:31: pub use constellation_store_s3::lease::lease_ttl_ms;
crates/engine/src/lease.rs:368: use constellation_store_s3::lease::now_unix_ms;
crates/engine/src/lease.rs:369: use constellation_store_s3::{Lease, LeaseMode, LeaseStore};
crates/engine/src/snapshot_batch.rs:1283: use constellation_store_s3::{ BlobStore, ChunkStore, CommitChain, LeaseMode, LeaseStore, NodeCache, PackStore, };
crates/engine/src/prune.rs:19: use constellation_store_s3::LeaseMode;
crates/engine/src/snapwalk.rs:1096: use constellation_store_s3::CompressionSetting;
crates/engine/src/authority_driver.rs:44: use constellation_store_s3::{ ChunkStore, CompressionSetting, LeaseMode, LeaseStore, LogStore, StoreError, };
crates/engine/src/authority_driver.rs:9122: use constellation_store_s3::LeaseMode;
crates/engine/src/authority_driver.rs:9856: use constellation_store_s3::{ChunkStore, CompressionSetting};
crates/engine/src/shipper.rs:225: use constellation_store_s3::LeaseMode;
crates/engine/src/shipper.rs:527: use constellation_store_s3::{ChunkStore, CompressionSetting};
crates/engine/src/shipper.rs:750: use constellation_store_s3::LeaseMode;
crates/engine/src/doctor.rs:6: use constellation_store_s3::{CasProbeReport, Versioning};
crates/engine/src/lifecycle.rs:1080: use constellation_store_s3::{LeaseMode, LeaseStore};
crates/engine/src/snapsched.rs:135: use constellation_store_s3::{LeaseMode, StoreError};
crates/engine/src/upload.rs:14: use constellation_store_s3::{ChunkStore, CompressionSetting};
crates/engine/src/sync.rs:51: pub use constellation_store_s3::ErrorClass;
crates/engine/src/gc.rs:15: use constellation_store_s3::{ append_journal, publish_condemned, GcJournalEntry, LeaseMode, LogStore, SnapshotRecord, SnapshotStore, StoreError, };
crates/engine/src/gc.rs:1152: use constellation_store_s3::{read_condemned, CompressionSetting};
crates/engine/src/gc.rs:1208: use constellation_store_s3::CompressionSetting;
crates/engine/src/gc.rs:1447: use constellation_store_s3::CompressionSetting;
crates/engine/src/gc.rs:1575: use constellation_store_s3::{ChunkStore, CompressionSetting, LogStore};
crates/engine/src/gc.rs:1697: use constellation_store_s3::CompressionSetting;
crates/engine/src/gc.rs:1863: use constellation_store_s3::CompressionSetting;
crates/engine/src/gc.rs:2229: use constellation_store_s3::CompressionSetting;
crates/engine/src/leave.rs:177: use constellation_store_s3::{ claim_node_id, get_node, write_eligible_roster, Lease, LeaseMode, LeaseStore, };
crates/engine/src/coop.rs:1609: use constellation_store_s3::CompressionSetting;
crates/engine/src/snapacct/service_tests.rs:12: use constellation_store_s3::{ChunkStore, CommitChain, CompressionSetting};
crates/engine/src/view/mod.rs:198: use constellation_store_s3::{ChunkStore, CompressionSetting, DecodePriority};
crates/engine/src/view/cookie.rs:27: use constellation_store_s3::{FsMeta, SharedE2eKeys};
crates/engine/src/view/durable_ack_tests.rs:12: use constellation_store_s3::lease::now_unix_ms;
crates/engine/src/view/durable_ack_tests.rs:13: use constellation_store_s3::{Lease, LeaseMode, LeaseStore};
crates/engine/src/view/memcache_tests.rs:15: use constellation_store_s3::{create_keyring_block, unlock};
crates/engine/src/manager/tests.rs:10: use constellation_store_s3::{ChunkStore, FsMeta, PreflightCheck};
crates/engine/src/manager/doctor.rs:6: use constellation_store_s3::{Capabilities, CasProbeReport, MissingFs};
crates/engine/src/manager/create.rs:8: use constellation_store_s3::{CompressionSetting, FsMeta, PreflightCheck, StoreError};
# store-s3: 9 statements in 7 files
crates/store-s3/src/probe.rs:35: use crate::cas::{classify, http_status, CasCode};
crates/store-s3/src/inbox.rs:97: use crate::e2e::{decrypt_object, encrypt_object, SharedE2eKeys};
crates/store-s3/src/log.rs:18: use crate::e2e::{decrypt_object, encrypt_object, SharedE2eKeys};
crates/store-s3/src/snapsched.rs:26: use crate::lease::LeaseMode;
crates/store-s3/src/heartbeat.rs:39: use crate::lease::RenewalKey;
crates/store-s3/src/store.rs:4: use crate::codec::CompressionSetting;
crates/store-s3/src/store.rs:6: use crate::e2e::{decrypt_object, encrypt_object, KeyringBlock, SharedE2eKeys};
crates/store-s3/src/store.rs:8: use crate::{format, layout};
crates/store-s3/src/format.rs:13: use crate::codec::{self, Codec, CompressionSetting};
## platform -> replicix-platform-lite
# engine: 1 statements in 1 files
crates/engine/src/lease.rs:27: use constellation_platform::clock::protocol;
# platform: 11 statements in 5 files
crates/platform/src/macos.rs:26: use crate::fs::FsPrimitives;
crates/platform/src/macos.rs:28: use crate::lock::FileLock;
crates/platform/src/macos.rs:33: use crate::{unsupported, HostServices};
crates/platform/src/secrets.rs:32: use crate::lock::FileLock;
crates/platform/src/unsupported.rs:17: use crate::fs::FsPrimitives;
crates/platform/src/unsupported.rs:19: use crate::lock::{FileLock, LockGuard};
crates/platform/src/unsupported.rs:23: use crate::{unsupported, HostServices};
crates/platform/src/unix.rs:16: use crate::lock::{FileLock, LockGuard};
crates/platform/src/linux.rs:21: use crate::fs::FsPrimitives;
crates/platform/src/linux.rs:23: use crate::lock::FileLock;
crates/platform/src/linux.rs:794: use crate::lock::open_lock_file;
## csi -> replicix-csi-common
# csi: 49 statements in 13 files
crates/csi/src/identity.rs:14: use crate::proto::csi::v1::identity_server::Identity as IdentityRpc;
crates/csi/src/identity.rs:15: use crate::proto::csi::v1::plugin_capability::{ service::Type as ServiceCapability, Service, Type as CapabilityType, };
crates/csi/src/identity.rs:18: use crate::proto::csi::v1::{ GetPluginCapabilitiesRequest, GetPluginCapabilitiesResponse, GetPluginInfoRequest, GetPluginInfoResponse, PluginCapability, ProbeRequest, ProbeResponse, };
crates/csi/src/purge.rs:102: use crate::volume_id::{TRASH_DIR, VOLUMES_DIR};
crates/csi/src/credentials.rs:38: use crate::params::SecretRef;
crates/csi/src/main.rs:8: use constellation_csi::credentials::{KubeSecrets, Refresher};
crates/csi/src/main.rs:18: use constellation_csi::proto::csi::v1::controller_server::ControllerServer;
crates/csi/src/main.rs:19: use constellation_csi::proto::csi::v1::identity_server::IdentityServer;
crates/csi/src/main.rs:20: use constellation_csi::proto::csi::v1::node_server::NodeServer;
crates/csi/src/main.rs:340: use constellation_csi::proto::csi::v1::node_client::NodeClient;
crates/csi/src/main.rs:341: use constellation_csi::proto::csi::v1::{NodeGetVolumeHealthRequest, VolumeHealthErrorType};
crates/csi/src/controller.rs:95: use crate::params::{ClassParams, Layout, PVC_NAMESPACE_KEY, PVC_NAME_KEY};
crates/csi/src/controller.rs:96: use crate::proto::csi::v1::controller_server::Controller as ControllerRpc;
crates/csi/src/controller.rs:97: use crate::proto::csi::v1::controller_service_capability::rpc::Type as RpcType;
crates/csi/src/controller.rs:98: use crate::proto::csi::v1::controller_service_capability::{Rpc, Type as CapabilityType};
crates/csi/src/controller.rs:99: use crate::proto::csi::v1::volume_capability::access_mode::Mode;
crates/csi/src/controller.rs:100: use crate::proto::csi::v1::volume_capability::AccessType;
crates/csi/src/controller.rs:101: use crate::proto::csi::v1::*;
crates/csi/src/controller.rs:102: use crate::volume_id::{validate_name, VolumeId, TRASH_DIR, VOLUMES_DIR};
crates/csi/src/control_client.rs:15: use crate::params::ClassParams;
crates/csi/src/engine_pods.rs:119: use crate::credentials::{fingerprint, is_awaiting_unlock, unlock_params, Refresher, Secrets};
crates/csi/src/engine_pods.rs:121: use crate::params::{ClassParams, CredentialMode, SecretRef};
crates/csi/src/engine_pods.rs:122: use crate::volume_id::VolumeId;
crates/csi/src/engine_pods.rs:3390: use crate::params::ClassParams;
crates/csi/src/controller/snapshots.rs:90: use crate::credentials::Secrets;
crates/csi/src/controller/snapshots.rs:91: use crate::params::{ClassParams, Layout};
crates/csi/src/controller/snapshots.rs:92: use crate::proto::csi::v1::volume_content_source::Type as SourceType;
crates/csi/src/controller/snapshots.rs:93: use crate::proto::csi::v1::*;
crates/csi/src/controller/snapshots.rs:94: use crate::volume_id::{validate_snapshot_name, SnapshotId, VolumeId, VOLUMES_DIR};
crates/csi/src/controller/tests.rs:3: use crate::proto::csi::v1::volume_capability::{AccessMode, BlockVolume, MountVolume};
crates/csi/src/node/mod.rs:84: use crate::credentials::{is_awaiting_unlock, unlock_params, Refresher, Secrets};
crates/csi/src/node/mod.rs:86: use crate::params::{ClassParams, CredentialMode, SecretRef, SHARD_KEY};
crates/csi/src/node/mod.rs:87: use crate::proto::csi::v1::node_server::Node as NodeRpc;
crates/csi/src/node/mod.rs:88: use crate::proto::csi::v1::node_service_capability::rpc::Type as RpcType;
crates/csi/src/node/mod.rs:89: use crate::proto::csi::v1::node_service_capability::{Rpc, Type as CapabilityType};
crates/csi/src/node/mod.rs:90: use crate::proto::csi::v1::volume_capability::access_mode::Mode;
crates/csi/src/node/mod.rs:91: use crate::proto::csi::v1::volume_capability::AccessType;
crates/csi/src/node/mod.rs:92: use crate::proto::csi::v1::volume_health::VolumeHealthEntry;
crates/csi/src/node/mod.rs:93: use crate::proto::csi::v1::volume_usage::Unit;
crates/csi/src/node/mod.rs:94: use crate::proto::csi::v1::*;
crates/csi/src/node/mod.rs:95: use crate::volume_id::VolumeId;
crates/csi/src/node/tests.rs:8: use crate::proto::csi::v1::volume_capability::{AccessMode, MountVolume};
crates/csi/src/node/tests.rs:897: use crate::credentials::fake::FakeSecrets;
crates/csi/src/node/tests.rs:1321: use crate::credentials::fake::FakeSecrets;
crates/csi/src/node/engines.rs:13: use crate::credentials::{fingerprint, unlock_params, Secrets};
crates/csi/tests/identity.rs:10: use constellation_csi::proto::csi::v1::identity_client::IdentityClient;
crates/csi/tests/identity.rs:11: use constellation_csi::proto::csi::v1::identity_server::IdentityServer;
crates/csi/tests/identity.rs:12: use constellation_csi::proto::csi::v1::node_server::NodeServer;
crates/csi/tests/identity.rs:13: use constellation_csi::proto::csi::v1::{GetPluginInfoRequest, ProbeRequest};
```

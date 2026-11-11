# Provenance: replicix-objcas

Extracted from the Constellation repository (https://github.com/replicix/constellation) at commit
`15ea0c51` (the exact commit the files were copied from).
Synced since: `lease.rs` at `fc5d8be6` (ADR-51: `InheritedTail.kept`,
`InheritedTail.holder`, `Lease::retired_tenure`). Diff the sources below
from the newest synced commit to find later changes.

## Source files

- `crates/store-s3/src/cas.rs` -> `src/cas.rs`
- `crates/store-s3/src/classify.rs` -> `src/classify.rs`
- `crates/store-s3/src/probe.rs` -> `src/probe.rs`
- `crates/store-s3/src/aws_auth.rs` -> `src/aws_auth.rs`
- `crates/store-s3/src/lease.rs` -> `src/lease.rs` (whole file)
- `crates/store-s3/src/control.rs` -> `src/control.rs` (torn-read retrying JSON GET; needed by `cas` and `lease`)
- `crates/store-s3/src/store.rs`: `ChunkStore::preflight`, `ChunkStore::probe_conditional_writes`, `PreflightCheck`, `Capabilities`, `concise_os_error`, `is_missing_bucket` -> `src/preflight.rs` (methods became free functions over `&dyn ObjectStore`)
- `crates/store-s3/src/error.rs`: the variants these modules use -> `src/error.rs`
- test support `crates/store-s3/src/faulty.rs`, `scripted_http.rs` -> `src/` (`#[cfg(test)]`, crate-private)

## What changed

- `StoreError` -> `Error` with only `ObjectStore`, `CasConflict`, `Meta`, `Json`, `Io`, `AwsCredentials` (same messages).
- `classify`: the arms for the other `StoreError` variants are replaced by `register_error_classifier`.
- Wall time: `constellation_platform::clock::protocol().wall()` -> the `clock::Clock` trait (`SystemClock` default, `set_clock` to inject).
- Environment overrides renamed `REPLICIX_*`; a legacy prefix can be registered (`set_legacy_env_prefix`).
- `layout::lease` -> `lease::lease_path`.
- `aws_auth` test no longer builds a `ChunkStore`; it issues a plain GET.
- Not moved: `layout.rs` (bucket layout), heartbeat, designation, nodes, everything that needs `fs-core` or `mtree`.
- Kept as is: the 30 s retry cap, `RedactingConnector`, `object_store` pinned to the Constellation lockfile version (0.14.1).

## What changed



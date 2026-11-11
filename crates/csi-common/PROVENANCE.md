# Provenance: replicix-csi-common

Extracted from the Constellation repository (https://github.com/replicix/constellation) at commit
`15ea0c51` (the exact commit the files were copied from).
Constellation's later changes to these files are not included; diff the sources
below against this crate to find them.

## Source files

- `crates/csi/proto/{csi.proto,LICENSE,README.md,VERSION}` -> `proto/` (CSI spec v1.13.0, Apache-2.0, unmodified)
- `crates/csi/build.rs` -> `build.rs`, `crates/csi/src/proto.rs` -> `src/proto.rs`
- `crates/csi/src/volume_id.rs` -> `src/volume_id.rs`
- `crates/csi/src/params.rs` -> `src/params.rs`
- `crates/csi/src/credentials.rs` -> `src/credentials.rs` (the part with no control-protocol types)

## What changed

- `volume_id`: unchanged; added `IdPrefix`/`valid_token` for drivers with their own id scheme.
- `params`: `ClassParams::parse` takes the default bucket prefix as an argument and `DEFAULT_PREFIX` is gone; `fs_create` -> `fs_create_spec` returning the plain `FsCreateSpec` (no control-protocol type); a few helpers made public.
- `credentials`: `unlock_params`, `is_awaiting_unlock`, `rotation_attribution`, `retry_push`, `follow_rotations` stay with their driver; `KubeSecrets` is behind the `kube-secrets` feature (default); `Refresher::with_backoff` and the `fake` source are public (`test-support` for the latter).

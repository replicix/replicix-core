# Provenance: replicix-platform-lite

Extracted from the Constellation repository (https://github.com/replicix/constellation) at commit
`15ea0c51` (the exact commit the files were copied from).
Constellation's later changes to these files are not included; diff the sources
below against this crate to find them.

## Source files

- `crates/platform/src/lock.rs` -> `src/lock.rs` (trait, guard, `open_lock_file`)
- `crates/platform/src/fs.rs` -> `src/fs.rs` (`FsPrimitives`)
- `crates/platform/src/unix.rs` (`UnixFileLock`, the xattr calls) and `crates/platform/src/linux.rs` (`LinuxFs`, `fallocate`) -> `src/lock.rs` (`FlockLock`), `src/fs.rs` (`NativeFs`)
- `crates/platform/src/secrets.rs` (`forbid_core_dumps`, `core_dumps_forbidden` only) -> `src/coredump.rs`
- `crates/platform/src/lib.rs` (`unsupported`)

## What changed

- `UnixFileLock` (crate-private) -> public `FlockLock`; the "would block" test is `ErrorKind::WouldBlock` instead of Constellation's errno code type (same errno).
- `LinuxFs` (private) -> public `NativeFs`, Linux only.
- Tests added for the lock, `NativeFs` and the core-dump functions.
- Not moved: secret stores (keyring/zbus), dirs, process facts, daemonizing, lifecycle, mount table, clocks.

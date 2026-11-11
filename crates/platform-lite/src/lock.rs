// SPDX-License-Identifier: MPL-2.0
//! Advisory whole-file locks: `daemon.lock` (one daemon per state dir),
//! the takeover mutex beside it, and the writer locks of the registry and
//! the E2E pins.
//!
//! On Linux and macOS these are `flock(2)` locks: owned by the open file
//! description, released when its last descriptor closes — including by
//! the kernel at process exit, which is what makes a leaked
//! [`LockGuard`] a "held for the rest of this process's life" lock. A Windows port would map the
//! same shape onto `LockFileEx`.

use std::fs::File;
use std::io;
use std::path::Path;

/// A held lock: the locked file, released when this is dropped (the
/// descriptor closes). `std::mem::forget` keeps it for the process's
/// lifetime.
#[derive(Debug)]
pub struct LockGuard {
    file: File,
}

impl LockGuard {
    /// Only [`FileLock`] implementations make guards, after locking.
    pub fn new(file: File) -> LockGuard {
        LockGuard { file }
    }

    pub fn file(&self) -> &File {
        &self.file
    }
}

pub trait FileLock: Send + Sync {
    /// Take an exclusive lock on `file`, waiting for as long as another
    /// holder has it.
    fn lock(&self, file: File) -> io::Result<LockGuard>;

    /// Take an exclusive lock on `file` if nobody holds one: `Ok(None)`
    /// when another open file holds it (the OS said "would block",
    /// `EWOULDBLOCK`), an error for anything else.
    fn try_lock(&self, file: File) -> io::Result<Option<LockGuard>>;

    /// The pids that may hold the lock on `path`, when the host can tell
    /// (Linux: `/proc/locks`). Empty when nobody holds it, or the host
    /// cannot say. More than the holder may show: `/proc/locks` names a
    /// file only by device and inode number, which other files can share
    /// (btrfs numbers every subvolume's inodes from 257 under one
    /// device), so a pid here holds *a* lock under the same name — confirm
    /// it with [`FileLock::opened_by`] before acting on it.
    fn holder_pids(&self, path: &Path) -> Vec<u32>;

    /// Whether process `pid` has `path`'s very file (the same device and
    /// inode as `stat(path)` reports) open, as far as the host can tell:
    /// `false` when it does not, or when that cannot be seen (no `/proc`,
    /// another user's process).
    fn opened_by(&self, pid: u32, path: &Path) -> bool;
}

/// Open (creating, never truncating) the file a lock is taken on.
pub fn open_lock_file(path: &Path) -> io::Result<File> {
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
}

/// `flock(2)` locks (unix). `holders` answers [`FileLock::holder_pids`] and
/// `opened_by` [`FileLock::opened_by`], which only Linux can (from
/// `/proc`); [`FlockLock::new`] answers "unknown" to both.
#[cfg(unix)]
pub struct FlockLock {
    pub holders: fn(&Path) -> Vec<u32>,
    pub opened_by: fn(u32, &Path) -> bool,
}

#[cfg(unix)]
impl FlockLock {
    /// Locks with no way to name the holders of a lock.
    pub fn new() -> FlockLock {
        FlockLock {
            holders: |_| Vec::new(),
            opened_by: |_, _| false,
        }
    }
}

#[cfg(unix)]
impl Default for FlockLock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
fn flock(file: &File, op: libc::c_int) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: `file` owns a valid open fd for the duration of the call;
    // `flock` does not touch memory.
    if unsafe { libc::flock(file.as_raw_fd(), op) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
impl FileLock for FlockLock {
    fn lock(&self, file: File) -> io::Result<LockGuard> {
        loop {
            match flock(&file, libc::LOCK_EX) {
                Ok(()) => return Ok(LockGuard::new(file)),
                // A signal handler (SA_RESTART-less) interrupted the wait.
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn try_lock(&self, file: File) -> io::Result<Option<LockGuard>> {
        match flock(&file, libc::LOCK_EX | libc::LOCK_NB) {
            Ok(()) => Ok(Some(LockGuard::new(file))),
            // EAGAIN == EWOULDBLOCK: the OS said "would block".
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn holder_pids(&self, path: &Path) -> Vec<u32> {
        (self.holders)(path)
    }

    fn opened_by(&self, pid: u32, path: &Path) -> bool {
        (self.opened_by)(pid, path)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    fn open(path: &Path) -> File {
        open_lock_file(path).unwrap()
    }

    #[test]
    fn a_second_open_cannot_take_a_held_lock() {
        let locks = FlockLock::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        let held = locks.try_lock(open(&path)).unwrap().expect("free lock");
        // flock is per open file description, so this holds in-process too.
        assert!(locks.try_lock(open(&path)).unwrap().is_none());
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = {
            let path = path.clone();
            std::thread::spawn(move || {
                let guard = FlockLock::new().lock(open(&path)).unwrap();
                tx.send(()).unwrap();
                drop(guard);
            })
        };
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err());
        drop(held);
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("the blocking lock was granted once released");
        waiter.join().unwrap();
        assert!(locks.try_lock(open(&path)).unwrap().is_some());
    }

    #[test]
    fn a_leaked_guard_keeps_the_lock() {
        let locks = FlockLock::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        let guard = locks.try_lock(open(&path)).unwrap().unwrap();
        let fd = guard.file().as_raw_fd();
        std::mem::forget(guard);
        assert!(locks.try_lock(open(&path)).unwrap().is_none());
        // SAFETY: the leaked guard's fd, closed exactly once here.
        unsafe { libc::close(fd) };
        // A sibling test's child process can hold a copy of the descriptor
        // between its fork and exec; the lock is free once that exec ran.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if locks.try_lock(open(&path)).unwrap().is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "lock never freed");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn holders_default_to_unknown() {
        let locks = FlockLock::new();
        assert!(locks.holder_pids(Path::new("/nonexistent")).is_empty());
        assert!(!locks.opened_by(1, Path::new("/nonexistent")));
    }
}

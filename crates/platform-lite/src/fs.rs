// SPDX-License-Identifier: MPL-2.0
//! The filesystem capabilities that differ between hosts (plan 31 §2
//! point 2): hole punching, preallocation, fsync strength and page-cache
//! advice, and extended attributes (the same calls on Linux and macOS, with
//! different arities). Everything else the engine does to local files is
//! plain `std::fs`.

use std::fs::File;
use std::io;
use std::path::Path;

pub trait FsPrimitives: Send + Sync {
    /// Deallocate `len` bytes at `offset` without changing the file's
    /// length: reads there return zeros, and the blocks go back to the
    /// filesystem (Linux `FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE`,
    /// macOS `F_PUNCHHOLE`). A filesystem that cannot punch holes answers
    /// an error the caller may ignore when the bytes are only an
    /// optimisation to free (staging's are: they are already durable in
    /// the chunk cache).
    fn punch_hole(&self, file: &File, offset: u64, len: u64) -> io::Result<()>;

    /// Allocate blocks for `len` bytes at `offset`, so later writes there
    /// cannot fail for lack of space. With `keep_size` the file's length
    /// does not change; without it the file grows to cover the range.
    fn preallocate(&self, file: &File, offset: u64, len: u64, keep_size: bool) -> io::Result<()>;

    /// Flush `file` to stable storage, through the drive's own cache
    /// where the host distinguishes (macOS `F_FULLFSYNC`; on Linux
    /// `fsync` already asks the device to flush).
    fn full_fsync(&self, file: &File) -> io::Result<()>;

    /// Advise the host to drop `file`'s cached pages for `len` bytes at
    /// `offset` (`len == 0`: to the end), so the next read comes from the
    /// filesystem (Linux `POSIX_FADV_DONTNEED`). Advisory: an error only
    /// means the pages may stay.
    fn drop_cache(&self, file: &File, offset: u64, len: u64) -> io::Result<()>;

    /// Set extended attribute `name` (e.g. `user.color`) of `path`, or of
    /// what it points to when it is a symlink, creating it or replacing
    /// its value. For code that drives a mounted filesystem through the
    /// kernel (the harness's scenarios); the engine keeps its own
    /// attributes in the metadata store. Hosts without extended
    /// attributes answer [`io::ErrorKind::Unsupported`].
    fn set_xattr(&self, _path: &Path, _name: &str, _value: &[u8]) -> io::Result<()> {
        Err(crate::unsupported("extended attributes"))
    }

    /// The value of extended attribute `name` of `path` (symlinks
    /// followed). A missing attribute is the OS's error (Linux `ENODATA`,
    /// macOS `ENOATTR`).
    fn get_xattr(&self, _path: &Path, _name: &str) -> io::Result<Vec<u8>> {
        Err(crate::unsupported("extended attributes"))
    }

    /// The names of `path`'s extended attributes (symlinks followed), in
    /// the order the host lists them.
    fn list_xattr(&self, _path: &Path) -> io::Result<Vec<String>> {
        Err(crate::unsupported("extended attributes"))
    }

    /// Remove extended attribute `name` of `path` (symlinks followed).
    fn remove_xattr(&self, _path: &Path, _name: &str) -> io::Result<()> {
        Err(crate::unsupported("extended attributes"))
    }
}

#[cfg(target_os = "linux")]
pub use linux::NativeFs;

/// `FsPrimitives` for Linux: `fallocate(2)` hole punching and
/// preallocation, `posix_fadvise(2)`, `setxattr(2)` and friends.
#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    #[derive(Debug, Clone, Copy, Default)]
    pub struct NativeFs;

    fn off_t(n: u64) -> io::Result<libc::off_t> {
        libc::off_t::try_from(n).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("offset {n} out of range"),
            )
        })
    }

    fn fallocate(file: &File, mode: libc::c_int, offset: u64, len: u64) -> io::Result<()> {
        // SAFETY: `file` owns a valid open fd for the duration of the call.
        let rc = unsafe { libc::fallocate(file.as_raw_fd(), mode, off_t(offset)?, off_t(len)?) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn cpath(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
    }

    fn cname(name: &str) -> io::Result<CString> {
        CString::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))
    }

    /// Read a variable-length xattr result: ask for its size, then for the
    /// bytes, asking again if it grew in between (`ERANGE`).
    fn read_sized(
        mut call: impl FnMut(*mut libc::c_void, usize) -> libc::ssize_t,
    ) -> io::Result<Vec<u8>> {
        loop {
            let size = call(std::ptr::null_mut(), 0);
            if size < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buf = vec![0u8; size as usize];
            let read = call(buf.as_mut_ptr().cast(), buf.len());
            if read < 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::ERANGE) {
                    continue;
                }
                return Err(e);
            }
            buf.truncate(read as usize);
            return Ok(buf);
        }
    }

    impl FsPrimitives for NativeFs {
        fn punch_hole(&self, file: &File, offset: u64, len: u64) -> io::Result<()> {
            fallocate(
                file,
                libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
                offset,
                len,
            )
        }

        fn preallocate(
            &self,
            file: &File,
            offset: u64,
            len: u64,
            keep_size: bool,
        ) -> io::Result<()> {
            let mode = if keep_size {
                libc::FALLOC_FL_KEEP_SIZE
            } else {
                0
            };
            fallocate(file, mode, offset, len)
        }

        fn full_fsync(&self, file: &File) -> io::Result<()> {
            file.sync_all()
        }

        fn drop_cache(&self, file: &File, offset: u64, len: u64) -> io::Result<()> {
            // SAFETY: `file` owns a valid open fd; the advice is advisory.
            let rc = unsafe {
                libc::posix_fadvise(
                    file.as_raw_fd(),
                    off_t(offset)?,
                    off_t(len)?,
                    libc::POSIX_FADV_DONTNEED,
                )
            };
            // posix_fadvise returns the error number instead of setting errno.
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(rc));
            }
            Ok(())
        }

        fn set_xattr(&self, path: &Path, name: &str, value: &[u8]) -> io::Result<()> {
            let (path, name) = (cpath(path)?, cname(name)?);
            // SAFETY: valid NUL-terminated strings and a live buffer.
            let rc = unsafe {
                libc::setxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            };
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn get_xattr(&self, path: &Path, name: &str) -> io::Result<Vec<u8>> {
            let (path, name) = (cpath(path)?, cname(name)?);
            // SAFETY: valid NUL-terminated strings; `read_sized` passes a
            // null buffer with size 0, or a live one of the size it gives.
            read_sized(|buf, len| unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), buf, len) })
        }

        fn list_xattr(&self, path: &Path) -> io::Result<Vec<String>> {
            let path = cpath(path)?;
            // SAFETY: as in `get_xattr`.
            let names =
                read_sized(|buf, len| unsafe { libc::listxattr(path.as_ptr(), buf.cast(), len) })?;
            Ok(names
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect())
        }

        fn remove_xattr(&self, path: &Path, name: &str) -> io::Result<()> {
            let (path, name) = (cpath(path)?, cname(name)?);
            // SAFETY: valid NUL-terminated strings.
            if unsafe { libc::removexattr(path.as_ptr(), name.as_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Seek, SeekFrom, Write};

        #[test]
        fn preallocate_and_punch_hole_keep_the_length() {
            let dir = tempfile::tempdir().unwrap();
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .read(true)
                .write(true)
                .open(dir.path().join("f"))
                .unwrap();
            f.write_all(&[7u8; 64 * 1024]).unwrap();
            f.sync_all().unwrap();
            match NativeFs.punch_hole(&f, 4096, 8192) {
                Ok(()) => {
                    assert_eq!(f.metadata().unwrap().len(), 64 * 1024);
                    f.seek(SeekFrom::Start(4096)).unwrap();
                    let mut b = [1u8; 16];
                    std::io::Read::read_exact(&mut f, &mut b).unwrap();
                    assert_eq!(b, [0u8; 16]);
                }
                Err(e) => eprintln!("SKIPPED (not run): no hole punching here ({e})"),
            }
            NativeFs.full_fsync(&f).unwrap();
            NativeFs.drop_cache(&f, 0, 0).unwrap();
        }

        #[test]
        fn xattrs_round_trip_and_report_a_missing_one() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("f");
            std::fs::write(&path, b"x").unwrap();
            match NativeFs.set_xattr(&path, "user.replicix.test", b"blue") {
                Ok(()) => {}
                Err(e) if e.raw_os_error() == Some(libc::ENOTSUP) => {
                    eprintln!("SKIPPED (not run): no extended attributes here ({e})");
                    return;
                }
                Err(e) => panic!("{e}"),
            }
            assert_eq!(
                NativeFs.get_xattr(&path, "user.replicix.test").unwrap(),
                b"blue"
            );
            assert!(NativeFs
                .list_xattr(&path)
                .unwrap()
                .contains(&"user.replicix.test".to_string()));
            NativeFs.remove_xattr(&path, "user.replicix.test").unwrap();
            assert!(NativeFs.get_xattr(&path, "user.replicix.test").is_err());
        }
    }
}

// SPDX-License-Identifier: MPL-2.0
//! Keeping a process's memory out of core dumps.

use std::io;

/// Keep this process's memory out of core dumps: a process
/// whose credentials live only in memory — a process waiting
/// for credentials to be handed to it, one holding Secrets it passes on — must not
/// write them wherever the node's `core_pattern` points when it crashes.
/// `RLIMIT_CORE` 0 stops a core file (and handlers like systemd-coredump
/// that honour it); on Linux the process is also made non-dumpable
/// (`PR_SET_DUMPABLE` 0), which stops every core handler and keeps other
/// processes of the same uid from reading its memory through `/proc` or
/// `ptrace`. A no-op where neither exists.
pub fn forbid_core_dumps() -> io::Result<()> {
    #[cfg(unix)]
    {
        let none = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: a valid rlimit for this process.
        if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &none) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: PR_SET_DUMPABLE takes one integer argument.
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Whether [`forbid_core_dumps`] is in effect for this process.
pub fn core_dumps_forbidden() -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: PR_GET_DUMPABLE takes no argument.
        if unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return false;
        }
    }
    #[cfg(unix)]
    {
        let mut limit = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        // SAFETY: a valid out-pointer.
        if unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) } != 0 || limit.rlim_cur != 0 {
            return false;
        }
        true
    }
    #[cfg(not(unix))]
    false
}

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    #[test]
    fn forbidding_core_dumps_sticks_for_the_process() {
        // Run in a child so the test process stays dumpable.
        let me = std::env::current_exe().unwrap();
        let out = std::process::Command::new(me)
            .args(["--exact", "coredump::tests::child_forbids", "--nocapture"])
            .env("REPLICIX_COREDUMP_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn child_forbids() {
        if std::env::var_os("REPLICIX_COREDUMP_CHILD").is_none() {
            return;
        }
        super::forbid_core_dumps().unwrap();
        assert!(super::core_dumps_forbidden());
    }
}

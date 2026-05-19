//! Shared filesystem and environment utilities.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{env, path::Path};

/// Splits the `PATH` environment variable into individual directory strings.
/// Returns an empty `Vec` when `PATH` is unset.
pub(crate) fn get_paths() -> Vec<String> {
    match env::var("PATH") {
        Ok(path_var) => env::split_paths(&path_var)
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        Err(_) => vec![],
    }
}

/// Returns `true` if `path` has any execute bit set (Unix), `false` otherwise or on error.
pub(crate) fn is_executable(path: &Path) -> bool {
    #[cfg(windows)]
    {
        println!("Windows is not supported.");
        process.exit(0);
    }

    #[cfg(unix)]
    {
        fs::metadata(path)
            .map(|f| f.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

/// Serializes all tests that write and then exec a script file, or that spawn child
/// processes (fork), to prevent the ETXTBSY race.
///
/// ETXTBSY occurs when `execve(script)` is called while another thread's recently-forked
/// child still holds an inherited write-fd for `script` (the fd is open between `fork` and
/// the child's `exec`, even though it is `O_CLOEXEC`).  Holding this lock for the full
/// write-then-exec window, and for each `Command::spawn` call, ensures the windows never
/// overlap.
#[cfg(test)]
pub(crate) fn fork_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn is_executable_true_for_executable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bin");
        fs::write(&path, b"").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&path));
    }

    #[test]
    fn is_executable_false_for_non_executable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data");
        fs::write(&path, b"").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&path));
    }

    #[test]
    fn is_executable_false_for_nonexistent_path() {
        assert!(!is_executable(Path::new("/nonexistent/path/xyz_shell_test")));
    }

    #[test]
    fn get_paths_returns_strings() {
        // PATH is set in any normal test environment; verify the return type is sensible.
        let paths = get_paths();
        // Every entry must be a valid (non-empty) string — no raw OsStr leakage.
        for p in &paths {
            assert!(!p.is_empty());
        }
    }
}

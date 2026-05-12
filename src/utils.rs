use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{env, path::Path};

pub(crate) fn get_paths() -> Vec<String> {
    match env::var("PATH") {
        Ok(path_var) => env::split_paths(&path_var)
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        Err(_) => vec![],
    }
}

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

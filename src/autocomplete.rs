use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use rustyline::{
    Changeset, Context, Helper, Highlighter, Hinter, Validator,
    completion::{Completer, FilenameCompleter, Pair},
    line_buffer::LineBuffer,
};

use crate::{
    builtin::Builtin,
    utils::{get_paths, is_executable},
};

/// Tab-completion helper for rustyline that suggests shell builtins and `PATH` executables.
#[derive(Helper, Hinter, Highlighter, Validator)]
pub(crate) struct AutoCompletion {
    paths: Vec<String>,
    file_completer: FilenameCompleter,
}

impl AutoCompletion {
    /// Creates a new `AutoCompletion` by reading the `PATH` environment variable.
    pub(crate) fn new() -> Self {
        let file_completer = FilenameCompleter::new();
        let paths = get_paths();
        Self {
            paths,
            file_completer,
        }
    }

    #[cfg(test)]
    fn with_paths(paths: Vec<String>) -> Self {
        let file_completer = FilenameCompleter::new();
        Self {
            paths,
            file_completer,
        }
    }

    /// Returns all executable files inside `dir` whose name starts with `partial_name`.
    fn find_executables_by_partial_name(dir: &Path, partial_name: &str) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(dir) else {
            return vec![];
        };

        entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let matches = path
                    .file_name()
                    .map(|s| s.to_string_lossy().starts_with(partial_name))
                    .unwrap_or(false);
                (matches && path.is_file() && is_executable(&path)).then_some(path)
            })
            .collect()
    }
}

impl Completer for AutoCompletion {
    type Candidate = Pair;

    /// Returns completions for `line` by matching its prefix against builtins and `PATH` executables.
    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let commands = Builtin::builtin_cmds();
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        // Check Builtins
        for cmd in commands {
            if cmd.starts_with(line) {
                candidates.push(Pair {
                    display: format!("{} ", cmd.to_owned()),
                    replacement: format!("{} ", cmd.to_owned()),
                });
            }
        }

        // Check PATH executables
        for path_str in &self.paths {
            let path = Path::new(path_str);

            let path_list = AutoCompletion::find_executables_by_partial_name(path, line);

            for p in path_list {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();

                if seen.insert(name.clone()) {
                    candidates.push(Pair {
                        display: format!("{} ", name),
                        replacement: format!("{} ", name),
                    })
                }
            }
        }

        if let Ok((_, mut file_candidates)) = self.file_completer.complete(line, pos, ctx) {
            candidates.append(file_candidates.as_mut());
        }

        candidates.sort_by(|a, b| a.display.cmp(&b.display));

        Ok((0, candidates))
    }

    /// Replaces the text in `line` from `start` to the cursor with `elected`.
    fn update(&self, line: &mut LineBuffer, start: usize, elected: &str, cl: &mut Changeset) {
        let new_elected = if elected.ends_with('/') {
            format!(" {}", elected)
        } else {
            format!(" {} ", elected)
        };

        let (start, elected) = match line.rfind(' ') {
            Some(s) => (s, new_elected.as_str()),
            None => (start, elected),
        };

        let end = line.pos();
        line.replace(start..end, elected, cl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::{Context, history::DefaultHistory};
    use std::{fs, os::unix::fs::PermissionsExt};

    fn ctx(history: &DefaultHistory) -> Context<'_> {
        Context::new(history)
    }

    // --- find_executables_by_partial_name ---

    #[test]
    fn find_exec_matches_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("mybin");
        fs::write(&bin, b"").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let results = AutoCompletion::find_executables_by_partial_name(dir.path(), "my");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name().unwrap(), "mybin");
    }

    #[test]
    fn find_exec_ignores_non_executable() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("myfile"), b"").unwrap();

        let results = AutoCompletion::find_executables_by_partial_name(dir.path(), "my");
        assert!(results.is_empty());
    }

    #[test]
    fn find_exec_ignores_non_matching_name() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("othertool");
        fs::write(&bin, b"").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let results = AutoCompletion::find_executables_by_partial_name(dir.path(), "my");
        assert!(results.is_empty());
    }

    #[test]
    fn find_exec_ignores_directories() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("mydir");
        fs::create_dir(&subdir).unwrap();

        let results = AutoCompletion::find_executables_by_partial_name(dir.path(), "my");
        assert!(results.is_empty());
    }

    #[test]
    fn find_exec_nonexistent_dir_returns_empty() {
        let results = AutoCompletion::find_executables_by_partial_name(
            Path::new("/nonexistent/path/xyz_shell_test"),
            "foo",
        );
        assert!(results.is_empty());
    }

    // --- complete: builtins ---

    #[test]
    fn complete_builtin_prefix() {
        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let (start, candidates) = ac.complete("ec", 2, &ctx(&h)).unwrap();
        assert_eq!(start, 0);
        let replacements: Vec<&str> = candidates.iter().map(|p| p.replacement.as_str()).collect();
        assert!(replacements.contains(&"echo "));
    }

    #[test]
    fn complete_builtin_multiple_matches() {
        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let (_, candidates) = ac.complete("e", 1, &ctx(&h)).unwrap();
        let replacements: Vec<&str> = candidates.iter().map(|p| p.replacement.as_str()).collect();
        assert!(replacements.contains(&"echo "));
        assert!(replacements.contains(&"exit "));
    }

    #[test]
    fn complete_no_match_returns_empty() {
        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let (_, candidates) = ac.complete("zzz_no_such_cmd", 15, &ctx(&h)).unwrap();
        assert!(candidates.is_empty());
    }

    // --- complete: PATH executables ---

    #[test]
    fn complete_path_executable() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("mytool");
        fs::write(&bin, b"").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let ac = AutoCompletion::with_paths(vec![dir.path().to_str().unwrap().to_owned()]);
        let h = DefaultHistory::new();
        let (_, candidates) = ac.complete("my", 2, &ctx(&h)).unwrap();
        let replacements: Vec<&str> = candidates.iter().map(|p| p.replacement.as_str()).collect();
        assert!(replacements.contains(&"mytool "));
    }

    #[test]
    fn complete_path_non_executable_excluded() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("myfile"), b"").unwrap();

        let ac = AutoCompletion::with_paths(vec![dir.path().to_str().unwrap().to_owned()]);
        let h = DefaultHistory::new();
        let (_, candidates) = ac.complete("my", 2, &ctx(&h)).unwrap();
        assert!(candidates.is_empty());
    }

    #[test]
    fn complete_candidates_include_trailing_space() {
        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let (_, candidates) = ac.complete("pw", 2, &ctx(&h)).unwrap();
        assert!(candidates.iter().all(|p| p.replacement.ends_with(' ')));
    }
}

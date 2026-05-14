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

        let mut start = 0;
        if let Ok((file_start, file_candidates)) = self.file_completer.complete(line, pos, ctx) {
            if !file_candidates.is_empty() {
                start = file_start;
            }
            let mut file_candidates = file_candidates
                .iter()
                .map(|c| {
                    let rep = if c.replacement.ends_with('/') {
                        c.replacement.clone()
                    } else {
                        format!("{} ", c.replacement)
                    };
                    Pair {
                        display: rep.clone(),
                        replacement: rep,
                    }
                })
                .collect::<Vec<_>>();

            candidates.append(file_candidates.as_mut());
        }

        candidates.sort_by(|a, b| a.display.cmp(&b.display));

        Ok((start, candidates))
    }

    /// Replaces the text in `line` from `start` to the cursor with `elected`.
    fn update(&self, line: &mut LineBuffer, start: usize, elected: &str, cl: &mut Changeset) {
        let end = line.pos();
        line.replace(start..end, elected, cl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::{Context, completion::longest_common_prefix, history::DefaultHistory};
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

    // --- complete: file completion ---

    #[test]
    fn complete_file_single_match() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("myfile.txt"), b"").unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/myf", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(candidates.len(), 1);
        let rep = &candidates[0].replacement;
        assert!(rep.contains("myfile.txt"), "got '{rep}'");
        assert!(rep.ends_with(' '), "file candidate should have trailing space, got '{rep}'");
    }

    #[test]
    fn complete_file_single_dir_gets_slash_not_space() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("mydir")).unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/my", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(candidates.len(), 1);
        let rep = &candidates[0].replacement;
        assert!(rep.ends_with('/'), "directory should end with '/', got '{rep}'");
        assert!(!rep.ends_with("//"), "no double slash, got '{rep}'");
        assert!(!rep.ends_with(' '), "directory should not end with space, got '{rep}'");
    }

    #[test]
    fn complete_file_multiple_matches_all_returned() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("foo_alpha"), b"").unwrap();
        fs::write(dir.path().join("foo_beta"), b"").unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/foo", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(
            candidates.len(),
            2,
            "both matching files should be returned"
        );
        let reps: Vec<&str> = candidates.iter().map(|c| c.replacement.as_str()).collect();
        assert!(reps.iter().any(|r| r.contains("foo_alpha")));
        assert!(reps.iter().any(|r| r.contains("foo_beta")));
    }

    #[test]
    fn complete_file_no_match_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("other.txt"), b"").unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/zzz", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert!(
            candidates.is_empty(),
            "no files match prefix 'zzz', expected empty"
        );
    }

    #[test]
    fn complete_file_nested_path() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("target_file"), b"").unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/subdir/tar", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(candidates.len(), 1);
        assert!(
            candidates[0].replacement.contains("target_file"),
            "replacement should contain 'target_file', got '{}'",
            candidates[0].replacement
        );
    }

    #[test]
    fn complete_file_three_matches_all_returned() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bar_one"), b"").unwrap();
        fs::write(dir.path().join("bar_two"), b"").unwrap();
        fs::write(dir.path().join("bar_three"), b"").unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/bar", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(
            candidates.len(),
            3,
            "all three matching files should be returned"
        );
        let reps: Vec<&str> = candidates.iter().map(|c| c.replacement.as_str()).collect();
        assert!(reps.iter().any(|r| r.contains("bar_one")));
        assert!(reps.iter().any(|r| r.contains("bar_two")));
        assert!(reps.iter().any(|r| r.contains("bar_three")));
    }

    // --- progressive LCP through nested directories ---
    //
    // Given: xyz_foo/  xyz_foo_bar/  xyz_foo_bar_baz/
    //
    //   xyz_<TAB>         → "xyz_foo"        (LCP, no slash — not a unique dir yet)
    //   xyz_foo_<TAB>     → "xyz_foo_bar"    (LCP, no slash — still ambiguous)
    //   xyz_foo_bar_<TAB> → "xyz_foo_bar_baz/" (single match, slash from FilenameCompleter)

    #[test]
    fn complete_dir_progressive_lcp_step1() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("xyz_foo")).unwrap();
        fs::create_dir(dir.path().join("xyz_foo_bar")).unwrap();
        fs::create_dir(dir.path().join("xyz_foo_bar_baz")).unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/xyz_", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(candidates.len(), 3);

        // rustyline computes the LCP of all candidates and passes it to update().
        let lcp =
            longest_common_prefix(&candidates).expect("candidates should have a common prefix");
        assert!(
            lcp.ends_with("xyz_foo"),
            "LCP should extend to 'xyz_foo', got '{lcp}'"
        );
        assert!(
            !lcp.ends_with('/'),
            "LCP of multiple dirs should not have trailing slash"
        );
    }

    #[test]
    fn complete_dir_progressive_lcp_step2() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("xyz_foo")).unwrap();
        fs::create_dir(dir.path().join("xyz_foo_bar")).unwrap();
        fs::create_dir(dir.path().join("xyz_foo_bar_baz")).unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/xyz_foo_", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(
            candidates.len(),
            2,
            "two dirs starting with 'xyz_foo_' should be returned"
        );

        let lcp =
            longest_common_prefix(&candidates).expect("candidates should have a common prefix");
        assert!(
            lcp.ends_with("xyz_foo_bar"),
            "LCP should extend to 'xyz_foo_bar', got '{lcp}'"
        );
        assert!(
            !lcp.ends_with('/'),
            "LCP of multiple dirs should not have trailing slash"
        );
    }

    #[test]
    fn complete_dir_progressive_lcp_step3_single_match_gets_slash() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("xyz_foo")).unwrap();
        fs::create_dir(dir.path().join("xyz_foo_bar")).unwrap();
        fs::create_dir(dir.path().join("xyz_foo_bar_baz")).unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/xyz_foo_bar_", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(
            candidates.len(),
            1,
            "single remaining dir should give one candidate"
        );
        let rep = &candidates[0].replacement;
        assert!(
            rep.ends_with("xyz_foo_bar_baz/"),
            "sole dir match should end with '/', got '{rep}'"
        );
        assert!(
            !rep.ends_with("//"),
            "replacement must not have double slash, got '{rep}'"
        );
    }

    // --- file trailing space is lost in LCP of multiple matches ---
    //
    // File candidates carry a trailing space so a unique match inserts one.
    // When multiple files share a prefix, rustyline's LCP stops at the first
    // character where they diverge — before the space — so no space is inserted
    // for a partial completion.  e.g. "foo_alpha " + "foo_beta " → LCP "foo_"

    #[test]
    fn complete_multiple_files_lcp_loses_trailing_space() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("foo_alpha"), b"").unwrap();
        fs::write(dir.path().join("foo_beta"), b"").unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/foo", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        assert_eq!(candidates.len(), 2);
        let lcp = longest_common_prefix(&candidates).expect("should have LCP");
        assert!(lcp.ends_with("foo_"), "LCP should be 'foo_', got '{lcp}'");
        assert!(!lcp.ends_with(' '), "partial LCP should not end with space, got '{lcp}'");
    }

    // --- listing: dirs end with '/', files end with ' ' ---

    #[test]
    fn complete_mixed_candidates_dirs_slash_files_space() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bar.txt"), b"").unwrap();
        fs::create_dir(dir.path().join("foo")).unwrap();

        let ac = AutoCompletion::with_paths(vec![]);
        let h = DefaultHistory::new();
        let prefix = format!("{}/", dir.path().display());
        let (_, candidates) = ac.complete(&prefix, prefix.len(), &ctx(&h)).unwrap();

        for c in &candidates {
            let rep = &c.replacement;
            if rep.contains("foo") {
                assert!(rep.ends_with('/'), "directory should end with '/', got '{rep}'");
                assert!(!rep.ends_with("//"), "no double slash, got '{rep}'");
            }
            if rep.contains("bar.txt") {
                assert!(rep.ends_with(' '), "file should end with space, got '{rep}'");
                assert!(!rep.ends_with('/'), "file should not end with '/', got '{rep}'");
            }
        }
    }
}

//! Command dispatch: parses a raw input line and routes it to builtins or PATH executables.

use std::collections::HashMap;

use rustyline::history::{DefaultHistory, FileHistory, History};

use crate::builtin::{Builtin, SharedCompletions};
use crate::command::{PipelineResult, build_pipeline, execute_pipeline};
use crate::history::Helper;
use crate::jobs::Jobs;
use crate::utils::get_paths;

/// Resolves and executes shell commands against the entries in `PATH`.
pub(crate) struct Processor {
    paths: Vec<String>,
    bi: Builtin,
    jobs: Jobs,
    history_helper: Helper,
    declare_vars: HashMap<String, String>,
    last_exit_code: i32,
}

impl Processor {
    /// Creates a new `Processor` by reading the `PATH` environment variable.
    pub(crate) fn new() -> Self {
        Self {
            paths: get_paths(),
            bi: Builtin::new(),
            jobs: Jobs::new(),
            history_helper: Helper::new(),
            declare_vars: HashMap::new(),
            last_exit_code: 0,
        }
    }

    // pub(crate) fn last_exit_code(&self) -> i32 {
    //     self.last_exit_code
    // }

    pub(crate) fn load_history(&mut self, history: &mut FileHistory) {
        match &self.history_helper.read_history_file() {
            Ok(hist) => {
                for h in hist.iter() {
                    let _ = history.add(h);
                }
            }

            Err(e) => eprintln!("{e}"),
        }
    }

    pub(crate) fn save_history(&mut self, history: &mut FileHistory) {
        let _ = &self.history_helper.write_history_file(history);
    }

    /// Parses and dispatches a full command line, routing to builtins or external executables.
    pub(crate) fn process_command(&mut self, input: &str, history: &mut DefaultHistory) {
        let input = input.trim();
        if input.is_empty() {
            return;
        }

        // Look for non-pipeline builtins
        let first_token = input.split_whitespace().next().unwrap_or("");

        match first_token {
            "cd" => {
                let args = input.strip_prefix(first_token).unwrap_or("").trim();
                Builtin::cd(args);
                return;
            }
            "declare" => {
                let args: Vec<&str> = input.split_whitespace().skip(1).collect();
                match args[0] {
                    "-p" => {
                        if args.len() == 2 {
                            match self.declare_vars.get(args[1]) {
                                Some(val) => println!("{}", val),
                                None => println!("declare: {}: not found", args[1]),
                            }
                        }
                    }
                    _ => println!("WTF!"),
                }
            }
            "exit" => {
                self.save_history(history);
                let _ = Builtin::exit();
                return;
            }
            "jobs" => {
                self.jobs.print_jobs();
                return;
            }
            "history" => {
                let args: Vec<&str> = input.split_whitespace().skip(1).collect();

                if args.is_empty() {
                    for i in 0..history.len() {
                        println!("  {} {}", i + 1, history[i]);
                    }
                } else {
                    match args[0] {
                        "-r" => {
                            if args.len() == 2 {
                                match self.history_helper.read_file(args[1]) {
                                    Ok(hist) => {
                                        for h in hist {
                                            let _ = history.add(h.as_str());
                                        }
                                    }
                                    Err(e) => eprintln!("Error reading history file: {}", e),
                                }
                            }

                            return;
                        }
                        "-a" => {
                            if args.len() == 2 {
                                self.history_helper.append_file(args[1], history);
                            }
                            return;
                        }
                        "-w" => {
                            if args.len() == 2 {
                                self.history_helper.write_file(args[1], history);
                            }
                            return;
                        }
                        _ => {
                            let hist_count = if args.is_empty() {
                                0
                            } else {
                                let count = args[0].parse::<usize>().unwrap_or(0);
                                history.len().saturating_sub(count)
                            };

                            for i in hist_count..history.len() {
                                println!("  {} {}", i + 1, history[i]);
                            }
                        }
                    }
                }
                return;
            }
            _ => {}
        }

        let segments = build_pipeline(input, &self.paths, self.bi.completions());
        match execute_pipeline(segments) {
            PipelineResult::Foreground(code) => {
                self.jobs.check_done_jobs();
                self.last_exit_code = code;
            }
            PipelineResult::Background(child) => {
                let cmd = input
                    .trim_end()
                    .strip_suffix('&')
                    .unwrap_or(input)
                    .trim_end()
                    .to_string();
                self.jobs.track(child, cmd);
            }
        }
    }

    /// Returns the shared completions handle so the tab-completion helper can read it.
    pub(crate) fn shared_completions(&self) -> SharedCompletions {
        self.bi.completions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::history::History;
    use std::{env, fs};
    use tempfile::NamedTempFile;

    /// Serializes all tests that read or write the process-wide PATH variable.
    ///
    /// `env::set_var`/`remove_var` affect every thread in the process, so any two
    /// tests that touch PATH must not run concurrently.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn make_hist(entries: &[&str]) -> DefaultHistory {
        let mut h = DefaultHistory::new();
        for e in entries {
            let _ = h.add(e);
        }
        h
    }

    // --- Processor::process_command ---

    #[test]
    fn test_process_empty_input_no_panic() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        p.process_command("", &mut hist);
        p.process_command("   ", &mut hist);
    }

    // --- cd ---

    #[test]
    fn test_process_cd_valid_path() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        let orig = env::current_dir().unwrap();
        p.process_command("cd /tmp", &mut hist);
        let _ = env::set_current_dir(&orig);
    }

    #[test]
    fn test_process_cd_invalid_path_no_panic() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        p.process_command("cd /this/path/does/not/exist/xyz_shell_test", &mut hist);
    }

    #[test]
    fn test_cd_changes_directory() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        let orig = env::current_dir().unwrap();
        p.process_command("cd /tmp", &mut hist);
        let cwd = env::current_dir().unwrap();
        let _ = env::set_current_dir(&orig);
        assert_eq!(cwd.to_str().unwrap(), "/tmp");
    }

    // --- declare ---

    #[test]
    fn test_declare_p_missing_var_no_panic() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        // Variable was never set; should print "not found" without panicking.
        p.process_command("declare -p NONEXISTENT_VAR_XYZ", &mut hist);
    }

    // --- jobs ---

    #[test]
    fn test_jobs_empty_no_panic() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        p.process_command("jobs", &mut hist);
    }

    // --- history (no args) ---

    #[test]
    fn test_history_no_args_no_panic() {
        let mut hist = make_hist(&["echo hello", "ls -la"]);
        let mut p = Processor::new();
        p.process_command("history", &mut hist);
    }

    #[test]
    fn test_history_empty_no_panic() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        p.process_command("history", &mut hist);
    }

    // --- history N (count) ---

    #[test]
    fn test_history_count_no_panic() {
        let mut hist = make_hist(&["a", "b", "c", "d", "e"]);
        let mut p = Processor::new();
        p.process_command("history 2", &mut hist);
    }

    #[test]
    fn test_history_count_larger_than_history_no_panic() {
        let mut hist = make_hist(&["only"]);
        let mut p = Processor::new();
        p.process_command("history 100", &mut hist);
    }

    // --- history -r ---

    #[test]
    fn test_history_r_loads_entries_from_file() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        fs::write(&path, "cmd_one\ncmd_two\n").unwrap();

        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        p.process_command(&format!("history -r {}", path), &mut hist);

        assert_eq!(hist.len(), 2);
        assert_eq!(format!("{}", hist[0]), "cmd_one");
        assert_eq!(format!("{}", hist[1]), "cmd_two");
    }

    #[test]
    fn test_history_r_nonexistent_file_no_panic() {
        let mut hist = DefaultHistory::new();
        let mut p = Processor::new();
        p.process_command("history -r /tmp/no_such_file_xyz_shell_test", &mut hist);
        assert_eq!(hist.len(), 0);
    }

    // --- history -a ---

    #[test]
    fn test_history_a_appends_to_file() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();

        let mut hist = make_hist(&["first", "second"]);
        let mut p = Processor::new();
        p.process_command(&format!("history -a {}", path), &mut hist);

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("first"));
        assert!(contents.contains("second"));
    }

    #[test]
    fn test_history_a_only_appends_new_entries() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();

        let mut hist = make_hist(&["alpha", "beta"]);
        let mut p = Processor::new();

        // First append writes both entries.
        p.process_command(&format!("history -a {}", path), &mut hist);

        // Add a third entry and append again.
        let _ = hist.add("gamma");
        p.process_command(&format!("history -a {}", path), &mut hist);

        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        // "gamma" should appear exactly once.
        assert_eq!(lines.iter().filter(|&&l| l == "gamma").count(), 1);
    }

    // --- history -w ---

    #[test]
    fn test_history_w_writes_all_entries() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();

        let mut hist = make_hist(&["one", "two", "three"]);
        let mut p = Processor::new();
        p.process_command(&format!("history -w {}", path), &mut hist);

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("one"));
        assert!(contents.contains("two"));
        assert!(contents.contains("three"));
    }

    // --- Processor::new ---

    #[test]
    fn test_new_reads_path_entries() {
        let _lock = env_lock();
        let orig = env::var("PATH").ok();
        unsafe { env::set_var("PATH", "/usr/bin:/bin") };
        let cmds = Processor::new();
        match orig {
            Some(p) => unsafe { env::set_var("PATH", p) },
            None => unsafe { env::remove_var("PATH") },
        }
        assert!(cmds.paths.contains(&"/usr/bin".to_string()));
        assert!(cmds.paths.contains(&"/bin".to_string()));
    }

    #[test]
    fn test_new_empty_when_path_unset() {
        let _lock = env_lock();
        let orig = env::var("PATH").ok();
        unsafe { env::remove_var("PATH") };
        let cmds = Processor::new();
        match orig {
            Some(p) => unsafe { env::set_var("PATH", p) },
            None => unsafe { env::remove_var("PATH") },
        }
        assert!(cmds.paths.is_empty());
    }
}

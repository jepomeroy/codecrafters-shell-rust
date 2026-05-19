//! Command dispatch: parses a raw input line and routes it to builtins or PATH executables.

use anyhow::anyhow;
use std::collections::VecDeque;
use std::io::stdout;
use std::path::Path;

use crate::builtin::{Builtin, SharedCompletions};
use crate::command::InternalCommand;
use crate::jobs::Jobs;
use crate::utils::{get_paths, is_executable};

/// Resolves and executes shell commands against the entries in `PATH`.
pub(crate) struct Processor {
    paths: Vec<String>,
    bi: Builtin,
    jobs: Jobs,
}

impl Processor {
    /// Creates a new `Processor` by reading the `PATH` environment variable.
    pub(crate) fn new() -> Self {
        Self {
            paths: get_paths(),
            bi: Builtin::new(),
            jobs: Jobs::new(),
        }
    }

    /// Returns the shared completions handle so the tab-completion helper can read it.
    pub(crate) fn shared_completions(&self) -> SharedCompletions {
        self.bi.completions()
    }

    /// Searches `PATH` for an executable named `cmd`. Returns its full path if found.
    fn is_executable_command(&self, cmd: &str) -> Option<String> {
        for path in &self.paths {
            let cmd_path = Path::new(path).join(cmd);

            if cmd_path.exists() && is_executable(&cmd_path) {
                return Some(cmd_path.to_string_lossy().into_owned());
            }
        }

        None
    }

    /// Splits a raw input line into a command name and its parsed arguments.
    ///
    /// Returns an error if the trimmed input is empty.
    fn parse_cmd(input: &str) -> Result<InternalCommand, anyhow::Error> {
        let input = input.trim();

        if input.is_empty() {
            return Err(anyhow!("command is empty"));
        }

        let commands = input
            .split('|')
            .map(|s| s.trim())
            .collect::<VecDeque<&str>>();

        let internal_cmds = InternalCommand::new(commands).expect("Could not build command");

        Ok(internal_cmds)
    }

    /// Parses and dispatches a full command line, routing to builtins or external executables.
    pub(crate) fn process_command(&mut self, input: &str) {
        match Processor::parse_cmd(input) {
            Ok(mut cmd) => {
                match cmd.get_command() {
                    "cd" => {
                        cmd.get_args().iter().for_each(|p| {
                            let _ = self.bi.cd(p);
                        });

                        0
                    }
                    "complete" => self.bi.complete(&cmd.get_args(), &mut stdout()),
                    "echo" => {
                        let args = cmd.get_args();

                        if let Some(cmd_path) = self.is_executable_command(cmd.get_command()) {
                            let cmd = InternalCommand::new_with_args(cmd_path, args);
                            cmd.execute(None);
                        } else {
                            self.bi
                                .echo(args.iter().map(|s| s.as_str()).collect(), &mut stdout());
                        }

                        0
                    }
                    "exit" => Builtin::exit(),
                    "jobs" => {
                        self.jobs.print_jobs();
                        0
                    }
                    "pwd" => self.bi.pwd(),
                    "type" => {
                        for arg in cmd.get_args().iter() {
                            self.bi.check_type(arg, self.is_executable_command(arg));
                        }

                        0
                    }
                    _ => match self.is_executable_command(cmd.get_command()) {
                        Some(_) => {
                            if cmd.is_background() {
                                self.jobs.track(
                                    cmd.execute_background().expect("Command not found"),
                                    cmd.get_command_with_args(),
                                );
                            } else {
                                cmd.execute(None);
                            }

                            0
                        }
                        None => self.bi.unknown(cmd.get_command()),
                    },
                };
            }
            Err(_) => print!(""),
        }

        self.jobs.check_done_jobs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::{env, fs};

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

    #[test]
    fn test_parse_empty() {
        assert!(Processor::parse_cmd("").is_err());
    }

    #[test]
    fn test_cmd_only_parse() {
        let command = Processor::parse_cmd("foo").unwrap();
        assert_eq!(command.get_command(), "foo");
        assert_eq!(command.get_args(), Vec::<&str>::new());
    }

    #[test]
    fn test_simple_parse() {
        let command = Processor::parse_cmd("foo bar").unwrap();
        assert_eq!(command.get_command(), "foo");
        assert_eq!(command.get_args(), vec!["bar"]);
    }

    #[test]
    fn test_long_parse() {
        let command = Processor::parse_cmd("foo bar baz bop").unwrap();
        assert_eq!(command.get_command(), "foo");
        assert_eq!(command.get_args(), vec!["bar", "baz", "bop"]);
    }

    // Leading space: splits into ("", Some("foo")) — cmd is empty string, not an error.
    #[test]
    fn test_parse_leading_space() {
        let command = Processor::parse_cmd(" foo").unwrap();
        assert_eq!(command.get_command(), "foo");
        assert_eq!(command.get_args(), Vec::<&str>::new());
    }

    // Command with single and double quotes
    #[test]
    fn test_parse_cmd_with_single_quotes() {
        let command = Processor::parse_cmd("'my command' arg1").unwrap();
        assert_eq!(command.get_command(), "my command");
        assert_eq!(command.get_args(), vec!["arg1"]);
    }

    #[test]
    fn test_parse_cmd_with_double_quotes() {
        let command = Processor::parse_cmd(r#""my command" arg1"#).unwrap();
        assert_eq!(command.get_command(), "my command");
        assert_eq!(command.get_args(), vec!["arg1"]);
    }

    #[test]
    fn test_parse_cmd_containing_single_quotes() {
        let command = Processor::parse_cmd(r#""my 'command'" arg1"#).unwrap();
        assert_eq!(command.get_command(), r#"my 'command'"#);
        assert_eq!(command.get_args(), vec!["arg1"]);
    }

    #[test]
    fn test_parse_cmd_containing_double_quotes() {
        let command = Processor::parse_cmd(r#"'my "command"' arg1"#).unwrap();
        assert_eq!(command.get_command(), "my \"command\"");
        assert_eq!(command.get_args(), vec!["arg1"]);
    }

    #[test]
    fn test_parse_cmd_with_pipe() {
        let int_cmd = Processor::parse_cmd("ls | grep .rs");
        assert!(int_cmd.is_ok());
    }

    // A single space is not an empty string so parse_cmd should error out.
    #[test]
    fn test_parse_single_space_not_error() {
        assert!(Processor::parse_cmd(" ").is_err());
    }

    // --- Commands::new ---
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

    // --- Commands::is_executalble_command ---

    #[test]
    fn test_is_executable_not_found_empty_paths() {
        let cmds = Processor {
            paths: vec![],
            bi: Builtin::new(),
            jobs: Jobs::new(),
        };
        assert!(cmds.is_executable_command("ls").is_none());
    }

    #[test]
    fn test_is_executable_not_found_wrong_path() {
        let cmds = Processor {
            paths: vec!["/nonexistent/path/xyz_shell_test".to_string()],
            bi: Builtin::new(),
            jobs: Jobs::new(),
        };
        assert!(cmds.is_executable_command("ls").is_none());
    }

    #[test]
    fn test_is_executable_found() {
        let tmpdir = env::temp_dir().join(format!("shell_test_exec_{}", std::process::id()));
        fs::create_dir_all(&tmpdir).unwrap();
        let cmd_file = tmpdir.join("myfakeshellcmd");
        fs::write(&cmd_file, b"#!/bin/sh\necho hi").unwrap();
        let mut perms = fs::metadata(&cmd_file).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&cmd_file, perms).unwrap();

        let cmds = Processor {
            paths: vec![tmpdir.to_string_lossy().into_owned()],
            bi: Builtin::new(),
            jobs: Jobs::new(),
        };

        let result = cmds.is_executable_command("myfakeshellcmd");
        let _ = fs::remove_dir_all(&tmpdir);

        assert!(result.is_some());
        assert!(result.unwrap().ends_with("myfakeshellcmd"));
    }

    #[test]
    fn test_is_executable_non_executable_file_not_returned() {
        let tmpdir = env::temp_dir().join(format!("shell_test_noexec_{}", std::process::id()));
        fs::create_dir_all(&tmpdir).unwrap();
        let cmd_file = tmpdir.join("noexecfile");
        fs::write(&cmd_file, b"data").unwrap();
        let mut perms = fs::metadata(&cmd_file).unwrap().permissions();
        perms.set_mode(0o644); // no execute bit
        fs::set_permissions(&cmd_file, perms).unwrap();

        let cmds = Processor {
            paths: vec![tmpdir.to_string_lossy().into_owned()],
            bi: Builtin::new(),
            jobs: Jobs::new(),
        };
        let result = cmds.is_executable_command("noexecfile");
        let _ = fs::remove_dir_all(&tmpdir);

        assert!(result.is_none());
    }
}

//! Built-in shell commands (`cd`, `echo`, `exit`, `pwd`, `type`, `complete`, `jobs`).

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::command::PipelineCommand;
use crate::utils::find_in_paths;

/// Thread-safe map from command name to the path of its completion program.
pub(crate) type SharedCompletions = Arc<Mutex<HashMap<String, String>>>;

fn write_out(stdout: Option<File>, line: &str) {
    match stdout {
        Some(mut f) => writeln!(f, "{}", line).ok(),
        None => writeln!(std::io::stdout(), "{}", line).ok(),
    };
}

fn write_err(stderr: Option<File>, line: &str) {
    match stderr {
        Some(mut f) => writeln!(f, "{}", line).ok(),
        None => writeln!(std::io::stderr(), "{}", line).ok(),
    };
}

/// Builtin Echo command
pub(crate) struct EchoCommand {
    pub(crate) args: Vec<String>,
}

impl PipelineCommand for EchoCommand {
    fn execute(
        self: Box<Self>,
        _stdin: Option<File>,
        stdout: Option<File>,
        _stderr: Option<File>,
    ) -> i32 {
        write_out(stdout, &self.args.join(" "));
        0
    }
}
/// Builtin Pwd Command
pub(crate) struct PwdCommand;

impl PipelineCommand for PwdCommand {
    fn execute(
        self: Box<Self>,
        _stdin: Option<File>,
        stdout: Option<File>,
        stderr: Option<File>,
    ) -> i32 {
        match env::current_dir() {
            Ok(p) => write_out(stdout, &p.display().to_string()),
            Err(e) => write_err(stderr, &format!("pwd: {e}")),
        };

        0
    }
}

/// Builtin `type` command: reports whether each argument is a builtin, a PATH executable, or unknown.
pub(crate) struct TypeCommand {
    pub(crate) args: Vec<String>,
    pub(crate) paths: Vec<String>,
}

impl PipelineCommand for TypeCommand {
    fn execute(
        self: Box<Self>,
        _stdin: Option<File>,
        stdout: Option<File>,
        _stderr: Option<File>,
    ) -> i32 {
        // stdout would be consumed after the first write, so make a &mut File before processing
        let mut out: Box<dyn Write> = match stdout {
            Some(f) => Box::new(f),
            None => Box::new(std::io::stdout()),
        };

        for arg in self.args {
            let line = if Builtin::is_builtin(&arg) {
                format!("{} is a shell builtin", arg)
            } else if let Some(path) = find_in_paths(&arg, &self.paths) {
                format!("{} is {}", arg, path)
            } else {
                format!("{}: not found", arg)
            };

            let _ = writeln!(out, "{}", &line);
        }

        0
    }
}

/// Builtin Completions command
pub(crate) struct CompleteCommand {
    pub(crate) args: Vec<String>,
    pub(crate) completions: SharedCompletions,
}

impl PipelineCommand for CompleteCommand {
    fn execute(
        self: Box<Self>,
        _stdin: Option<File>,
        stdout: Option<File>,
        _stderr: Option<File>,
    ) -> i32 {
        let mut out: Box<dyn Write> = match stdout {
            Some(f) => Box::new(f),
            None => Box::new(std::io::stdout()),
        };

        if self.args.is_empty() {
            writeln!(out).ok();
            return 0;
        }

        let mut completions = self.completions.lock().unwrap();

        match self.args[0].as_str() {
            "-C" => {
                if self.args.len() != 3 {
                    writeln!(out).ok();
                    return 0;
                }

                completions.insert(self.args[2].to_owned(), self.args[1].to_owned());
                return 0;
            }
            "-p" => {
                if self.args.len() != 2 {
                    writeln!(out).ok();
                    return 0;
                }

                match completions.get(&self.args[1]) {
                    Some(v) => writeln!(out, "complete -C '{}' {}", v, self.args[1]).ok(),
                    None => writeln!(
                        out,
                        "complete: {}: no completion specification",
                        self.args[1]
                    )
                    .ok(),
                };
            }
            "-r" => {
                if self.args.len() != 2 {
                    writeln!(out).ok();
                    return 0;
                }
                completions.remove(&self.args[1]);
            }
            _ => {
                writeln!(out).ok();
                return 0;
            }
        }

        0
    }
}

/// Builtin Unknown command
pub(crate) struct UnknownCommand {
    pub(crate) cmd: String,
}

impl PipelineCommand for UnknownCommand {
    fn execute(
        self: Box<Self>,
        _stdin: Option<File>,
        _stdout: Option<File>,
        stderr: Option<File>,
    ) -> i32 {
        write_err(stderr, &format!("{}: command not found", self.cmd));
        0
    }
}

/// Strips the ` (os error N)` suffix that Rust appends to OS error messages,
/// producing the bare POSIX description bash would print.
fn strip_os_error_suffix(e: &std::io::Error) -> String {
    let msg = e.to_string();
    msg.split(" (os error").next().unwrap_or(&msg).to_string()
}

pub(crate) struct Builtin {
    completions: SharedCompletions,
}

impl Builtin {
    /// Creates a `Builtin` with an empty completions map.
    pub(crate) fn new() -> Self {
        Self::with_completions(Arc::new(Mutex::new(HashMap::new())))
    }

    /// Creates a `Builtin` sharing the given completions map.
    pub(crate) fn with_completions(completions: SharedCompletions) -> Self {
        Self { completions }
    }

    /// Returns a clone of the shared completions handle.
    pub(crate) fn completions(&self) -> SharedCompletions {
        Arc::clone(&self.completions)
    }

    /// Returns `true` if `cmd` is the name of a shell builtin.
    pub(crate) fn is_builtin(cmd: &str) -> bool {
        Builtin::builtin_cmds().contains(&cmd)
    }

    /// Returns the list of names recognised as shell builtins.
    pub(crate) fn builtin_cmds() -> Vec<&'static str> {
        vec![
            "cd", "complete", "declare", "echo", "exit", "history", "jobs", "pwd", "type",
        ]
    }

    /// Changes the current working directory to `args`.
    ///
    /// An empty string or `"~"` navigates to `$HOME`. Prints an error to stderr and returns 1 on
    /// failure; returns 0 on success.
    pub(crate) fn cd(args: &str) -> i32 {
        let path = match args {
            "" | "~" => match env::home_dir() {
                Some(home) => home,
                None => {
                    eprintln!("cd: HOME not set");
                    return 1;
                }
            },
            p => PathBuf::from(p),
        };

        match env::set_current_dir(&path) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("cd: {}: {}", path.display(), strip_os_error_suffix(&e));
                1
            }
        }
    }

    /// Terminates the shell process with exit code 0.
    pub(crate) fn exit() -> i32 {
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::PipelineCommand;
    use std::collections::HashMap;
    use std::env;
    use std::fs::File;
    use std::io::Read;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn tmp_path(tag: &str) -> std::path::PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        env::temp_dir().join(format!("builtin_{}_{}_{}", tag, std::process::id(), n))
    }

    /// Runs `cmd` capturing its stdout into a `String` via a temp file.
    fn capture_stdout(cmd: Box<dyn PipelineCommand>) -> (i32, String) {
        let path = tmp_path("stdout");
        let write_file = File::create(&path).unwrap();
        let exit_code = cmd.execute(None, Some(write_file), None);
        let mut buf = String::new();
        File::open(&path).unwrap().read_to_string(&mut buf).unwrap();
        std::fs::remove_file(&path).ok();
        (exit_code, buf)
    }

    /// Runs `cmd` capturing its stderr into a `String` via a temp file.
    fn capture_stderr(cmd: Box<dyn PipelineCommand>) -> (i32, String) {
        let path = tmp_path("stderr");
        let write_file = File::create(&path).unwrap();
        let exit_code = cmd.execute(None, None, Some(write_file));
        let mut buf = String::new();
        File::open(&path).unwrap().read_to_string(&mut buf).unwrap();
        std::fs::remove_file(&path).ok();
        (exit_code, buf)
    }

    fn fresh_completions() -> SharedCompletions {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn test_list_commands() {
        assert_eq!(
            Builtin::builtin_cmds(),
            vec![
                "cd", "complete", "declare", "echo", "exit", "history", "jobs", "pwd", "type"
            ]
        );
    }

    #[test]
    fn test_echo_no_args_returns_zero() {
        let (code, output) = capture_stdout(Box::new(EchoCommand { args: vec![] }));
        assert_eq!(code, 0);
        assert_eq!(output, "\n");
    }

    #[test]
    fn test_echo_with_args_returns_zero() {
        let (code, output) = capture_stdout(Box::new(EchoCommand {
            args: vec!["hello".to_string(), "world".to_string()],
        }));
        assert_eq!(code, 0);
        assert_eq!(output, "hello world\n");
    }

    #[test]
    fn test_pwd_returns_zero() {
        let (code, _) = capture_stdout(Box::new(PwdCommand {}));
        assert_eq!(code, 0);
    }

    #[test]
    fn test_pwd_outputs_current_directory() {
        let cwd = env::current_dir().unwrap();
        let (code, output) = capture_stdout(Box::new(PwdCommand {}));
        assert_eq!(code, 0);
        assert_eq!(output.trim(), cwd.to_string_lossy());
    }

    #[test]
    fn test_unknown_returns_zero() {
        let (code, _) = capture_stderr(Box::new(UnknownCommand {
            cmd: "nope".to_string(),
        }));
        assert_eq!(code, 0);
    }

    #[test]
    fn test_unknown_command_error_message() {
        let (code, stderr) = capture_stderr(Box::new(UnknownCommand {
            cmd: "foobar".to_string(),
        }));
        assert_eq!(code, 0);
        assert_eq!(stderr.trim(), "foobar: command not found");
    }

    #[test]
    fn test_cd_invalid_path_returns_one() {
        assert_eq!(Builtin::cd("/this/path/does/not/exist/xyz_shell_test"), 1);
    }

    #[test]
    fn test_cd_valid_path_returns_zero() {
        let orig = env::current_dir().unwrap();
        let code = Builtin::cd("/tmp");
        let _ = env::set_current_dir(&orig);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_cd_changes_working_directory() {
        let orig = env::current_dir().unwrap();
        Builtin::cd("/tmp");
        let cwd = env::current_dir().unwrap();
        let _ = env::set_current_dir(&orig);
        assert_eq!(cwd.to_string_lossy(), "/tmp");
    }

    // --- strip_os_error_suffix ---

    #[test]
    fn test_strip_os_error_removes_suffix() {
        // errno 2 = ENOENT on Linux; Rust formats it as "No such file or directory (os error 2)"
        let e = std::io::Error::from_raw_os_error(2);
        let msg = strip_os_error_suffix(&e);
        assert_eq!(msg, "No such file or directory");
    }

    #[test]
    fn test_strip_os_error_no_suffix_unchanged() {
        let e = std::io::Error::other("something went wrong");
        assert_eq!(strip_os_error_suffix(&e), "something went wrong");
    }

    #[test]
    fn test_strip_os_error_permission_denied() {
        // errno 13 = EACCES; verify a second OS error is also stripped correctly
        let e = std::io::Error::from_raw_os_error(13);
        let msg = strip_os_error_suffix(&e);
        assert_eq!(msg, "Permission denied");
    }

    #[test]
    fn test_check_type_builtin_returns_zero() {
        let (code, output) = capture_stdout(Box::new(TypeCommand {
            args: vec!["echo".to_string()],
            paths: vec![],
        }));
        assert_eq!(code, 0);
        assert_eq!(output, "echo is a shell builtin\n");
    }

    #[test]
    fn test_check_type_with_path_returns_zero() {
        let (code, output) = capture_stdout(Box::new(TypeCommand {
            args: vec!["ls".to_string()],
            paths: vec!["/usr/bin".to_string()],
        }));
        assert_eq!(code, 0);
        assert!(output.contains("ls is"));
    }

    #[test]
    fn test_check_type_unknown_returns_zero() {
        let (code, output) = capture_stdout(Box::new(TypeCommand {
            args: vec!["notabuiltin_xyz".to_string()],
            paths: vec![],
        }));
        assert_eq!(code, 0);
        assert_eq!(output, "notabuiltin_xyz: not found\n");
    }

    #[test]
    fn test_check_type_builtin_not_classified_as_path() {
        // A builtin must be reported as a builtin even when a matching PATH entry exists.
        let (code, output) = capture_stdout(Box::new(TypeCommand {
            args: vec!["pwd".to_string()],
            paths: vec!["/usr/bin".to_string()],
        }));
        assert_eq!(code, 0);
        assert_eq!(output, "pwd is a shell builtin\n");
    }

    #[test]
    fn test_completion_empty_args_outputs_newline() {
        let (code, output) = capture_stdout(Box::new(CompleteCommand {
            args: vec![],
            completions: fresh_completions(),
        }));
        assert_eq!(code, 0);
        assert_eq!(output, "\n");
    }

    #[test]
    fn test_completion_unknown_flag_outputs_newline() {
        let (code, output) = capture_stdout(Box::new(CompleteCommand {
            args: vec!["one_arg".to_string()],
            completions: fresh_completions(),
        }));
        assert_eq!(code, 0);
        assert_eq!(output, "\n");
    }

    #[test]
    fn test_completion_p_no_spec_returns_message() {
        let (code, output) = capture_stdout(Box::new(CompleteCommand {
            args: vec!["-p".to_string(), "git".to_string()],
            completions: fresh_completions(),
        }));
        assert_eq!(code, 0);
        assert_eq!(output, "complete: git: no completion specification\n");
    }

    #[test]
    fn test_completion_set_and_get() {
        let completions = fresh_completions();

        let (set_code, set_out) = capture_stdout(Box::new(CompleteCommand {
            args: vec![
                "-C".to_string(),
                "/path/to/git/completer".to_string(),
                "git".to_string(),
            ],
            completions: completions.clone(),
        }));
        assert_eq!(set_code, 0);
        assert_eq!(set_out, "");

        let (get_code, get_out) = capture_stdout(Box::new(CompleteCommand {
            args: vec!["-p".to_string(), "git".to_string()],
            completions: completions.clone(),
        }));
        assert_eq!(get_code, 0);
        assert_eq!(get_out, "complete -C '/path/to/git/completer' git\n");
    }

    #[test]
    fn test_completion_set_remove_and_get() {
        let completions = fresh_completions();

        capture_stdout(Box::new(CompleteCommand {
            args: vec![
                "-C".to_string(),
                "/path/to/git/completer".to_string(),
                "git".to_string(),
            ],
            completions: completions.clone(),
        }));

        let (_, out) = capture_stdout(Box::new(CompleteCommand {
            args: vec!["-p".to_string(), "git".to_string()],
            completions: completions.clone(),
        }));
        assert_eq!(out, "complete -C '/path/to/git/completer' git\n");

        capture_stdout(Box::new(CompleteCommand {
            args: vec!["-r".to_string(), "git".to_string()],
            completions: completions.clone(),
        }));

        let (_, out) = capture_stdout(Box::new(CompleteCommand {
            args: vec!["-p".to_string(), "git".to_string()],
            completions: completions.clone(),
        }));
        assert_eq!(out, "complete: git: no completion specification\n");
    }
}

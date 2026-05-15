use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::exit;
use std::sync::{Arc, Mutex};

pub(crate) type SharedCompletions = Arc<Mutex<HashMap<String, String>>>;

pub(crate) struct Builtin {
    completions: SharedCompletions,
}

impl Builtin {
    pub(crate) fn new() -> Self {
        Self::with_completions(Arc::new(Mutex::new(HashMap::new())))
    }

    pub(crate) fn with_completions(completions: SharedCompletions) -> Self {
        Self { completions }
    }

    pub(crate) fn completions(&self) -> SharedCompletions {
        Arc::clone(&self.completions)
    }

    /// Changes the current working directory.
    ///
    /// An empty string or `"~"` navigates to the user's home directory.
    /// Returns `0` on success, `1` if the path does not exist or `HOME` is unset.
    pub(crate) fn cd(&self, args: &str) -> i32 {
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
            Err(_) => {
                println!("cd: {}: No such file or directory", path.display());
                1
            }
        }
    }

    /// Prints a completion error message for `args[1]` if `args` has the correct format; otherwise,
    /// prints a blank line. Always returns `0`.
    pub(crate) fn complete<W: Write>(&self, args: &[String], out: &mut W) -> i32 {
        if args.is_empty() {
            writeln!(out).ok();
            return 0;
        }

        let mut completions = self.completions.lock().unwrap();

        match args[0].as_str() {
            "-p" => {
                if args.len() != 2 {
                    writeln!(out).ok();
                    return 0;
                }

                match completions.get(&args[1]) {
                    Some(v) => writeln!(out, "{}", v).ok(),
                    None => {
                        writeln!(out, "complete: {}: no completion specification", args[1]).ok()
                    }
                };
            }
            "-C" => {
                if args.len() != 3 {
                    writeln!(out).ok();
                    return 0;
                }

                completions.insert(args[2].to_owned(), args[1].to_owned());
                return 0;
            }
            _ => {
                writeln!(out).ok();
                return 0;
            }
        }

        0
    }

    /// Prints `args` joined by a single space followed by a newline. Always returns `0`.
    pub(crate) fn echo<W: Write>(&self, args: Vec<&str>, out: &mut W) -> i32 {
        writeln!(out, "{}", args.join(" ")).ok();

        0
    }

    /// Terminates the process with exit code `0`. Does not return.
    pub(crate) fn exit() -> i32 {
        exit(0);
    }

    /// Reports whether `type_arg` is a shell builtin, an external command, or unknown.
    ///
    /// Pass the resolved executable path in `arg_path` when the command was found in `PATH`;
    /// pass `None` if it was not found. Always returns `0`.
    pub(crate) fn check_type(&self, type_arg: &str, arg_path: Option<String>) -> i32 {
        if Builtin::builtin_cmds().contains(&type_arg) {
            println!("{} is a shell builtin", type_arg);
        } else if let Some(path) = arg_path {
            println!("{} is {}", type_arg, path);
        } else {
            println!("{}: not found", type_arg);
        }

        0
    }

    /// Prints the current working directory to stdout. Always returns `0`.
    pub(crate) fn pwd(&self) -> i32 {
        match env::current_dir() {
            Ok(path) => println!("{}", path.display()),
            Err(e) => eprintln!("Error getting current directory: {}", e),
        }

        0
    }

    /// Prints a "command not found" message for `cmd`. Always returns `0`.
    pub(crate) fn unknown(&self, cmd: &str) -> i32 {
        println!("{}: command not found", cmd);

        0
    }

    /// Returns the list of names recognised as shell builtins.
    pub(crate) fn builtin_cmds() -> Vec<&'static str> {
        vec!["cd", "complete", "echo", "exit", "pwd", "type"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_commands() {
        let builtins = Builtin::builtin_cmds();
        assert_eq!(
            builtins,
            vec!["cd", "complete", "echo", "exit", "pwd", "type"]
        )
    }

    #[test]
    fn test_echo_no_args_returns_zero() {
        let bi = Builtin::new();
        let mut out = vec![];
        assert_eq!(bi.echo(vec![], &mut out), 0);
        assert_eq!(String::from_utf8(out).unwrap(), "\n");
    }

    #[test]
    fn test_echo_with_args_returns_zero() {
        let bi = Builtin::new();
        let mut out = vec![];
        assert_eq!(bi.echo(vec!["hello", "world"], &mut out), 0);
        assert_eq!(String::from_utf8(out).unwrap(), "hello world\n");
    }

    #[test]
    fn test_pwd_returns_zero() {
        let bi = Builtin::new();
        assert_eq!(bi.pwd(), 0);
    }

    #[test]
    fn test_unknown_returns_zero() {
        let bi = Builtin::new();
        assert_eq!(bi.unknown("nope"), 0);
    }

    #[test]
    fn test_cd_invalid_path_returns_one() {
        let bi = Builtin::new();
        assert_eq!(bi.cd("/this/path/does/not/exist/xyz_shell_test"), 1);
    }

    #[test]
    fn test_cd_valid_path_returns_zero() {
        let bi = Builtin::new();
        let orig = env::current_dir().unwrap();
        let result = bi.cd("/tmp");
        // Restore regardless of outcome so we don't pollute other tests.
        let _ = env::set_current_dir(&orig);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_check_type_builtin_returns_zero() {
        let bi = Builtin::new();
        assert_eq!(bi.check_type("echo", None), 0);
    }

    #[test]
    fn test_check_type_with_path_returns_zero() {
        let bi = Builtin::new();
        assert_eq!(bi.check_type("ls", Some("/usr/bin/ls".to_string())), 0);
    }

    #[test]
    fn test_check_type_unknown_returns_zero() {
        let bi = Builtin::new();
        assert_eq!(bi.check_type("notabuiltin_xyz", None), 0);
    }

    #[test]
    fn test_check_type_builtin_not_classified_as_path() {
        let bi = Builtin::new();
        // A builtin should be reported as builtin even when a path is provided.
        assert_eq!(bi.check_type("pwd", Some("/usr/bin/pwd".to_string())), 0);
    }

    #[test]
    fn test_completion_returns_bad_arg_count() {
        let mut out = vec![];
        assert_eq!(Builtin::new().complete(&[], &mut out), 0);
        assert_eq!(String::from_utf8(out).unwrap(), "\n");

        let mut out = vec![];
        assert_eq!(
            Builtin::new().complete(&["one_arg".to_string()], &mut out),
            0
        );
        assert_eq!(String::from_utf8(out).unwrap(), "\n");
    }
    #[test]
    fn test_completion_returns_correct_args() {
        let mut out = vec![];
        assert_eq!(
            Builtin::new().complete(&["-p".to_string(), "git".to_string()], &mut out),
            0
        );

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "complete: git: no completion specification\n"
        );
    }

    #[test]
    fn test_completion_returns_correct_arg_count_bad_completion() {
        let mut out = vec![];
        assert_eq!(
            Builtin::new().complete(&["-p".to_string(), "not_a_command".to_string()], &mut out),
            0
        );

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "complete: not_a_command: no completion specification\n"
        );
    }

    #[test]
    fn test_completion_set_a_completion() {
        let bi = Builtin::new();
        let mut out = vec![];
        assert_eq!(
            bi.complete(
                &[
                    "-C".to_string(),
                    "/path/to/git/completer".to_string(),
                    "git".to_string()
                ],
                &mut out
            ),
            0
        );

        assert_eq!(String::from_utf8(out).unwrap(), "");

        let mut out = vec![];
        assert_eq!(
            bi.complete(&["-p".to_string(), "git".to_string()], &mut out),
            0
        );

        assert_eq!(String::from_utf8(out).unwrap(), "/path/to/git/completer\n");
    }
}

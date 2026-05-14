use std::env;
use std::path::PathBuf;
use std::process::exit;

pub(crate) struct Builtin {}

impl Builtin {
    /// Changes the current working directory.
    ///
    /// An empty string or `"~"` navigates to the user's home directory.
    /// Returns `0` on success, `1` if the path does not exist or `HOME` is unset.
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
            Err(_) => {
                println!("cd: {}: No such file or directory", path.display());
                1
            }
        }
    }

    /// Prints `args` joined by a single space followed by a newline. Always returns `0`.
    pub(crate) fn echo(args: Vec<&str>) -> i32 {
        let args = args.join(" ");

        println!("{}", args);

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
    pub(crate) fn check_type(type_arg: &str, arg_path: Option<String>) -> i32 {
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
    pub(crate) fn pwd() -> i32 {
        match env::current_dir() {
            Ok(path) => println!("{}", path.display()),
            Err(e) => eprintln!("Error getting current directory: {}", e),
        }

        0
    }

    /// Prints a "command not found" message for `cmd`. Always returns `0`.
    pub(crate) fn unknown(cmd: &str) -> i32 {
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
        assert_eq!(Builtin::echo(vec![]), 0);
    }

    #[test]
    fn test_echo_with_args_returns_zero() {
        assert_eq!(Builtin::echo(vec!["hello", "world"]), 0);
    }

    #[test]
    fn test_pwd_returns_zero() {
        assert_eq!(Builtin::pwd(), 0);
    }

    #[test]
    fn test_unknown_returns_zero() {
        assert_eq!(Builtin::unknown("nope"), 0);
    }

    #[test]
    fn test_cd_invalid_path_returns_one() {
        assert_eq!(Builtin::cd("/this/path/does/not/exist/xyz_shell_test"), 1);
    }

    #[test]
    fn test_cd_valid_path_returns_zero() {
        let orig = env::current_dir().unwrap();
        let result = Builtin::cd("/tmp");
        // Restore regardless of outcome so we don't pollute other tests.
        let _ = env::set_current_dir(&orig);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_check_type_builtin_returns_zero() {
        assert_eq!(Builtin::check_type("echo", None), 0);
    }

    #[test]
    fn test_check_type_with_path_returns_zero() {
        assert_eq!(
            Builtin::check_type("ls", Some("/usr/bin/ls".to_string())),
            0
        );
    }

    #[test]
    fn test_check_type_unknown_returns_zero() {
        assert_eq!(Builtin::check_type("notabuiltin_xyz", None), 0);
    }

    #[test]
    fn test_check_type_builtin_not_classified_as_path() {
        // A builtin should be reported as builtin even when a path is provided.
        assert_eq!(
            Builtin::check_type("pwd", Some("/usr/bin/pwd".to_string())),
            0
        );
    }
}

use anyhow::anyhow;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

use crate::builtin::Builtin;

pub(crate) struct Commands {
    paths: Vec<String>,
}

enum ParserState {
    Normal,
    SingleQuote,
    DoubleQuote,
}

impl Commands {
    pub(crate) fn new() -> Self {
        let paths = match env::var("PATH") {
            Ok(path_var) => env::split_paths(&path_var)
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            Err(_) => vec![],
        };

        Self { paths }
    }

    fn execute_command(&self, cmd: &str, args: Vec<String>) -> i32 {
        let results = Command::new(cmd).args(args.iter()).output();

        match results {
            Ok(output) => {
                print!("{}", String::from_utf8_lossy(&output.stdout));
                output.status.code().unwrap_or(0)
            }
            Err(e) => {
                println!("Error: {}", e);

                1
            }
        }
    }

    fn is_executalble_command(&self, cmd: &str) -> Option<String> {
        for path in &self.paths {
            let cmd_path = Path::new(path).join(cmd);

            if cmd_path.exists() {
                match fs::metadata(&cmd_path) {
                    Ok(metadata) => {
                        if metadata.permissions().mode() & 0o111 != 0 {
                            return Some(cmd_path.to_string_lossy().into_owned());
                        }
                    }
                    Err(e) => eprintln!("{e}"),
                }
            }
        }

        None
    }

    fn parse_cmd(input: &str) -> Result<(&str, Vec<String>), anyhow::Error> {
        let input = input.trim();

        if input.is_empty() {
            return Err(anyhow!("command is empty"));
        }

        if let Some((cmd, args)) = input.split_once(' ') {
            Ok((cmd, Commands::parse_args(args)))
        } else {
            Ok((input, vec![]))
        }
    }

    fn parse_args(args_str: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut state = ParserState::Normal;

        for c in args_str.chars() {
            match c {
                '\'' => match state {
                    ParserState::Normal => state = ParserState::SingleQuote,
                    ParserState::SingleQuote => state = ParserState::Normal,
                    ParserState::DoubleQuote => current.push('\''),
                },
                '\"' => match state {
                    ParserState::Normal => state = ParserState::DoubleQuote,
                    ParserState::SingleQuote => current.push('\"'),
                    ParserState::DoubleQuote => state = ParserState::Normal,
                },
                ' ' => match state {
                    ParserState::Normal => {
                        if !current.is_empty() {
                            args.push(current.split_off(0));
                        }
                    }
                    _ => current.push(' '),
                },
                other => current.push(other),
            }
        }

        if !current.is_empty() {
            args.push(current);
        }

        args
    }

    pub(crate) fn process_command(&self, input: &str) {
        match Commands::parse_cmd(input) {
            Ok((cmd, args)) => {
                match cmd {
                    "cd" => {
                        args.iter().for_each(|p| {
                            let _ = Builtin::cd(p);
                        });

                        0
                    }
                    "echo" => Builtin::echo(args.iter().map(|s| s.as_str()).collect()),
                    "exit" => Builtin::exit(),
                    "pwd" => Builtin::pwd(),
                    "type" => {
                        for arg in args.iter() {
                            Builtin::check_type(arg, self.is_executalble_command(arg));
                        }

                        0
                    }
                    _ => match self.is_executalble_command(cmd) {
                        Some(_path) => self.execute_command(cmd, args),
                        None => Builtin::unknown(cmd),
                    },
                };
            }
            Err(_) => println!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_parse_empty() {
        assert!(Commands::parse_cmd("").is_err());
    }

    #[test]
    fn test_cmd_only_parse() {
        let (cmd, args) = Commands::parse_cmd("foo").unwrap();
        assert_eq!(cmd, "foo");
        assert_eq!(args, Vec::<&str>::new());
    }

    #[test]
    fn test_simple_parse() {
        let (cmd, args) = Commands::parse_cmd("foo bar").unwrap();
        assert_eq!(cmd, "foo");
        assert_eq!(args, vec!["bar"]);
    }

    #[test]
    fn test_long_parse() {
        let (cmd, args) = Commands::parse_cmd("foo bar baz bop").unwrap();
        assert_eq!(cmd, "foo");
        assert_eq!(args, vec!["bar", "baz", "bop"]);
    }

    // Leading space: splits into ("", Some("foo")) — cmd is empty string, not an error.
    #[test]
    fn test_parse_leading_space() {
        let (cmd, args) = Commands::parse_cmd(" foo").unwrap();
        assert_eq!(cmd, "foo");
        assert_eq!(args, Vec::<&str>::new());
    }

    // A single space is not an empty string so parse_cmd succeeds.
    #[test]
    fn test_parse_single_space_not_error() {
        assert!(Commands::parse_cmd(" ").is_err());
    }

    // --- Parse args ---
    // parse_args is just split_whitespace, so we can test that directly.
    #[test]
    fn test_parse_args() {
        let args = Commands::parse_args("  arg1   arg2  arg3  ");
        assert_eq!(args, vec!["arg1", "arg2", "arg3"]);
    }

    #[test]
    fn test_parse_args_with_single_quotes() {
        let args = Commands::parse_args("'arg1   arg2'");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_single_quotes() {
        let args = Commands::parse_args("'arg1''arg2'");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_empty_single_quotes() {
        let args = Commands::parse_args("arg1''arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quotes() {
        let args = Commands::parse_args("\"arg1   arg2\"");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_double_quotes() {
        let args = Commands::parse_args("\"arg1\"\"arg2\"");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_unquoted() {
        let args = Commands::parse_args("\"arg1\"arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_separate_double_quotes() {
        let args = Commands::parse_args("\"arg1\" \"arg2\"");
        assert_eq!(args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_inner_single_quote() {
        let args = Commands::parse_args("\"arg1's arg2\"");
        assert_eq!(args, vec!["arg1's arg2"]);
    }

    // --- Commands::new ---

    #[test]
    fn test_new_reads_path_entries() {
        let orig = env::var("PATH").unwrap_or_default();
        unsafe { env::set_var("PATH", "/usr/bin:/bin") };
        let cmds = Commands::new();
        unsafe { env::set_var("PATH", &orig) };
        assert!(cmds.paths.contains(&"/usr/bin".to_string()));
        assert!(cmds.paths.contains(&"/bin".to_string()));
    }

    #[test]
    fn test_new_empty_when_path_unset() {
        let orig = env::var("PATH").ok();
        unsafe { env::remove_var("PATH") };
        let cmds = Commands::new();
        if let Some(p) = orig {
            unsafe { env::set_var("PATH", p) };
        }
        assert!(cmds.paths.is_empty());
    }

    // --- Commands::is_executalble_command ---

    #[test]
    fn test_is_executable_not_found_empty_paths() {
        let cmds = Commands { paths: vec![] };
        assert!(cmds.is_executalble_command("ls").is_none());
    }

    #[test]
    fn test_is_executable_not_found_wrong_path() {
        let cmds = Commands {
            paths: vec!["/nonexistent/path/xyz_shell_test".to_string()],
        };
        assert!(cmds.is_executalble_command("ls").is_none());
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

        let cmds = Commands {
            paths: vec![tmpdir.to_string_lossy().into_owned()],
        };
        let result = cmds.is_executalble_command("myfakeshellcmd");
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

        let cmds = Commands {
            paths: vec![tmpdir.to_string_lossy().into_owned()],
        };
        let result = cmds.is_executalble_command("noexecfile");
        let _ = fs::remove_dir_all(&tmpdir);

        assert!(result.is_none());
    }

    // --- Commands::process_command ---

    #[test]
    fn test_process_command_empty_does_not_panic() {
        let cmds = Commands { paths: vec![] };
        cmds.process_command("");
    }

    #[test]
    fn test_process_command_unknown_does_not_panic() {
        let cmds = Commands { paths: vec![] };
        cmds.process_command("thisdoesnotexist_xyz");
    }

    #[test]
    fn test_process_command_echo_does_not_panic() {
        let cmds = Commands { paths: vec![] };
        cmds.process_command("echo hello");
    }
}

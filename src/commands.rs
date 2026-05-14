use anyhow::anyhow;
use std::io::stdout;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::builtin::Builtin;
use crate::redirect::{Redirect, RedirectType};
use crate::utils::{get_paths, is_executable};

/// Resolves and executes shell commands against the entries in `PATH`.
pub(crate) struct Commands {
    paths: Vec<String>,
}

enum ParserState {
    /// Unquoted text; spaces split args and backslash escapes the next char.
    Normal,
    /// Backslash-single-quote region (`\'...\' `): content is literal, spaces don't split,
    /// `\'` closes (emitting `'`), `\"` emits `"`.
    BSQOpen,
    /// Single-quoted region (`'...'`): everything literal until the closing `'`.
    SingleQuote,
    /// Double-quoted region (`"..."`): backslash still escapes inside.
    DoubleQuote,
    /// After a `\` outside a single-quote: the next character is taken literally.
    Escaped,
}

impl Commands {
    /// Creates a new `Commands` instance by reading the `PATH` environment variable.
    pub(crate) fn new() -> Self {
        let paths = get_paths();
        Self { paths }
    }

    /// Runs `cmd` with `args` as a child process, prints its stdout, and returns its exit code.
    fn execute_command(&self, cmd: &str, args: Vec<String>) -> i32 {
        let redirect = match Redirect::has_redirect(&args) {
            Ok(redirect) => redirect,
            Err(e) => {
                println!("{e}");
                return -1;
            }
        };

        match redirect.redirect_type {
            RedirectType::None => match Command::new(cmd).args(args.iter()).output() {
                Ok(output) => {
                    print!("{}", String::from_utf8_lossy(&output.stdout));
                    output.status.code().unwrap_or(0)
                }
                Err(e) => {
                    println!("Error: {}", e);
                    1
                }
            },
            RedirectType::StdOut => {
                let output_file = match redirect.get_redirect_file() {
                    Ok(f) => f,
                    Err(e) => {
                        println!("Error: {}", e);
                        return 1;
                    }
                };

                match Command::new(cmd)
                    .args(&args[0..redirect.position])
                    .stdout(Stdio::from(output_file))
                    .spawn()
                {
                    Ok(mut child) => match child.wait() {
                        Ok(status) => status.code().unwrap_or(0),
                        Err(e) => {
                            println!("Error: {}", e);
                            1
                        }
                    },
                    Err(e) => {
                        println!("Error: {}", e);
                        1
                    }
                }
            }
            RedirectType::StdErr => {
                let output_file = match redirect.get_redirect_file() {
                    Ok(f) => f,
                    Err(e) => {
                        println!("Error: {}", e);
                        return 1;
                    }
                };

                match Command::new(cmd)
                    .args(&args[0..redirect.position])
                    .stderr(Stdio::from(output_file))
                    .spawn()
                {
                    Ok(mut child) => match child.wait() {
                        Ok(status) => status.code().unwrap_or(0),
                        Err(e) => {
                            println!("Error: {}", e);
                            1
                        }
                    },
                    Err(e) => {
                        println!("Error: {}", e);
                        1
                    }
                }
            }
        }
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
    fn parse_cmd(input: &str) -> Result<(String, Vec<String>), anyhow::Error> {
        let input = input.trim();

        if input.is_empty() {
            return Err(anyhow!("command is empty"));
        }

        let mut input_list = Commands::parse_input(input);

        let cmd = input_list.remove(0);

        Ok((cmd, input_list))
    }

    /// Parses the argument portion of a command line into a vector of strings,
    /// handling single quotes, double quotes, backslash-single-quote regions, and escape sequences.
    fn parse_input(args_str: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut state = ParserState::Normal;
        // Remembers whether Escaped was entered from Normal or DoubleQuote,
        // so we can return to the right state afterward.
        let mut prev_state = ParserState::Normal;

        let mut iter = args_str.chars().peekable();

        while let Some(c) = iter.next() {
            match c {
                '\'' => match state {
                    ParserState::Normal => state = ParserState::SingleQuote,
                    ParserState::SingleQuote => state = ParserState::Normal,
                    ParserState::DoubleQuote => current.push('\''), // literal inside "..."
                    ParserState::BSQOpen => current.push('\''),     // literal inside \'...\'
                    ParserState::Escaped => {
                        // \' — literal single quote, return to prior context
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push('\'');
                    }
                },
                '\"' => match state {
                    ParserState::Normal => state = ParserState::DoubleQuote,
                    ParserState::SingleQuote => current.push('\"'), // literal inside '...'
                    ParserState::DoubleQuote => state = ParserState::Normal,
                    ParserState::BSQOpen => current.push('\"'), // literal inside \'...\'
                    ParserState::Escaped => {
                        // \" — literal double quote, return to prior context
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push('\"');
                    }
                },
                ' ' => match state {
                    ParserState::Normal => {
                        // Unquoted space: flush current token
                        if !current.is_empty() {
                            args.push(current.split_off(0));
                        }
                    }
                    ParserState::Escaped => {
                        // \ followed by space: literal space
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push(' ');
                    }
                    // Inside any quote context: space is literal
                    _ => current.push(' '),
                },
                '\\' => match state {
                    ParserState::Escaped => {
                        // \\ — literal backslash
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push('\\');
                    }
                    // Backslash is literal inside single quotes
                    ParserState::SingleQuote => current.push('\\'),
                    ParserState::BSQOpen => {
                        // Peek to decide: \' closes the region, \" escapes the quote,
                        // anything else the backslash is literal.
                        if iter.peek() == Some(&'\'') {
                            iter.next();
                            current.push('\'');
                            state = ParserState::Normal;
                        } else if iter.peek() == Some(&'"') {
                            iter.next();
                            current.push('"');
                        } else {
                            current.push('\\');
                        }
                    }
                    // \' in Normal: open a BSQ region and emit the leading '
                    ParserState::Normal if iter.peek() == Some(&'\'') => {
                        iter.next();
                        current.push('\'');
                        state = ParserState::BSQOpen;
                    }
                    _ => {
                        // Normal or DoubleQuote: start an escape sequence
                        prev_state = state;
                        state = ParserState::Escaped;
                    }
                },
                other => match state {
                    ParserState::Escaped => {
                        // Any other escaped char is taken literally (backslash consumed)
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push(other);
                    }
                    _ => current.push(other),
                },
            }
        }

        if !current.is_empty() {
            args.push(current);
        }

        args
    }

    /// Parses and dispatches a full command line, routing to builtins or external executables.
    pub(crate) fn process_command(&self, input: &str) {
        match Commands::parse_cmd(input) {
            Ok((cmd, args)) => {
                match cmd.as_str() {
                    "cd" => {
                        args.iter().for_each(|p| {
                            let _ = Builtin::cd(p);
                        });

                        0
                    }
                    "complete" => Builtin::complete(&args, &mut stdout()),
                    "echo" => {
                        if let Some(cmd_path) = self.is_executable_command(&cmd) {
                            self.execute_command(&cmd_path, args);
                        } else {
                            Builtin::echo(args.iter().map(|s| s.as_str()).collect(), &mut stdout());
                        }

                        0
                    }
                    "exit" => Builtin::exit(),
                    "pwd" => Builtin::pwd(),
                    "type" => {
                        for arg in args.iter() {
                            Builtin::check_type(arg, self.is_executable_command(arg));
                        }

                        0
                    }
                    _ => match self.is_executable_command(&cmd) {
                        Some(_) => self.execute_command(&cmd, args),
                        None => Builtin::unknown(&cmd),
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
    use std::os::unix::fs::PermissionsExt;
    use std::{env, fs};

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

    // Command with single and double quotes
    #[test]
    fn test_parse_cmd_with_single_quotes() {
        let (cmd, args) = Commands::parse_cmd("'my command' arg1").unwrap();
        assert_eq!(cmd, "my command");
        assert_eq!(args, vec!["arg1"]);
    }

    #[test]
    fn test_parse_cmd_with_double_quotes() {
        let (cmd, args) = Commands::parse_cmd(r#""my command" arg1"#).unwrap();
        assert_eq!(cmd, "my command");
        assert_eq!(args, vec!["arg1"]);
    }

    #[test]
    fn test_parse_cmd_containing_single_quotes() {
        let (cmd, args) = Commands::parse_cmd(r#""my 'command'" arg1"#).unwrap();
        assert_eq!(cmd, r#"my 'command'"#);
        assert_eq!(args, vec!["arg1"]);
    }

    #[test]
    fn test_parse_cmd_containing_double_quotes() {
        let (cmd, args) = Commands::parse_cmd(r#"'my "command"' arg1"#).unwrap();
        assert_eq!(cmd, "my \"command\"");
        assert_eq!(args, vec!["arg1"]);
    }

    // A single space is not an empty string so parse_cmd should error out.
    #[test]
    fn test_parse_single_space_not_error() {
        assert!(Commands::parse_cmd(" ").is_err());
    }

    // --- Parse args ---
    #[test]
    fn test_parse_args() {
        let args = Commands::parse_input("  arg1   arg2  arg3  ");
        assert_eq!(args, vec!["arg1", "arg2", "arg3"]);
    }

    #[test]
    fn test_parse_args_with_single_quotes() {
        let args = Commands::parse_input("'arg1   arg2'");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_single_quotes() {
        let args = Commands::parse_input("'arg1''arg2'");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_empty_single_quotes() {
        let args = Commands::parse_input("arg1''arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quotes() {
        let args = Commands::parse_input("\"arg1   arg2\"");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_double_quotes() {
        let args = Commands::parse_input("\"arg1\"\"arg2\"");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_unquoted() {
        let args = Commands::parse_input("\"arg1\"arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_separate_double_quotes() {
        let args = Commands::parse_input("\"arg1\" \"arg2\"");
        assert_eq!(args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_inner_single_quote() {
        let args = Commands::parse_input("\"arg1's arg2\"");
        assert_eq!(args, vec!["arg1's arg2"]);
    }

    // Literal chars in arg
    #[test]
    fn test_parse_args_backslash_spaces() {
        let args = Commands::parse_input(r"arg1\ \ \ arg2");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_space_collapse_others() {
        let args = Commands::parse_input(r"arg1\     arg2");
        assert_eq!(args, vec!["arg1 ", "arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_char() {
        let args = Commands::parse_input(r"arg1\narg2");
        assert_eq!(args, vec!["arg1narg2"]);
    }

    #[test]
    fn test_parse_args_backslash_backslash() {
        let args = Commands::parse_input(r"arg1\\arg2");
        assert_eq!(args, vec![r"arg1\arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_single_quote() {
        let args = Commands::parse_input(r"\'arg1 arg2\'");
        assert_eq!(args, vec!["'arg1 arg2'"]);
    }

    // Support single quote sting literals
    #[test]
    fn test_parse_args_single_quote_with_multi_backslash() {
        let args = Commands::parse_input(r"'arg1\\\arg2'");
        assert_eq!(args, vec![r"arg1\\\arg2"]);
    }

    #[test]
    fn test_parse_args_single_quote_with_backslash_double_quote() {
        let args = Commands::parse_input("'arg1\"arg2'");
        assert_eq!(args, vec![r#"arg1"arg2"#]);
    }

    #[test]
    fn test_parse_args_backslash_single_quote_mixed() {
        let args = Commands::parse_input("'arg1\"arg2\"arg3'");
        assert_eq!(args, vec![r#"arg1"arg2"arg3"#]);
    }

    #[test]
    fn test_parse_args_escaped_single_and_double_quotes() {
        let args = Commands::parse_input(r#"\'\"arg1 arg2\"\'"#);
        assert_eq!(args, vec![r#"'"arg1 arg2"'"#]);
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
        assert!(cmds.is_executable_command("ls").is_none());
    }

    #[test]
    fn test_is_executable_not_found_wrong_path() {
        let cmds = Commands {
            paths: vec!["/nonexistent/path/xyz_shell_test".to_string()],
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

        let cmds = Commands {
            paths: vec![tmpdir.to_string_lossy().into_owned()],
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

        let cmds = Commands {
            paths: vec![tmpdir.to_string_lossy().into_owned()],
        };
        let result = cmds.is_executable_command("noexecfile");
        let _ = fs::remove_dir_all(&tmpdir);

        assert!(result.is_none());
    }
}

use anyhow::anyhow;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

use crate::builtin::Builtin;

pub(crate) struct Commands<'a> {
    commands: Vec<&'a str>,
    paths: Vec<String>,
}

impl<'a> Commands<'a> {
    pub(crate) fn new() -> Self {
        let commands = vec!["echo", "exit", "pwd", "type"];

        let paths = match env::var("PATH") {
            Ok(path_var) => env::split_paths(&path_var)
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            Err(_) => vec![],
        };

        Self { commands, paths }
    }

    fn execute_command(&self, cmd: &str, args: Option<&str>) -> i32 {
        let args_iter: Vec<&str> = args
            .map(|s| s.split_whitespace().collect())
            .unwrap_or_default();

        let results = Command::new(cmd).args(args_iter).output();

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

    fn list_builtin_commands(&self) -> Vec<&str> {
        self.commands.clone()
    }

    fn parse_cmd(input: &str) -> Result<(&str, Option<&str>), anyhow::Error> {
        if input.is_empty() {
            return Err(anyhow!("command is empty"));
        }

        if let Some((cmd, args)) = input.split_once(' ') {
            Ok((cmd, Some(args)))
        } else {
            Ok((input, None))
        }
    }

    pub(crate) fn process_command(&self, input: &str) {
        match Commands::parse_cmd(input) {
            Ok((cmd, args)) => {
                match cmd {
                    "echo" => Builtin::echo(args),
                    "exit" => Builtin::exit(),
                    "pwd" => Builtin::pwd(),
                    "type" => {
                        let arg_cmd = args.unwrap_or("");
                        Builtin::check_type(
                            arg_cmd,
                            self.list_builtin_commands(),
                            self.is_executalble_command(arg_cmd),
                        )
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

    #[test]
    fn test_parse_empty() {
        let input = "";
        let e = Commands::parse_cmd(input);
        assert!(e.is_err());
    }

    #[test]
    fn test_cmd_only_parse() {
        let input = "foo";
        if let Ok((cmd, args)) = Commands::parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_none());
        }
    }

    #[test]
    fn test_simple_parse() {
        let input = "foo bar";
        if let Ok((cmd, args)) = Commands::parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_some());
            assert_eq!(args, Some("bar"));
        }
    }

    #[test]
    fn test_long_parse() {
        let input = "foo bar baz bop";
        if let Ok((cmd, args)) = Commands::parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_some());
            assert_eq!(args, Some("bar baz bop"));
        }
    }

    #[test]
    fn test_list_commands() {
        let commands = Commands::new();
        let builtins = commands.list_builtin_commands();

        assert_eq!(builtins, vec!["echo", "exit", "type"])
    }
}

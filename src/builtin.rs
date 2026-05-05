use std::env;
use std::path::PathBuf;
use std::process::exit;

pub(crate) struct Builtin {}

impl Builtin {
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

    pub(crate) fn echo(args: Option<&str>) -> i32 {
        match args {
            Some(args) => println!("{}", args),
            _ => println!(),
        }

        0
    }

    pub(crate) fn exit() -> i32 {
        exit(0);
    }

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

    pub(crate) fn pwd() -> i32 {
        match env::current_dir() {
            Ok(path) => println!("{}", path.display()),
            Err(e) => eprintln!("Error getting current directory: {}", e),
        }

        0
    }

    pub(crate) fn unknown(cmd: &str) -> i32 {
        println!("{}: command not found", cmd);

        0
    }

    fn builtin_cmds() -> Vec<&'static str> {
        vec!["cd", "echo", "exit", "pwd", "type"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_commands() {
        let builtins = Builtin::builtin_cmds();

        assert_eq!(builtins, vec!["cd", "echo", "exit", "pwd", "type"])
    }
}

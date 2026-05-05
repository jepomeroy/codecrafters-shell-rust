use std::process::exit;

pub(crate) struct Builtin {}

impl Builtin {
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

    pub(crate) fn check_type(type_arg: &str, builtins: Vec<&str>, arg_path: Option<String>) -> i32 {
        if builtins.contains(&type_arg) {
            println!("{} is a shell builtin", type_arg);
        } else if let Some(path) = arg_path {
            println!("{} is {}", type_arg, path);
        } else {
            println!("{}: not found", type_arg);
        }

        0
    }

    pub(crate) fn unknown(cmd: &str) -> i32 {
        println!("{}: command not found", cmd);

        0
    }
}

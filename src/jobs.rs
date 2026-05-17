use std::process::Command;

pub(crate) struct Jobs {
    jobs: Vec<u32>,
}

impl Jobs {
    pub(crate) fn new() -> Self {
        Self { jobs: vec![] }
    }

    pub(crate) fn execute_background(&mut self, cmd: &str, args: Vec<String>) {
        if let Ok(child) = Command::new(cmd).args(args.iter()).spawn() {
            self.jobs.push(child.id());
            println!("[{}] {}", self.jobs.len(), child.id());
        } else {
            println!("ls command didn't start");
        }
    }

    pub(crate) fn is_background_job(args: &[String]) -> bool {
        if let Some(last) = args.last() {
            return last == "&";
        }

        false
    }
}


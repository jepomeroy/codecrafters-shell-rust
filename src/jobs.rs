use std::{fmt::Display, process::Command};

enum Status {
    Running,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let disp = match self {
            Status::Running => "Running",
        };

        write!(f, "{:>width$}", disp, width = disp.len() + 2)
    }
}

#[derive(PartialEq, Eq)]
enum JobPosition {
    Current,
    Prev,
    Untracked,
}

impl Display for JobPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pos = match self {
            JobPosition::Current => "+",
            JobPosition::Prev => "-",
            _ => " ",
        };

        write!(f, "{}", pos)
    }
}

struct Job {
    job_num: usize,
    job_pos: JobPosition,
    proc_id: u32,
    cmd: String,
    status: Status,
}

impl Job {
    fn new(cmd: &str, args: &[String], job_num: usize, proc_id: u32) -> Self {
        let command = format!("{} {}", cmd, args.join(" "));

        Self {
            job_num,
            job_pos: JobPosition::Current,
            proc_id,
            cmd: command,
            status: Status::Running,
        }
    }
}

impl Display for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}]{}{}{:>width_cmd$} &",
            self.job_num,
            self.job_pos,
            self.status,
            self.cmd,
            width_cmd = self.cmd.to_string().len() + 17,
        )
    }
}

pub(crate) struct Jobs {
    jobs: Vec<Job>,
}

impl Jobs {
    pub(crate) fn new() -> Self {
        Self { jobs: vec![] }
    }

    pub(crate) fn execute_background(&mut self, cmd: &str, args: Vec<String>) {
        if let Ok(child) = Command::new(cmd).args(args.iter()).spawn() {
            for j in &mut self.jobs {
                if j.job_pos == JobPosition::Current {
                    j.job_pos = JobPosition::Prev;
                } else {
                    j.job_pos = JobPosition::Untracked;
                };
            }

            let job_num = self.jobs.len() + 1;
            self.jobs.push(Job::new(cmd, &args, job_num, child.id()));
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

    pub(crate) fn print_jobs(&self) {
        for j in &self.jobs {
            println!("{}", j);
        }
    }
}


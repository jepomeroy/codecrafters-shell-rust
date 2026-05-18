use std::{
    fmt::Display,
    process::{Child, Command},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Complete,
    Done,
    Running,
    Unknown,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let disp = match self {
            Status::Complete => "Complete",
            Status::Done => "Done",
            Status::Running => "Running",
            Status::Unknown => "Unknown",
        };

        write!(f, "  {:<8}", disp)
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
    process: Child,
    cmd: String,
    status: Status,
}

impl Job {
    fn new(cmd: &str, args: &[String], job_num: usize, process: Child) -> Self {
        let command = format!("{} {}", cmd, args.join(" "));

        Self {
            job_num,
            job_pos: JobPosition::Current,
            process,
            cmd: command,
            status: Status::Running,
        }
    }

    fn check_status(&mut self) -> Status {
        match &self.process.try_wait() {
            Ok(Some(status)) => {
                if self.status == Status::Running {
                    self.status = Status::Done;
                } else {
                    self.status = Status::Complete;
                }
            }
            Ok(None) => self.status = Status::Running,
            Err(_) => self.status = Status::Unknown,
        };

        self.status
    }
}

impl Display for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}]{}{}{:>width_cmd$}{}",
            self.job_num,
            self.job_pos,
            self.status,
            self.cmd,
            if self.status == Status::Running {
                " &"
            } else {
                ""
            },
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
                }
            }

            let job_id = child.id();
            let job_num = self.jobs.len() + 1;

            self.jobs.push(Job::new(cmd, &args, job_num, child));
            println!("[{}] {}", self.jobs.len(), job_id);
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

    pub(crate) fn print_jobs(&mut self) {
        self.jobs.retain_mut(|j| match j.check_status() {
            Status::Unknown => false,
            Status::Complete => false,
            _ => {
                println!("{}", j);
                true
            }
        });
    }
}

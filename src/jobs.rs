//! Background job tracking: spawning, status polling, and `jobs` builtin output.

use std::{fmt::Display, process::Child};

/// Lifecycle state of a background job.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Job has been acknowledged as Done and can be cleaned up.
    Complete,
    /// Process exited since the last poll; printed once then transitions to `Complete`.
    Done,
    /// Process is still running.
    Running,
    /// `try_wait` returned an error; the job will be discarded.
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

        write!(f, "  {:<7}", disp)
    }
}

/// Shell job-list marker displayed next to a job number (e.g. `[1]+`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobPosition {
    /// Most-recently started job; displayed as `+`.
    Current,
    /// Second-most-recently started job; displayed as `-`.
    Prev,
    /// All older jobs; no marker.
    Untracked,
}

impl JobPosition {
    /// Returns the marker for the job one rank below `curr` in the most-recent ordering.
    fn get_next(curr: &JobPosition) -> JobPosition {
        match curr {
            JobPosition::Current => JobPosition::Prev,
            JobPosition::Prev => JobPosition::Untracked,
            JobPosition::Untracked => JobPosition::Untracked,
        }
    }
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

/// A single background job entry in the job table.
struct Job {
    job_num: usize,
    job_pos: JobPosition,
    process: Child,
    cmd: String,
    status: Status,
}

impl Job {
    /// Creates a new `Job` in the `Running` state with `Current` position.
    fn new(command: String, job_num: usize, process: Child) -> Self {
        Self {
            job_num,
            job_pos: JobPosition::Current,
            process,
            cmd: command,
            status: Status::Running,
        }
    }

    /// Polls the child process without blocking and updates `self.status`. Returns the new status.
    fn check_status(&mut self) -> Status {
        match &self.process.try_wait() {
            Ok(Some(_)) => {
                if self.status == Status::Running {
                    self.status = Status::Done;
                } else {
                    self.status = Status::Complete;
                };
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

/// Manages the table of background jobs for the current shell session.
pub(crate) struct Jobs {
    jobs: Vec<Job>,
}

impl Jobs {
    /// Creates an empty job table.
    pub(crate) fn new() -> Self {
        Self { jobs: vec![] }
    }

    /// Re-sorts jobs by job number and reassigns `Current`/`Prev`/`Untracked` markers.
    fn adjust_job_order(&mut self) {
        let mut curr = JobPosition::Current;

        self.jobs
            .sort_unstable_by_key(|j| std::cmp::Reverse(j.job_num));

        self.jobs.iter_mut().for_each(|j| {
            j.job_pos = curr;
            curr = JobPosition::get_next(&curr);
        });

        self.jobs.sort_unstable_by_key(|j| j.job_num);
    }

    /// Polls every job and prints a status line for any that have just finished.
    pub(crate) fn check_done_jobs(&mut self) {
        self.jobs.iter_mut().for_each(|j| {
            j.check_status();

            if j.status == Status::Done {
                println!("{}", j)
            }
        });
    }

    /// Polls all jobs, removes finished or errored ones, and recalculates position markers.
    pub(crate) fn check_jobs(&mut self) {
        self.jobs.iter_mut().for_each(|j| {
            j.check_status();
        });

        // Remove jobs that are Complete or Unknown (no longer show-able)
        self.jobs.retain(|j| match j.status {
            Status::Complete => false,
            Status::Done => true,
            Status::Running => true,
            Status::Unknown => false,
        });

        // Recalculate markers before printing
        self.adjust_job_order();
    }

    /// Adds a newly spawned background process to the job table and prints its job number and PID.
    pub(crate) fn track(&mut self, child: Child, command: String) {
        self.check_jobs();

        let job_id = child.id();
        let job_num = self.get_next_job_num();
        self.jobs.push(Job::new(command, job_num, child));

        println!("[{}] {}", self.jobs.len(), job_id);
    }

    /// Returns the lowest positive integer not already used as a job number.
    fn get_next_job_num(&self) -> usize {
        let mut job_num: usize = 1;

        for j in &self.jobs {
            if j.job_num > job_num {
                break;
            }
            if j.job_num == job_num {
                job_num += 1;
            }
        }

        job_num
    }

    /// Refreshes job statuses and prints the current job table (the `jobs` builtin).
    pub(crate) fn print_jobs(&mut self) {
        self.check_jobs();

        let _ = &self.jobs.iter().for_each(|j| {
            println!("{}", j);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn dummy_job(job_num: usize) -> Job {
        // Hold fork_lock during spawn to prevent ETXTBSY in concurrent autocomplete tests
        // (see utils::fork_lock for the full explanation).
        let _lock = crate::utils::fork_lock();
        let child = Command::new("sleep").arg("1000").spawn().unwrap();
        Job::new("sleep 1000".to_string(), job_num, child)
    }

    fn jobs_with_nums(nums: &[usize]) -> Jobs {
        let mut j = Jobs::new();
        for &n in nums {
            j.jobs.push(dummy_job(n));
        }
        j
    }

    fn kill_all(jobs: &mut Jobs) {
        for j in &mut jobs.jobs {
            let _ = j.process.kill();
            let _ = j.process.wait();
        }
    }

    // --- get_next_job_num ---

    #[test]
    fn test_empty_returns_one() {
        assert_eq!(Jobs::new().get_next_job_num(), 1);
    }

    #[test]
    fn test_sequential_returns_next() {
        // [1, 2, 3] → 4
        let mut jobs = jobs_with_nums(&[1, 2, 3]);
        assert_eq!(jobs.get_next_job_num(), 4);
        kill_all(&mut jobs);
    }

    #[test]
    fn test_gap_at_end_fills_gap() {
        // [1, 2, 4] → 3
        let mut jobs = jobs_with_nums(&[1, 2, 4]);
        assert_eq!(jobs.get_next_job_num(), 3);
        kill_all(&mut jobs);
    }

    #[test]
    fn test_gap_at_start_returns_one() {
        // [2, 3, 4] → 1
        let mut jobs = jobs_with_nums(&[2, 3, 4]);
        assert_eq!(jobs.get_next_job_num(), 1);
        kill_all(&mut jobs);
    }

    #[test]
    fn test_single_job_one_returns_two() {
        // [1] → 2
        let mut jobs = jobs_with_nums(&[1]);
        assert_eq!(jobs.get_next_job_num(), 2);
        kill_all(&mut jobs);
    }

    #[test]
    fn test_single_job_not_one_returns_one() {
        // [2] → 1
        let mut jobs = jobs_with_nums(&[2]);
        assert_eq!(jobs.get_next_job_num(), 1);
        kill_all(&mut jobs);
    }

    #[test]
    fn test_gap_in_middle() {
        // [1, 3] → 2
        let mut jobs = jobs_with_nums(&[1, 3]);
        assert_eq!(jobs.get_next_job_num(), 2);
        kill_all(&mut jobs);
    }

    #[test]
    fn test_large_gap_returns_first_hole() {
        // [1, 2, 5, 6] → 3
        let mut jobs = jobs_with_nums(&[1, 2, 5, 6]);
        assert_eq!(jobs.get_next_job_num(), 3);
        kill_all(&mut jobs);
    }
}

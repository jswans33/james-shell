use std::collections::HashMap;
use std::process::Child;

use crate::status;

/// The lifecycle state of a tracked job.
#[derive(Debug, PartialEq)]
pub enum JobStatus {
    Running,
    Stopped,
    Done(i32),
}

/// A single tracked background or stopped job.
pub struct Job {
    pub id: usize,
    pub pid: u32,
    pub pgid: u32,
    pub command: String,
    pub status: JobStatus,
    pub child: Child,
}

/// The shell's job table — tracks all background and stopped jobs.
pub struct JobTable {
    jobs: HashMap<usize, Job>,
    next_id: usize,
}

impl Default for JobTable {
    fn default() -> Self {
        Self::new()
    }
}

impl JobTable {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            next_id: 1,
        }
    }

    /// Add a running background job. Returns `(job_id, pid)`.
    pub fn add(&mut self, child: Child, command: String) -> (usize, u32) {
        let pgid = child.id();
        self.add_with_pgid(child, command, pgid)
    }

    /// Add a running background job with an explicit process-group id.
    pub fn add_with_pgid(&mut self, child: Child, command: String, pgid: u32) -> (usize, u32) {
        let id = self.next_id;
        let pid = child.id();
        self.jobs.insert(
            id,
            Job {
                id,
                pid,
                pgid,
                command,
                status: JobStatus::Running,
                child,
            },
        );
        self.next_id += 1;
        (id, pid)
    }

    /// Add a job that has already been stopped (e.g. via Ctrl-Z). Returns `(job_id, pid)`.
    pub fn add_stopped(&mut self, child: Child, command: String) -> (usize, u32) {
        let pgid = child.id();
        self.add_stopped_with_pgid(child, command, pgid)
    }

    /// Add a stopped job with an explicit process-group id.
    pub fn add_stopped_with_pgid(
        &mut self,
        child: Child,
        command: String,
        pgid: u32,
    ) -> (usize, u32) {
        let (id, pid) = self.add_with_pgid(child, command, pgid);
        if let Some(job) = self.jobs.get_mut(&id) {
            job.status = JobStatus::Stopped;
        }
        (id, pid)
    }

    /// Non-blocking poll of all running jobs. Prints `[N]  Done  cmd` for
    /// any that have finished and removes them from the table.
    pub fn reap(&mut self) {
        let mut done_ids = Vec::new();

        for (id, job) in self.jobs.iter_mut() {
            if job.status != JobStatus::Running {
                continue;
            }
            match job.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status::exit_code(status);
                    job.status = JobStatus::Done(code);
                    println!("[{}]  Done  {}", job.id, job.command);
                    done_ids.push(*id);
                }
                Ok(None) => {} // still running
                Err(e) => {
                    eprintln!("jsh: error checking job {}: {}", id, e);
                }
            }
        }

        for id in done_ids {
            self.jobs.remove(&id);
        }
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Job> {
        self.jobs.get_mut(&id)
    }

    pub fn remove(&mut self, id: usize) -> Option<Job> {
        self.jobs.remove(&id)
    }

    /// All jobs sorted by job ID (ascending).
    pub fn jobs_sorted(&self) -> Vec<&Job> {
        let mut list: Vec<&Job> = self.jobs.values().collect();
        list.sort_by_key(|j| j.id);
        list
    }

    /// Job ID of the most recently added job (any status), for use as the
    /// `fg` / `bg` default when no argument is given.
    pub fn most_recent_id(&self) -> Option<usize> {
        self.jobs.keys().copied().max()
    }

    /// Job ID of the most recently added *stopped* job, for use as the
    /// default target when `bg` is called with no arguments.
    pub fn most_recent_stopped_id(&self) -> Option<usize> {
        self.jobs
            .iter()
            .filter(|(_, j)| j.status == JobStatus::Stopped)
            .map(|(id, _)| *id)
            .max()
    }

    /// IDs of all currently running (not stopped/done) jobs, for `wait`.
    pub fn running_ids(&self) -> Vec<usize> {
        self.jobs
            .iter()
            .filter(|(_, j)| j.status == JobStatus::Running)
            .map(|(id, _)| *id)
            .collect()
    }
}

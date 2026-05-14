use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::worker_api::WorkerPool;

/// The current status of a background job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// The job is waiting to run.
    Queued,
    /// The job is currently running.
    Running,
    /// The job completed successfully.
    Completed,
    /// The job failed.
    Failed,
    /// The job was cancelled before completion.
    Cancelled,
}

/// A task that can be offloaded to a worker process.
pub trait BackgroundJob: Send + Serialize + 'static {
    /// The result type returned on completion.
    type Output: Serialize + for<'de> Deserialize<'de> + Send + 'static;
    /// Returns the unique identifier for this job.
    fn id(&self) -> &str;
}

/// Manages a queue of background jobs with optional worker-pool integration.
pub struct JobScheduler {
    worker_pool: Option<WorkerPool>,
    jobs: Arc<Mutex<HashMap<String, JobStatus>>>,
    results: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

impl JobScheduler {
    /// Creates a new job scheduler with no worker pool.
    pub fn new() -> Self {
        Self {
            worker_pool: None,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            results: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Attaches a worker pool for out-of-process execution.
    pub fn with_worker_pool(mut self, pool: WorkerPool) -> Self {
        self.worker_pool = Some(pool);
        self
    }

    /// Schedules a job for execution.
    pub fn schedule<Job>(&self, job: Job) -> Result<String>
    where
        Job: BackgroundJob,
    {
        let id = job.id().to_string();
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.contains_key(&id) {
                return Err(anyhow!("job already exists: {}", id));
            }
            jobs.insert(id.clone(), JobStatus::Queued);
        }

        if let Some(ref pool) = self.worker_pool {
            let result: Result<Job::Output> = pool.request(job).context("worker request failed");
            let mut jobs = self.jobs.lock().unwrap();
            let mut results = self.results.lock().unwrap();

            match result {
                Ok(output) => {
                    let value =
                        serde_json::to_value(output).context("failed to serialize result")?;
                    results.insert(id.clone(), value);
                    jobs.insert(id.clone(), JobStatus::Completed);
                }
                Err(_) => {
                    jobs.insert(id.clone(), JobStatus::Failed);
                }
            }
        } else {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.insert(id.clone(), JobStatus::Failed);
        }

        Ok(id)
    }

    /// Cancels a queued or running job.
    pub fn cancel(&self, job_id: &str) -> Result<()> {
        let mut jobs = self.jobs.lock().unwrap();
        match jobs.get(job_id) {
            Some(JobStatus::Queued) | Some(JobStatus::Running) => {
                jobs.insert(job_id.to_string(), JobStatus::Cancelled);
                Ok(())
            }
            Some(status) => Err(anyhow!("job cannot be cancelled: {:?}", status)),
            None => Err(anyhow!("job not found: {}", job_id)),
        }
    }

    /// Returns the current status of a job.
    pub fn status(&self, job_id: &str) -> Option<JobStatus> {
        self.jobs.lock().unwrap().get(job_id).cloned()
    }

    /// Returns the serialized result of a completed job.
    pub fn result(&self, job_id: &str) -> Option<serde_json::Value> {
        self.results.lock().unwrap().get(job_id).cloned()
    }

    /// Returns a copy of all tracked job statuses.
    pub fn all_statuses(&self) -> HashMap<String, JobStatus> {
        self.jobs.lock().unwrap().clone()
    }
}

impl Default for JobScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct TestJob {
        id: String,
    }

    impl BackgroundJob for TestJob {
        type Output = String;
        fn id(&self) -> &str {
            &self.id
        }
    }

    #[test]
    fn test_job_scheduler_without_worker_pool() {
        let scheduler = JobScheduler::new();
        let job = TestJob {
            id: "job1".to_string(),
        };
        let id = scheduler.schedule(job).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Failed));
    }

    #[test]
    fn test_job_cancel() {
        let scheduler = JobScheduler::new();
        let id = "job1".to_string();
        scheduler
            .jobs
            .lock()
            .unwrap()
            .insert(id.clone(), JobStatus::Queued);
        assert!(scheduler.cancel(&id).is_ok());
        assert_eq!(scheduler.status(&id), Some(JobStatus::Cancelled));
    }

    #[test]
    fn test_job_cancel_not_found() {
        let scheduler = JobScheduler::new();
        assert!(scheduler.cancel("missing").is_err());
    }

    #[test]
    fn test_job_cancel_completed() {
        let scheduler = JobScheduler::new();
        let id = "job1".to_string();
        scheduler
            .jobs
            .lock()
            .unwrap()
            .insert(id.clone(), JobStatus::Completed);
        assert!(scheduler.cancel(&id).is_err());
    }
}

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::worker_api::WorkerPool;

/// Priority level for scheduling background jobs.
///
/// Jobs with higher priority are sorted first when querying all jobs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobPriority {
    /// Lowest priority; runs only when no other work is pending.
    Low,
    /// Default priority for most jobs.
    #[default]
    Normal,
    /// Elevated priority; scheduled before `Normal` and `Low` jobs.
    High,
    /// Highest priority; pre-empts all other priority levels.
    Critical,
}

impl JobPriority {
    fn ordinal(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Normal => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }
}

impl PartialOrd for JobPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JobPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.ordinal().cmp(&other.ordinal())
    }
}

/// Progress information emitted by a running job.
#[derive(Debug, Clone)]
pub struct JobProgress {
    /// Identifier of the job that emitted this progress event.
    pub job_id: String,
    /// Completion percentage in the range `0.0..=100.0`.
    pub percent: f64,
    /// Optional human-readable description of the current step.
    pub message: Option<String>,
}

/// A cooperative cancellation token backed by an atomic boolean.
///
/// Clone the token to share it between the scheduler and a running job.
/// Call [`CancellationToken::cancel`] to signal cancellation, and check
/// [`CancellationToken::is_cancelled`] from within the job to respond.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a new token in the non-cancelled state.
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns `true` if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// Signals cancellation. All clones of this token will observe the change.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration controlling how a failed job is retried.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts after the initial failure.
    pub max_retries: u32,
    /// Base delay in milliseconds between retries.
    pub delay_ms: u64,
    /// Multiplicative factor applied to `delay_ms` after each successive retry.
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            delay_ms: 1000,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    /// Validate retry settings before scheduling generated jobs.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.delay_ms > 0 || self.max_retries == 0,
            "retry delay must be greater than zero when retries are enabled"
        );
        anyhow::ensure!(
            self.backoff_multiplier.is_finite() && self.backoff_multiplier >= 1.0,
            "retry backoff multiplier must be finite and at least 1.0"
        );
        Ok(())
    }
}

/// The current status of a background job.
#[derive(Debug, Clone, PartialEq)]
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
    /// The job has been temporarily paused.
    Paused,
    /// The job is being retried after a failure.
    Retrying {
        /// The current retry attempt number (1-based).
        attempt: u32,
    },
}

/// A task that can be offloaded to a worker process.
pub trait BackgroundJob: Send + Serialize + 'static {
    /// The result type returned on completion.
    type Output: Serialize + for<'de> Deserialize<'de> + Send + 'static;
    /// Returns the unique identifier for this job.
    fn id(&self) -> &str;
}

/// Rich descriptor that accompanies a [`BackgroundJob`] through the scheduler.
///
/// Carries metadata such as priority, retry policy, cancellation token, and
/// dependency information that the scheduler uses when deciding execution order.
#[derive(Debug, Clone)]
pub struct JobDescriptor {
    /// Unique identifier for the job (must match the `BackgroundJob::id`).
    pub id: String,
    /// Scheduling priority.
    pub priority: JobPriority,
    /// Optional retry configuration applied on failure.
    pub retry_policy: Option<RetryPolicy>,
    /// Token that can be used to cooperatively cancel the job.
    pub cancellation_token: CancellationToken,
    /// IDs of jobs that must complete before this job can start.
    pub dependencies: Vec<String>,
}

impl JobDescriptor {
    /// Creates a minimal descriptor for the given job ID with default settings.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            priority: JobPriority::default(),
            retry_policy: None,
            cancellation_token: CancellationToken::new(),
            dependencies: Vec::new(),
        }
    }

    /// Sets the priority.
    pub fn with_priority(mut self, priority: JobPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Sets the retry policy.
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = Some(policy);
        self
    }

    /// Sets the cancellation token.
    pub fn with_cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = token;
        self
    }

    /// Sets the dependency list.
    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }

    /// Validate descriptor metadata before scheduling.
    pub fn validate(&self) -> Result<()> {
        validate_job_id(&self.id, "job id")?;
        if let Some(policy) = &self.retry_policy {
            policy.validate()?;
        }

        let mut seen = HashSet::new();
        for dependency in &self.dependencies {
            validate_job_id(dependency, "job dependency id")?;
            anyhow::ensure!(
                dependency != &self.id,
                "job cannot depend on itself: {}",
                self.id
            );
            anyhow::ensure!(
                seen.insert(dependency.as_str()),
                "job dependency id is duplicated: {dependency}"
            );
        }

        Ok(())
    }
}

/// Observable snapshot of a job's state at a point in time.
#[derive(Debug, Clone)]
pub struct JobInfo {
    /// Unique identifier of the job.
    pub id: String,
    /// Current status.
    pub status: JobStatus,
    /// Scheduling priority.
    pub priority: JobPriority,
    /// Latest progress report, if any.
    pub progress: Option<JobProgress>,
    /// Number of retries that have been attempted so far.
    pub retry_count: u32,
    /// When the job was first submitted.
    pub created_at: Instant,
    /// When the job transitioned to `Running` (if ever).
    pub started_at: Option<Instant>,
    /// When the job reached a terminal state (if ever).
    pub completed_at: Option<Instant>,
}

/// Internal bookkeeping entry for a tracked job.
#[derive(Debug, Clone)]
struct JobEntry {
    status: JobStatus,
    priority: JobPriority,
    progress: Option<JobProgress>,
    retry_count: u32,
    retry_policy: Option<RetryPolicy>,
    cancellation_token: CancellationToken,
    dependencies: Vec<String>,
    created_at: Instant,
    started_at: Option<Instant>,
    completed_at: Option<Instant>,
}

impl JobEntry {
    fn to_info(&self, id: &str) -> JobInfo {
        JobInfo {
            id: id.to_string(),
            status: self.status.clone(),
            priority: self.priority,
            progress: self.progress.clone(),
            retry_count: self.retry_count,
            created_at: self.created_at,
            started_at: self.started_at,
            completed_at: self.completed_at,
        }
    }
}

/// Manages a queue of background jobs with optional worker-pool integration.
///
/// Supports job priorities, cooperative cancellation, retry policies,
/// dependency graphs, bounded concurrency, and pause/resume semantics.
pub struct JobScheduler {
    worker_pool: Option<WorkerPool>,
    entries: Arc<Mutex<HashMap<String, JobEntry>>>,
    results: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    running_count: Arc<AtomicUsize>,
    max_concurrent: usize,
}

impl JobScheduler {
    /// Creates a new job scheduler with no worker pool and unlimited concurrency.
    pub fn new() -> Self {
        Self {
            worker_pool: None,
            entries: Arc::new(Mutex::new(HashMap::new())),
            results: Arc::new(Mutex::new(HashMap::new())),
            running_count: Arc::new(AtomicUsize::new(0)),
            max_concurrent: usize::MAX,
        }
    }

    /// Creates a new scheduler with bounded concurrency.
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }

    /// Attaches a worker pool for out-of-process execution.
    pub fn with_worker_pool(mut self, pool: WorkerPool) -> Self {
        self.worker_pool = Some(pool);
        self
    }

    /// Schedules a job for execution using default descriptor settings.
    pub fn schedule<Job>(&self, job: Job) -> Result<String>
    where
        Job: BackgroundJob,
    {
        let descriptor = JobDescriptor::new(job.id());
        self.schedule_with_descriptor(job, descriptor)
    }

    /// Schedules a job after validating generated job metadata.
    pub fn schedule_checked<Job>(&self, job: Job) -> Result<String>
    where
        Job: BackgroundJob,
    {
        let descriptor = JobDescriptor::new(job.id());
        self.schedule_with_descriptor_checked(job, descriptor)
    }

    /// Schedules a job for execution with an explicit [`JobDescriptor`].
    pub fn schedule_with_descriptor<Job>(
        &self,
        job: Job,
        descriptor: JobDescriptor,
    ) -> Result<String>
    where
        Job: BackgroundJob,
    {
        let id = job.id().to_string();

        {
            let mut entries = self.entries.lock().unwrap();
            if entries.contains_key(&id) {
                return Err(anyhow!("job already exists: {}", id));
            }

            let entry = JobEntry {
                status: JobStatus::Queued,
                priority: descriptor.priority,
                progress: None,
                retry_count: 0,
                retry_policy: descriptor.retry_policy,
                cancellation_token: descriptor.cancellation_token.clone(),
                dependencies: descriptor.dependencies.clone(),
                created_at: Instant::now(),
                started_at: None,
                completed_at: None,
            };
            entries.insert(id.clone(), entry);
        }

        if !self.dependencies_met(&id) {
            return Ok(id);
        }

        if descriptor.cancellation_token.is_cancelled() {
            let mut entries = self.entries.lock().unwrap();
            if let Some(entry) = entries.get_mut(&id) {
                entry.status = JobStatus::Cancelled;
                entry.completed_at = Some(Instant::now());
            }
            return Ok(id);
        }

        self.try_execute(&id, job)?;
        Ok(id)
    }

    /// Schedules a job with an explicit descriptor after validating metadata.
    pub fn schedule_with_descriptor_checked<Job>(
        &self,
        job: Job,
        descriptor: JobDescriptor,
    ) -> Result<String>
    where
        Job: BackgroundJob,
    {
        descriptor.validate()?;
        anyhow::ensure!(
            job.id() == descriptor.id,
            "job descriptor id must match job id: descriptor={}, job={}",
            descriptor.id,
            job.id()
        );
        self.schedule_with_descriptor(job, descriptor)
    }

    /// Attempts to execute a job, respecting concurrency limits.
    fn try_execute<Job>(&self, id: &str, job: Job) -> Result<()>
    where
        Job: BackgroundJob,
    {
        let current = self.running_count.load(Ordering::Acquire);
        if current >= self.max_concurrent {
            return Ok(());
        }

        self.running_count.fetch_add(1, Ordering::AcqRel);

        {
            let mut entries = self.entries.lock().unwrap();
            if let Some(entry) = entries.get_mut(id) {
                entry.status = JobStatus::Running;
                entry.started_at = Some(Instant::now());
            }
        }

        let result: Result<Job::Output> = if let Some(ref pool) = self.worker_pool {
            pool.request(job).context("worker request failed")
        } else {
            Err(anyhow!("no worker pool configured"))
        };

        self.running_count.fetch_sub(1, Ordering::AcqRel);

        let mut entries = self.entries.lock().unwrap();
        let mut results = self.results.lock().unwrap();

        match result {
            Ok(output) => {
                let value = serde_json::to_value(output).context("failed to serialize result")?;
                results.insert(id.to_string(), value);
                if let Some(entry) = entries.get_mut(id) {
                    entry.status = JobStatus::Completed;
                    entry.completed_at = Some(Instant::now());
                }
            }
            Err(_) => {
                if let Some(entry) = entries.get_mut(id) {
                    let should_retry = entry
                        .retry_policy
                        .as_ref()
                        .map(|p| entry.retry_count < p.max_retries)
                        .unwrap_or(false);

                    if should_retry {
                        entry.retry_count += 1;
                        entry.status = JobStatus::Retrying {
                            attempt: entry.retry_count,
                        };
                    } else {
                        entry.status = JobStatus::Failed;
                        entry.completed_at = Some(Instant::now());
                    }
                }
            }
        }

        Ok(())
    }

    /// Cancels a queued, running, or paused job.
    pub fn cancel(&self, job_id: &str) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        match entries.get_mut(job_id) {
            Some(entry) => match &entry.status {
                JobStatus::Queued
                | JobStatus::Running
                | JobStatus::Paused
                | JobStatus::Retrying { .. } => {
                    entry.cancellation_token.cancel();
                    entry.status = JobStatus::Cancelled;
                    entry.completed_at = Some(Instant::now());
                    Ok(())
                }
                status => Err(anyhow!("job cannot be cancelled: {:?}", status)),
            },
            None => Err(anyhow!("job not found: {}", job_id)),
        }
    }

    /// Pauses a queued or running job.
    pub fn pause(&self, job_id: &str) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        match entries.get_mut(job_id) {
            Some(entry) => match entry.status {
                JobStatus::Queued | JobStatus::Running => {
                    entry.status = JobStatus::Paused;
                    Ok(())
                }
                _ => Err(anyhow!("job cannot be paused in state: {:?}", entry.status)),
            },
            None => Err(anyhow!("job not found: {}", job_id)),
        }
    }

    /// Resumes a previously paused job back to `Queued` status.
    pub fn resume(&self, job_id: &str) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        match entries.get_mut(job_id) {
            Some(entry) if entry.status == JobStatus::Paused => {
                entry.status = JobStatus::Queued;
                Ok(())
            }
            Some(entry) => Err(anyhow!(
                "job cannot be resumed from state: {:?}",
                entry.status
            )),
            None => Err(anyhow!("job not found: {}", job_id)),
        }
    }

    /// Reports progress for a running job.
    pub fn report_progress(&self, progress: JobProgress) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        match entries.get_mut(&progress.job_id) {
            Some(entry) if entry.status == JobStatus::Running => {
                if !(0.0..=100.0).contains(&progress.percent) {
                    return Err(anyhow!(
                        "progress percent must be in 0..=100, got {}",
                        progress.percent
                    ));
                }
                entry.progress = Some(progress);
                Ok(())
            }
            Some(_) => Err(anyhow!("can only report progress for running jobs")),
            None => Err(anyhow!("job not found: {}", progress.job_id)),
        }
    }

    /// Returns the current status of a job.
    pub fn status(&self, job_id: &str) -> Option<JobStatus> {
        self.entries
            .lock()
            .unwrap()
            .get(job_id)
            .map(|e| e.status.clone())
    }

    /// Returns the serialized result of a completed job.
    pub fn result(&self, job_id: &str) -> Option<serde_json::Value> {
        self.results.lock().unwrap().get(job_id).cloned()
    }

    /// Returns a copy of all tracked job statuses.
    pub fn all_statuses(&self) -> HashMap<String, JobStatus> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.status.clone()))
            .collect()
    }

    /// Returns observable information for all tracked jobs, sorted by priority
    /// (highest first).
    pub fn jobs(&self) -> Vec<JobInfo> {
        let entries = self.entries.lock().unwrap();
        let mut infos: Vec<JobInfo> = entries
            .iter()
            .map(|(id, entry)| entry.to_info(id))
            .collect();
        infos.sort_by_key(|b| std::cmp::Reverse(b.priority));
        infos
    }

    /// Returns observable information for a single job.
    pub fn job_info(&self, job_id: &str) -> Option<JobInfo> {
        self.entries
            .lock()
            .unwrap()
            .get(job_id)
            .map(|e| e.to_info(job_id))
    }

    /// Returns the number of jobs currently in the `Running` state.
    pub fn running_count(&self) -> usize {
        self.running_count.load(Ordering::Acquire)
    }

    /// Returns the maximum number of concurrently running jobs.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Checks whether all dependencies of a job have completed.
    fn dependencies_met(&self, job_id: &str) -> bool {
        let entries = self.entries.lock().unwrap();
        let deps = match entries.get(job_id) {
            Some(entry) => entry.dependencies.clone(),
            None => return false,
        };

        deps.iter().all(|dep_id| {
            entries
                .get(dep_id)
                .map(|e| e.status == JobStatus::Completed)
                .unwrap_or(false)
        })
    }
}

fn validate_job_id(id: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!id.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        id == id.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(id.len() <= 128, "{label} cannot be longer than 128 bytes");
    anyhow::ensure!(
        id.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '-' | '_' | '/')),
        "{label} must contain only ASCII letters, numbers, '.', ':', '-', '_' or '/'"
    );
    Ok(())
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
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Queued,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: None,
                },
            );
        }
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
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Completed,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: Some(Instant::now()),
                },
            );
        }
        assert!(scheduler.cancel(&id).is_err());
    }

    #[test]
    fn test_priority_ordering() {
        assert!(JobPriority::Critical > JobPriority::High);
        assert!(JobPriority::High > JobPriority::Normal);
        assert!(JobPriority::Normal > JobPriority::Low);
        assert!(JobPriority::Low < JobPriority::Critical);
    }

    #[test]
    fn test_priority_default_is_normal() {
        assert_eq!(JobPriority::default(), JobPriority::Normal);
    }

    #[test]
    fn test_cancellation_token() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        let clone = token.clone();
        token.cancel();
        assert!(token.is_cancelled());
        assert!(clone.is_cancelled());
    }

    #[test]
    fn test_cancellation_token_default() {
        let token = CancellationToken::default();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.delay_ms, 1000);
        assert!((policy.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_job_descriptor_builder() {
        let token = CancellationToken::new();
        let descriptor = JobDescriptor::new("test-job")
            .with_priority(JobPriority::High)
            .with_retry_policy(RetryPolicy {
                max_retries: 5,
                delay_ms: 500,
                backoff_multiplier: 1.5,
            })
            .with_cancellation_token(token.clone())
            .with_dependencies(vec!["dep1".to_string(), "dep2".to_string()]);

        assert_eq!(descriptor.id, "test-job");
        assert_eq!(descriptor.priority, JobPriority::High);
        assert!(descriptor.retry_policy.is_some());
        assert_eq!(descriptor.retry_policy.unwrap().max_retries, 5);
        assert_eq!(descriptor.dependencies.len(), 2);
    }

    #[test]
    fn test_job_descriptor_validates_generated_metadata() {
        assert!(JobDescriptor::new("").validate().is_err());
        assert!(JobDescriptor::new(" job").validate().is_err());
        assert!(JobDescriptor::new("job id").validate().is_err());
        assert!(JobDescriptor::new("job\nid").validate().is_err());
        assert!(JobDescriptor::new("a".repeat(129)).validate().is_err());

        assert!(
            JobDescriptor::new("job")
                .with_dependencies(vec!["job".to_string()])
                .validate()
                .is_err()
        );
        assert!(
            JobDescriptor::new("job")
                .with_dependencies(vec!["dep".to_string(), "dep".to_string()])
                .validate()
                .is_err()
        );
        assert!(
            JobDescriptor::new("job")
                .with_dependencies(vec!["bad dep".to_string()])
                .validate()
                .is_err()
        );
        assert!(
            JobDescriptor::new("job")
                .with_retry_policy(RetryPolicy {
                    max_retries: 1,
                    delay_ms: 0,
                    backoff_multiplier: 2.0,
                })
                .validate()
                .is_err()
        );
        assert!(
            JobDescriptor::new("job")
                .with_retry_policy(RetryPolicy {
                    max_retries: 1,
                    delay_ms: 100,
                    backoff_multiplier: 0.5,
                })
                .validate()
                .is_err()
        );

        assert!(
            JobDescriptor::new("job")
                .with_dependencies(vec!["parent".to_string()])
                .with_retry_policy(RetryPolicy {
                    max_retries: 0,
                    delay_ms: 0,
                    backoff_multiplier: 1.0,
                })
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn test_schedule_checked_validates_descriptor_and_job_id() {
        let scheduler = JobScheduler::new();
        assert!(
            scheduler
                .schedule_checked(TestJob {
                    id: "bad id".to_string(),
                })
                .is_err()
        );

        let descriptor = JobDescriptor::new("other-job");
        assert!(
            scheduler
                .schedule_with_descriptor_checked(
                    TestJob {
                        id: "actual-job".to_string(),
                    },
                    descriptor,
                )
                .is_err()
        );

        let id = scheduler
            .schedule_checked(TestJob {
                id: "valid-job".to_string(),
            })
            .unwrap();
        assert_eq!(id, "valid-job");
        assert_eq!(scheduler.status(&id), Some(JobStatus::Failed));
    }

    #[test]
    fn test_job_status_paused_variant() {
        let status = JobStatus::Paused;
        assert_eq!(status, JobStatus::Paused);
        assert_ne!(status, JobStatus::Queued);
    }

    #[test]
    fn test_job_status_retrying_variant() {
        let status = JobStatus::Retrying { attempt: 2 };
        assert_eq!(status, JobStatus::Retrying { attempt: 2 });
        assert_ne!(status, JobStatus::Retrying { attempt: 1 });
        assert_ne!(status, JobStatus::Running);
    }

    #[test]
    fn test_pause_and_resume() {
        let scheduler = JobScheduler::new();
        let id = "pausable".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Queued,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: None,
                },
            );
        }

        scheduler.pause(&id).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Paused));

        scheduler.resume(&id).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Queued));
    }

    #[test]
    fn test_pause_invalid_state() {
        let scheduler = JobScheduler::new();
        let id = "completed-job".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Completed,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: Some(Instant::now()),
                },
            );
        }
        assert!(scheduler.pause(&id).is_err());
    }

    #[test]
    fn test_resume_invalid_state() {
        let scheduler = JobScheduler::new();
        let id = "queued-job".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Queued,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: None,
                },
            );
        }
        assert!(scheduler.resume(&id).is_err());
    }

    #[test]
    fn test_pause_not_found() {
        let scheduler = JobScheduler::new();
        assert!(scheduler.pause("ghost").is_err());
    }

    #[test]
    fn test_resume_not_found() {
        let scheduler = JobScheduler::new();
        assert!(scheduler.resume("ghost").is_err());
    }

    #[test]
    fn test_report_progress() {
        let scheduler = JobScheduler::new();
        let id = "running-job".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Running,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: Some(Instant::now()),
                    completed_at: None,
                },
            );
        }

        let progress = JobProgress {
            job_id: id.clone(),
            percent: 50.0,
            message: Some("halfway there".to_string()),
        };
        scheduler.report_progress(progress).unwrap();

        let info = scheduler.job_info(&id).unwrap();
        let p = info.progress.unwrap();
        assert!((p.percent - 50.0).abs() < f64::EPSILON);
        assert_eq!(p.message, Some("halfway there".to_string()));
    }

    #[test]
    fn test_report_progress_invalid_percent() {
        let scheduler = JobScheduler::new();
        let id = "running-job".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Running,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: Some(Instant::now()),
                    completed_at: None,
                },
            );
        }

        let progress = JobProgress {
            job_id: id.clone(),
            percent: 150.0,
            message: None,
        };
        assert!(scheduler.report_progress(progress).is_err());
    }

    #[test]
    fn test_report_progress_wrong_state() {
        let scheduler = JobScheduler::new();
        let id = "queued-job".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Queued,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: None,
                },
            );
        }

        let progress = JobProgress {
            job_id: id.clone(),
            percent: 10.0,
            message: None,
        };
        assert!(scheduler.report_progress(progress).is_err());
    }

    #[test]
    fn test_report_progress_not_found() {
        let scheduler = JobScheduler::new();
        let progress = JobProgress {
            job_id: "ghost".to_string(),
            percent: 10.0,
            message: None,
        };
        assert!(scheduler.report_progress(progress).is_err());
    }

    #[test]
    fn test_jobs_sorted_by_priority() {
        let scheduler = JobScheduler::new();
        let now = Instant::now();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            for (id, priority) in [
                ("low-job", JobPriority::Low),
                ("high-job", JobPriority::High),
                ("normal-job", JobPriority::Normal),
                ("critical-job", JobPriority::Critical),
            ] {
                entries.insert(
                    id.to_string(),
                    JobEntry {
                        status: JobStatus::Queued,
                        priority,
                        progress: None,
                        retry_count: 0,
                        retry_policy: None,
                        cancellation_token: CancellationToken::new(),
                        dependencies: vec![],
                        created_at: now,
                        started_at: None,
                        completed_at: None,
                    },
                );
            }
        }

        let jobs = scheduler.jobs();
        assert_eq!(jobs.len(), 4);
        assert_eq!(jobs[0].priority, JobPriority::Critical);
        assert_eq!(jobs[1].priority, JobPriority::High);
        assert_eq!(jobs[2].priority, JobPriority::Normal);
        assert_eq!(jobs[3].priority, JobPriority::Low);
    }

    #[test]
    fn test_job_info_fields() {
        let scheduler = JobScheduler::new();
        let now = Instant::now();
        let id = "info-job".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Running,
                    priority: JobPriority::High,
                    progress: Some(JobProgress {
                        job_id: id.clone(),
                        percent: 75.0,
                        message: Some("almost done".to_string()),
                    }),
                    retry_count: 1,
                    retry_policy: Some(RetryPolicy::default()),
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: now,
                    started_at: Some(now),
                    completed_at: None,
                },
            );
        }

        let info = scheduler.job_info(&id).unwrap();
        assert_eq!(info.id, id);
        assert_eq!(info.status, JobStatus::Running);
        assert_eq!(info.priority, JobPriority::High);
        assert_eq!(info.retry_count, 1);
        assert!(info.started_at.is_some());
        assert!(info.completed_at.is_none());
        assert!(info.progress.is_some());
    }

    #[test]
    fn test_job_info_not_found() {
        let scheduler = JobScheduler::new();
        assert!(scheduler.job_info("nope").is_none());
    }

    #[test]
    fn test_bounded_concurrency_tracking() {
        let scheduler = JobScheduler::new().with_max_concurrent(2);
        assert_eq!(scheduler.max_concurrent(), 2);
        assert_eq!(scheduler.running_count(), 0);
    }

    #[test]
    fn test_bounded_concurrency_blocks_excess() {
        let scheduler = JobScheduler::new().with_max_concurrent(0);
        let job = TestJob {
            id: "blocked".to_string(),
        };
        let id = scheduler.schedule(job).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Queued));
    }

    #[test]
    fn test_schedule_with_descriptor() {
        let scheduler = JobScheduler::new();
        let job = TestJob {
            id: "desc-job".to_string(),
        };
        let descriptor = JobDescriptor::new("desc-job").with_priority(JobPriority::Critical);
        let id = scheduler.schedule_with_descriptor(job, descriptor).unwrap();

        let info = scheduler.job_info(&id).unwrap();
        assert_eq!(info.priority, JobPriority::Critical);
    }

    #[test]
    fn test_schedule_duplicate_rejected() {
        let scheduler = JobScheduler::new();
        let job1 = TestJob {
            id: "dup".to_string(),
        };
        let job2 = TestJob {
            id: "dup".to_string(),
        };
        scheduler.schedule(job1).unwrap();
        assert!(scheduler.schedule(job2).is_err());
    }

    #[test]
    fn test_dependencies_block_execution() {
        let scheduler = JobScheduler::new();
        let job = TestJob {
            id: "child".to_string(),
        };
        let descriptor = JobDescriptor::new("child").with_dependencies(vec!["parent".to_string()]);

        let id = scheduler.schedule_with_descriptor(job, descriptor).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Queued));
    }

    #[test]
    fn test_dependencies_met_when_parent_completed() {
        let scheduler = JobScheduler::new();
        let now = Instant::now();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                "parent".to_string(),
                JobEntry {
                    status: JobStatus::Completed,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: now,
                    started_at: Some(now),
                    completed_at: Some(now),
                },
            );
        }

        let job = TestJob {
            id: "child".to_string(),
        };
        let descriptor = JobDescriptor::new("child").with_dependencies(vec!["parent".to_string()]);

        let id = scheduler.schedule_with_descriptor(job, descriptor).unwrap();
        assert_ne!(scheduler.status(&id), Some(JobStatus::Queued));
    }

    #[test]
    fn test_cancel_triggers_token() {
        let scheduler = JobScheduler::new();
        let token = CancellationToken::new();
        let id = "token-job".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Running,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: token.clone(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: Some(Instant::now()),
                    completed_at: None,
                },
            );
        }

        assert!(!token.is_cancelled());
        scheduler.cancel(&id).unwrap();
        assert!(token.is_cancelled());
        assert_eq!(scheduler.status(&id), Some(JobStatus::Cancelled));
    }

    #[test]
    fn test_cancel_paused_job() {
        let scheduler = JobScheduler::new();
        let id = "paused-cancel".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Paused,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: None,
                },
            );
        }
        assert!(scheduler.cancel(&id).is_ok());
        assert_eq!(scheduler.status(&id), Some(JobStatus::Cancelled));
    }

    #[test]
    fn test_cancel_retrying_job() {
        let scheduler = JobScheduler::new();
        let id = "retrying-cancel".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Retrying { attempt: 2 },
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 2,
                    retry_policy: Some(RetryPolicy::default()),
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: Some(Instant::now()),
                    completed_at: None,
                },
            );
        }
        assert!(scheduler.cancel(&id).is_ok());
        assert_eq!(scheduler.status(&id), Some(JobStatus::Cancelled));
    }

    #[test]
    fn test_schedule_with_pre_cancelled_token() {
        let scheduler = JobScheduler::new();
        let token = CancellationToken::new();
        token.cancel();

        let job = TestJob {
            id: "pre-cancelled".to_string(),
        };
        let descriptor = JobDescriptor::new("pre-cancelled").with_cancellation_token(token);

        let id = scheduler.schedule_with_descriptor(job, descriptor).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Cancelled));
    }

    #[test]
    fn test_retry_policy_on_failure() {
        let scheduler = JobScheduler::new();
        let job = TestJob {
            id: "retry-me".to_string(),
        };
        let descriptor = JobDescriptor::new("retry-me").with_retry_policy(RetryPolicy {
            max_retries: 3,
            delay_ms: 100,
            backoff_multiplier: 1.0,
        });

        let id = scheduler.schedule_with_descriptor(job, descriptor).unwrap();
        assert_eq!(
            scheduler.status(&id),
            Some(JobStatus::Retrying { attempt: 1 })
        );
        let info = scheduler.job_info(&id).unwrap();
        assert_eq!(info.retry_count, 1);
    }

    #[test]
    fn test_all_statuses() {
        let scheduler = JobScheduler::new();
        let now = Instant::now();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                "a".to_string(),
                JobEntry {
                    status: JobStatus::Queued,
                    priority: JobPriority::Low,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: now,
                    started_at: None,
                    completed_at: None,
                },
            );
            entries.insert(
                "b".to_string(),
                JobEntry {
                    status: JobStatus::Completed,
                    priority: JobPriority::High,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: now,
                    started_at: Some(now),
                    completed_at: Some(now),
                },
            );
        }

        let statuses = scheduler.all_statuses();
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses["a"], JobStatus::Queued);
        assert_eq!(statuses["b"], JobStatus::Completed);
    }

    #[test]
    fn test_pause_running_job() {
        let scheduler = JobScheduler::new();
        let id = "running-pause".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Running,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: Some(Instant::now()),
                    completed_at: None,
                },
            );
        }

        scheduler.pause(&id).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Paused));
    }

    #[test]
    fn test_completed_at_set_on_cancel() {
        let scheduler = JobScheduler::new();
        let id = "cancel-ts".to_string();
        {
            let mut entries = scheduler.entries.lock().unwrap();
            entries.insert(
                id.clone(),
                JobEntry {
                    status: JobStatus::Queued,
                    priority: JobPriority::Normal,
                    progress: None,
                    retry_count: 0,
                    retry_policy: None,
                    cancellation_token: CancellationToken::new(),
                    dependencies: vec![],
                    created_at: Instant::now(),
                    started_at: None,
                    completed_at: None,
                },
            );
        }

        scheduler.cancel(&id).unwrap();
        let info = scheduler.job_info(&id).unwrap();
        assert!(info.completed_at.is_some());
    }
}

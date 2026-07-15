use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::worker_api::WorkerPool;

const MAX_TRACKED_JOBS: usize = 4_096;
const MAX_JOB_DEPENDENCIES: usize = 256;
const MAX_RETRIES: u32 = 100;
const MAX_RETRY_DELAY_MS: u64 = 60 * 60 * 1000;
const MAX_JOB_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_JOB_RESULT_BYTES: usize = 1024 * 1024;

type ResultDecoder = Arc<dyn Fn(serde_json::Value) -> Result<serde_json::Value> + Send + Sync>;

#[derive(Clone)]
struct PendingJob {
    payload: serde_json::Value,
    decode_result: ResultDecoder,
}

struct RunningSlot(Arc<AtomicUsize>);

impl Drop for RunningSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

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

    /// Stable lowercase key for logs and generated scheduling policies.
    pub fn key(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
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

impl JobProgress {
    /// Whether a human-readable progress message is present.
    pub fn has_message(&self) -> bool {
        self.message.is_some()
    }

    /// Content-safe summary that avoids logging job ids, exact percent, or messages.
    pub fn to_text(&self) -> String {
        format!(
            "job progress: message {}, complete {}",
            self.has_message(),
            self.percent >= 100.0
        )
    }
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
            self.max_retries <= MAX_RETRIES,
            "retry attempts cannot exceed {MAX_RETRIES}"
        );
        anyhow::ensure!(
            self.delay_ms > 0 || self.max_retries == 0,
            "retry delay must be greater than zero when retries are enabled"
        );
        anyhow::ensure!(
            self.backoff_multiplier.is_finite() && self.backoff_multiplier >= 1.0,
            "retry backoff multiplier must be finite and at least 1.0"
        );
        anyhow::ensure!(
            self.backoff_multiplier <= 100.0,
            "retry backoff multiplier cannot exceed 100"
        );
        anyhow::ensure!(
            self.delay_ms <= MAX_RETRY_DELAY_MS,
            "retry delay cannot exceed {MAX_RETRY_DELAY_MS}ms"
        );
        Ok(())
    }

    /// Return the capped delay for a one-based retry attempt.
    pub fn delay_for_attempt(&self, attempt: u32) -> Option<std::time::Duration> {
        if self.validate().is_err() || attempt == 0 || attempt > self.max_retries {
            return None;
        }
        let exponent = i32::try_from(attempt.saturating_sub(1)).unwrap_or(i32::MAX);
        let multiplier = self.backoff_multiplier.powi(exponent);
        let delay = (self.delay_ms as f64 * multiplier).min(MAX_RETRY_DELAY_MS as f64);
        Some(std::time::Duration::from_millis(delay as u64))
    }

    /// Whether this policy schedules any retry attempts.
    pub fn has_retries(&self) -> bool {
        self.max_retries > 0
    }

    /// Whether this policy applies exponential or multiplicative backoff.
    pub fn has_backoff(&self) -> bool {
        self.backoff_multiplier > 1.0
    }

    /// Content-safe summary that avoids logging exact delays and counts.
    pub fn to_text(&self) -> String {
        format!(
            "retry policy: retries {}, delay {}, backoff {}",
            self.has_retries(),
            self.delay_ms > 0,
            self.has_backoff()
        )
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

impl JobStatus {
    /// Stable lowercase key for logs and routing.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Paused => "paused",
            Self::Retrying { .. } => "retrying",
        }
    }

    /// Whether this status is terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Whether this status means work is actively executing or will resume soon.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Retrying { .. })
    }

    /// Content-safe summary that avoids logging retry attempt counts.
    pub fn to_text(&self) -> String {
        format!(
            "job status: {}, active {}, terminal {}",
            self.key(),
            self.is_active(),
            self.is_terminal()
        )
    }
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

    /// Whether retry behavior is configured.
    pub fn has_retry_policy(&self) -> bool {
        self.retry_policy.is_some()
    }

    /// Number of configured dependencies.
    pub fn dependency_count(&self) -> usize {
        self.dependencies.len()
    }

    /// Whether cancellation has already been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// Content-safe summary that avoids logging job ids and dependency ids.
    pub fn to_text(&self) -> String {
        format!(
            "job descriptor: priority {}, retry {}, dependencies {}, cancelled {}",
            self.priority.key(),
            self.has_retry_policy(),
            self.dependency_count(),
            self.is_cancelled()
        )
    }

    /// Validate descriptor metadata before scheduling.
    pub fn validate(&self) -> Result<()> {
        validate_job_id(&self.id, "job id")?;
        if let Some(policy) = &self.retry_policy {
            policy.validate()?;
        }
        anyhow::ensure!(
            self.dependencies.len() <= MAX_JOB_DEPENDENCIES,
            "job cannot contain more than {MAX_JOB_DEPENDENCIES} dependencies"
        );

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

impl JobInfo {
    /// Whether the job has emitted progress.
    pub fn has_progress(&self) -> bool {
        self.progress.is_some()
    }

    /// Whether the job has ever started.
    pub fn has_started(&self) -> bool {
        self.started_at.is_some()
    }

    /// Whether the job has completed, failed, or cancelled.
    pub fn has_completed(&self) -> bool {
        self.completed_at.is_some() || self.status.is_terminal()
    }

    /// Content-safe summary that avoids logging job ids, progress messages, and timings.
    pub fn to_text(&self) -> String {
        format!(
            "job info: status {}, priority {}, progress {}, retries {}, started {}, completed {}",
            self.status.key(),
            self.priority.key(),
            self.has_progress(),
            self.retry_count > 0,
            self.has_started(),
            self.has_completed()
        )
    }
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
    pending: Arc<Mutex<HashMap<String, PendingJob>>>,
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
            pending: Arc::new(Mutex::new(HashMap::new())),
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
        descriptor.validate()?;
        anyhow::ensure!(
            id == descriptor.id,
            "job descriptor id must match job id: descriptor={}, job={id}",
            descriptor.id
        );
        let payload_bytes =
            serde_json::to_vec(&job).context("failed to serialize background job")?;
        anyhow::ensure!(
            payload_bytes.len() <= MAX_JOB_PAYLOAD_BYTES,
            "background job payload cannot exceed {MAX_JOB_PAYLOAD_BYTES} bytes"
        );
        let payload = serde_json::from_slice(&payload_bytes)
            .context("failed to decode serialized background job")?;
        let decode_result: ResultDecoder = Arc::new(|value| {
            let output: Job::Output = serde_json::from_value(value)
                .context("failed to deserialize background job result")?;
            let bytes =
                serde_json::to_vec(&output).context("failed to serialize background job result")?;
            anyhow::ensure!(
                bytes.len() <= MAX_JOB_RESULT_BYTES,
                "background job result cannot exceed {MAX_JOB_RESULT_BYTES} bytes"
            );
            serde_json::from_slice(&bytes)
                .context("failed to decode serialized background job result")
        });

        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if entries.contains_key(&id) {
                return Err(anyhow!("job already exists: {}", id));
            }
            anyhow::ensure!(
                entries.len() < MAX_TRACKED_JOBS,
                "background job scheduler cannot track more than {MAX_TRACKED_JOBS} jobs"
            );
            for dependency in &descriptor.dependencies {
                anyhow::ensure!(
                    !dependency_reaches(&entries, dependency, &id),
                    "job dependency cycle detected for {id}"
                );
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

        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                id.clone(),
                PendingJob {
                    payload,
                    decode_result,
                },
            );

        if descriptor.cancellation_token.is_cancelled() {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(entry) = entries.get_mut(&id) {
                entry.status = JobStatus::Cancelled;
                entry.completed_at = Some(Instant::now());
            }
            self.pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&id);
            return Ok(id);
        }

        self.drain_ready()?;
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
        self.schedule_with_descriptor(job, descriptor)
    }

    fn try_acquire_slot(&self) -> bool {
        let mut current = self.running_count.load(Ordering::Acquire);
        loop {
            if current >= self.max_concurrent {
                return false;
            }
            match self.running_count.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    fn try_execute(&self, id: &str) -> Result<bool> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(id)
            .cloned();
        let Some(pending) = pending else {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(entry) = entries.get_mut(id) {
                entry.status = JobStatus::Failed;
                entry.completed_at = Some(Instant::now());
            }
            return Ok(true);
        };

        if !self.try_acquire_slot() {
            return Ok(false);
        }
        let _slot = RunningSlot(self.running_count.clone());

        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(entry) = entries.get_mut(id) {
                entry.status = JobStatus::Running;
                entry.started_at = Some(Instant::now());
            }
        }

        let result: Result<serde_json::Value> = if let Some(ref pool) = self.worker_pool {
            let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pool.request::<_, serde_json::Value>(pending.payload.clone())
            }))
            .map_err(|_| anyhow!("worker request panicked"))?
            .context("worker request failed");
            response.and_then(|value| (pending.decode_result)(value))
        } else {
            Err(anyhow!("no worker pool configured"))
        };

        let mut completed_value = None;
        let mut clear_pending = false;
        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(entry) = entries.get_mut(id) else {
                return Ok(true);
            };
            if entry.cancellation_token.is_cancelled() || entry.status == JobStatus::Cancelled {
                entry.status = JobStatus::Cancelled;
                entry.completed_at.get_or_insert_with(Instant::now);
                clear_pending = true;
            } else {
                match result {
                    Ok(value) => {
                        completed_value = Some(value);
                        entry.status = JobStatus::Completed;
                        entry.completed_at = Some(Instant::now());
                        clear_pending = true;
                    }
                    Err(_) => {
                        let should_retry = entry
                            .retry_policy
                            .as_ref()
                            .is_some_and(|policy| entry.retry_count < policy.max_retries);
                        if should_retry {
                            entry.retry_count += 1;
                            entry.status = JobStatus::Retrying {
                                attempt: entry.retry_count,
                            };
                        } else {
                            entry.status = JobStatus::Failed;
                            entry.completed_at = Some(Instant::now());
                            clear_pending = true;
                        }
                    }
                }
            }
        }

        if let Some(value) = completed_value {
            self.results
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(id.to_string(), value);
        }
        if clear_pending {
            self.pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(id);
        }

        Ok(true)
    }

    fn drain_ready(&self) -> Result<()> {
        loop {
            let pending_ids: HashSet<String> = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .keys()
                .cloned()
                .collect();
            let next = {
                let entries = self
                    .entries
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                entries
                    .iter()
                    .filter(|(id, entry)| {
                        entry.status == JobStatus::Queued
                            && entry.dependencies.iter().all(|dependency| {
                                entries.get(dependency).is_some_and(|dependency| {
                                    dependency.status == JobStatus::Completed
                                })
                            })
                            && pending_ids.contains(*id)
                    })
                    .max_by_key(|(_, entry)| (entry.priority, std::cmp::Reverse(entry.created_at)))
                    .map(|(id, _)| id.clone())
            };
            let Some(id) = next else { break };
            if !self.try_execute(&id)? {
                break;
            }
        }
        Ok(())
    }

    /// Cancels a queued, running, or paused job.
    pub fn cancel(&self, job_id: &str) -> Result<()> {
        let result = {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        };
        if result.is_ok() {
            self.pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(job_id);
        }
        result
    }

    /// Pauses a queued or running job.
    pub fn pause(&self, job_id: &str) -> Result<()> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match entries.get_mut(job_id) {
                Some(entry) if entry.status == JobStatus::Paused => {
                    entry.status = JobStatus::Queued;
                }
                Some(entry) => {
                    return Err(anyhow!(
                        "job cannot be resumed from state: {:?}",
                        entry.status
                    ));
                }
                None => return Err(anyhow!("job not found: {}", job_id)),
            }
        }
        self.drain_ready()
    }

    /// Requeues a job after the caller has observed its retry delay.
    pub fn retry(&self, job_id: &str) -> Result<()> {
        anyhow::ensure!(
            self.pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(job_id),
            "retry payload is unavailable: {job_id}"
        );
        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match entries.get_mut(job_id) {
                Some(entry) if matches!(entry.status, JobStatus::Retrying { .. }) => {
                    entry.status = JobStatus::Queued;
                }
                Some(entry) => {
                    return Err(anyhow!(
                        "job cannot be retried from state: {:?}",
                        entry.status
                    ));
                }
                None => return Err(anyhow!("job not found: {job_id}")),
            }
        }
        self.drain_ready()
    }

    /// Returns the configured delay for the job's current retry attempt.
    pub fn retry_delay(&self, job_id: &str) -> Option<std::time::Duration> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = entries.get(job_id)?;
        let JobStatus::Retrying { attempt } = entry.status else {
            return None;
        };
        entry.retry_policy.as_ref()?.delay_for_attempt(attempt)
    }

    /// Reports progress for a running job.
    pub fn report_progress(&self, progress: JobProgress) -> Result<()> {
        validate_job_id(&progress.job_id, "progress job id")?;
        anyhow::ensure!(
            progress.percent.is_finite() && (0.0..=100.0).contains(&progress.percent),
            "progress percent must be finite and in 0..=100"
        );
        if let Some(message) = &progress.message {
            validate_background_reason(message, "progress message")?;
        }
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match entries.get_mut(&progress.job_id) {
            Some(entry) if entry.status == JobStatus::Running => {
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(job_id)
            .map(|e| e.status.clone())
    }

    /// Returns the serialized result of a completed job.
    pub fn result(&self, job_id: &str) -> Option<serde_json::Value> {
        self.results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(job_id)
            .cloned()
    }

    /// Returns a copy of all tracked job statuses.
    pub fn all_statuses(&self) -> HashMap<String, JobStatus> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), v.status.clone()))
            .collect()
    }

    /// Returns observable information for all tracked jobs, sorted by priority
    /// (highest first).
    pub fn jobs(&self) -> Vec<JobInfo> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    /// Removes a terminal job and any stored result from the scheduler.
    pub fn remove_terminal(&self, job_id: &str) -> Result<()> {
        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let entry = entries
                .get(job_id)
                .ok_or_else(|| anyhow!("job not found: {job_id}"))?;
            anyhow::ensure!(entry.status.is_terminal(), "job is not terminal: {job_id}");
            entries.remove(job_id);
        }
        self.results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(job_id);
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(job_id);
        Ok(())
    }
}

fn dependency_reaches(entries: &HashMap<String, JobEntry>, start: &str, target: &str) -> bool {
    let mut stack = vec![start];
    let mut visited = HashSet::new();
    while let Some(job_id) = stack.pop() {
        if job_id == target {
            return true;
        }
        if !visited.insert(job_id) {
            continue;
        }
        if let Some(entry) = entries.get(job_id) {
            stack.extend(entry.dependencies.iter().map(String::as_str));
        }
    }
    false
}

/// Next action for a checked background-work handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundWorkNextAction {
    /// Schedule the job immediately.
    ScheduleJob,
    /// Queue the job and wait for dependencies to complete.
    WaitForDependencies,
    /// Report progress for a running job.
    ReportProgress,
    /// Cancel a queued, running, paused, or retrying job.
    CancelJob,
    /// Pause a queued or running job.
    PauseJob,
    /// Resume a paused job.
    ResumeJob,
    /// Use a worker pool for off-UI-thread execution.
    UseWorkerPool,
    /// Escalate to a helper or utility process.
    UseHelperProcess,
}

impl BackgroundWorkNextAction {
    /// Stable key for logs and generated routing.
    pub fn key(self) -> &'static str {
        match self {
            Self::ScheduleJob => "schedule-job",
            Self::WaitForDependencies => "wait-for-dependencies",
            Self::ReportProgress => "report-progress",
            Self::CancelJob => "cancel-job",
            Self::PauseJob => "pause-job",
            Self::ResumeJob => "resume-job",
            Self::UseWorkerPool => "use-worker-pool",
            Self::UseHelperProcess => "use-helper-process",
        }
    }
}

/// Checked background-work request for generated jobs, workers, and queues.
#[derive(Debug, Clone)]
pub enum BackgroundWorkRequest {
    /// Job descriptor ready for scheduling.
    Job(JobDescriptor),
    /// Progress update for a running job.
    Progress(JobProgress),
    /// Cancel a job.
    Cancel {
        /// Job identifier to cancel.
        job_id: String,
    },
    /// Pause a job.
    Pause {
        /// Job identifier to pause.
        job_id: String,
    },
    /// Resume a job.
    Resume {
        /// Job identifier to resume.
        job_id: String,
    },
    /// Require a worker pool before executing generated work.
    WorkerPool {
        /// Diagnostic reason for requiring a worker pool.
        reason: String,
    },
    /// Escalate work to a helper process.
    HelperProcess {
        /// Diagnostic reason for requiring helper-process isolation.
        reason: String,
    },
}

impl BackgroundWorkRequest {
    /// Whether this request schedules a job descriptor.
    pub fn is_job(&self) -> bool {
        matches!(self, Self::Job(_))
    }

    /// Whether this request reports progress.
    pub fn is_progress(&self) -> bool {
        matches!(self, Self::Progress(_))
    }

    /// Whether this request cancels a job.
    pub fn is_cancel(&self) -> bool {
        matches!(self, Self::Cancel { .. })
    }

    /// Whether this request pauses a job.
    pub fn is_pause(&self) -> bool {
        matches!(self, Self::Pause { .. })
    }

    /// Whether this request resumes a job.
    pub fn is_resume(&self) -> bool {
        matches!(self, Self::Resume { .. })
    }

    /// Whether this request requires a worker pool.
    pub fn is_worker_pool(&self) -> bool {
        matches!(self, Self::WorkerPool { .. })
    }

    /// Whether this request escalates to a helper process.
    pub fn is_helper_process(&self) -> bool {
        matches!(self, Self::HelperProcess { .. })
    }

    /// Next action implied by this request.
    pub fn next_action(&self) -> BackgroundWorkNextAction {
        match self {
            Self::Job(descriptor) if descriptor.dependency_count() > 0 => {
                BackgroundWorkNextAction::WaitForDependencies
            }
            Self::Job(_) => BackgroundWorkNextAction::ScheduleJob,
            Self::Progress(_) => BackgroundWorkNextAction::ReportProgress,
            Self::Cancel { .. } => BackgroundWorkNextAction::CancelJob,
            Self::Pause { .. } => BackgroundWorkNextAction::PauseJob,
            Self::Resume { .. } => BackgroundWorkNextAction::ResumeJob,
            Self::WorkerPool { .. } => BackgroundWorkNextAction::UseWorkerPool,
            Self::HelperProcess { .. } => BackgroundWorkNextAction::UseHelperProcess,
        }
    }

    /// Job descriptor when this request schedules a job.
    pub fn descriptor(&self) -> Option<&JobDescriptor> {
        match self {
            Self::Job(descriptor) => Some(descriptor),
            _ => None,
        }
    }

    /// Progress update when this request reports progress.
    pub fn progress(&self) -> Option<&JobProgress> {
        match self {
            Self::Progress(progress) => Some(progress),
            _ => None,
        }
    }

    /// Job id for job, progress, cancel, pause, or resume requests.
    pub fn job_id(&self) -> Option<&str> {
        match self {
            Self::Job(descriptor) => Some(&descriptor.id),
            Self::Progress(progress) => Some(&progress.job_id),
            Self::Cancel { job_id } | Self::Pause { job_id } | Self::Resume { job_id } => {
                Some(job_id)
            }
            Self::WorkerPool { .. } | Self::HelperProcess { .. } => None,
        }
    }

    /// Whether this request has a helper/worker reason.
    pub fn has_reason(&self) -> bool {
        matches!(
            self,
            Self::WorkerPool { reason } | Self::HelperProcess { reason } if !reason.is_empty()
        )
    }

    /// Content-safe request summary.
    pub fn to_text(&self) -> String {
        let detail = match self {
            Self::Job(descriptor) => descriptor.to_text(),
            Self::Progress(progress) => progress.to_text(),
            Self::Cancel { .. } => "job control: cancel".to_string(),
            Self::Pause { .. } => "job control: pause".to_string(),
            Self::Resume { .. } => "job control: resume".to_string(),
            Self::WorkerPool { .. } => {
                format!("worker pool request: reason {}", self.has_reason())
            }
            Self::HelperProcess { .. } => {
                format!("helper process request: reason {}", self.has_reason())
            }
        };
        format!(
            "background work request: action {}, {}",
            self.next_action().key(),
            detail
        )
    }

    /// Validate the request before routing generated background work.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Job(descriptor) => descriptor.validate(),
            Self::Progress(progress) => {
                validate_job_id(&progress.job_id, "progress job id")?;
                anyhow::ensure!(
                    progress.percent.is_finite() && (0.0..=100.0).contains(&progress.percent),
                    "progress percent must be finite and in 0..=100"
                );
                if let Some(message) = &progress.message {
                    validate_background_reason(message, "progress message")?;
                }
                Ok(())
            }
            Self::Cancel { job_id } => validate_job_id(job_id, "cancel job id"),
            Self::Pause { job_id } => validate_job_id(job_id, "pause job id"),
            Self::Resume { job_id } => validate_job_id(job_id, "resume job id"),
            Self::WorkerPool { reason } => validate_background_reason(reason, "worker pool reason"),
            Self::HelperProcess { reason } => {
                validate_background_reason(reason, "helper process reason")
            }
        }
    }
}

/// Builder for a checked background-work handoff.
#[derive(Debug, Clone)]
pub struct BackgroundWorkHandoffBuilder {
    request: BackgroundWorkRequest,
}

impl BackgroundWorkHandoffBuilder {
    /// Handoff for a job descriptor.
    pub fn descriptor(descriptor: JobDescriptor) -> Self {
        Self {
            request: BackgroundWorkRequest::Job(descriptor),
        }
    }

    /// Handoff for a job id with default descriptor settings.
    pub fn job(job_id: impl Into<String>) -> Self {
        Self::descriptor(JobDescriptor::new(job_id))
    }

    /// Handoff for a progress update.
    pub fn progress(job_id: impl Into<String>, percent: f64) -> Self {
        Self {
            request: BackgroundWorkRequest::Progress(JobProgress {
                job_id: job_id.into(),
                percent,
                message: None,
            }),
        }
    }

    /// Attach a progress message to a progress handoff.
    pub fn progress_message(mut self, message: impl Into<String>) -> Self {
        if let BackgroundWorkRequest::Progress(progress) = &mut self.request {
            progress.message = Some(message.into());
        }
        self
    }

    /// Handoff for cancelling a job.
    pub fn cancel(job_id: impl Into<String>) -> Self {
        Self {
            request: BackgroundWorkRequest::Cancel {
                job_id: job_id.into(),
            },
        }
    }

    /// Handoff for pausing a job.
    pub fn pause(job_id: impl Into<String>) -> Self {
        Self {
            request: BackgroundWorkRequest::Pause {
                job_id: job_id.into(),
            },
        }
    }

    /// Handoff for resuming a job.
    pub fn resume(job_id: impl Into<String>) -> Self {
        Self {
            request: BackgroundWorkRequest::Resume {
                job_id: job_id.into(),
            },
        }
    }

    /// Handoff for requiring a worker pool.
    pub fn worker_pool(reason: impl Into<String>) -> Self {
        Self {
            request: BackgroundWorkRequest::WorkerPool {
                reason: reason.into(),
            },
        }
    }

    /// Handoff for escalating work to a helper process.
    pub fn helper_process(reason: impl Into<String>) -> Self {
        Self {
            request: BackgroundWorkRequest::HelperProcess {
                reason: reason.into(),
            },
        }
    }

    /// Request carried by this builder.
    pub fn request(&self) -> &BackgroundWorkRequest {
        &self.request
    }

    /// Next action implied by this builder.
    pub fn next_action(&self) -> BackgroundWorkNextAction {
        self.request.next_action()
    }

    /// Content-safe builder summary.
    pub fn to_text(&self) -> String {
        format!(
            "background work handoff builder: {}",
            self.request.to_text()
        )
    }

    /// Validate the handoff before routing it.
    pub fn validate(&self) -> Result<()> {
        self.request.validate()
    }

    /// Build the checked handoff.
    pub fn build_checked(self) -> Result<BackgroundWorkHandoff> {
        self.validate()?;
        let next_action = self.request.next_action();
        Ok(BackgroundWorkHandoff {
            request: self.request,
            next_action,
        })
    }
}

/// Checked handoff for generated background work and worker routing.
#[derive(Debug, Clone)]
pub struct BackgroundWorkHandoff {
    request: BackgroundWorkRequest,
    next_action: BackgroundWorkNextAction,
}

impl BackgroundWorkHandoff {
    /// Build a checked job descriptor handoff.
    pub fn descriptor(descriptor: JobDescriptor) -> Result<Self> {
        BackgroundWorkHandoffBuilder::descriptor(descriptor).build_checked()
    }

    /// Build a checked default job handoff.
    pub fn job(job_id: impl Into<String>) -> Result<Self> {
        BackgroundWorkHandoffBuilder::job(job_id).build_checked()
    }

    /// Build a checked progress handoff.
    pub fn progress(job_id: impl Into<String>, percent: f64) -> Result<Self> {
        BackgroundWorkHandoffBuilder::progress(job_id, percent).build_checked()
    }

    /// Build a checked cancel handoff.
    pub fn cancel(job_id: impl Into<String>) -> Result<Self> {
        BackgroundWorkHandoffBuilder::cancel(job_id).build_checked()
    }

    /// Build a checked pause handoff.
    pub fn pause(job_id: impl Into<String>) -> Result<Self> {
        BackgroundWorkHandoffBuilder::pause(job_id).build_checked()
    }

    /// Build a checked resume handoff.
    pub fn resume(job_id: impl Into<String>) -> Result<Self> {
        BackgroundWorkHandoffBuilder::resume(job_id).build_checked()
    }

    /// Build a checked worker-pool handoff.
    pub fn worker_pool(reason: impl Into<String>) -> Result<Self> {
        BackgroundWorkHandoffBuilder::worker_pool(reason).build_checked()
    }

    /// Build a checked helper-process handoff.
    pub fn helper_process(reason: impl Into<String>) -> Result<Self> {
        BackgroundWorkHandoffBuilder::helper_process(reason).build_checked()
    }

    /// Request carried by this handoff.
    pub fn request(&self) -> &BackgroundWorkRequest {
        &self.request
    }

    /// Next action to take.
    pub fn next_action(&self) -> BackgroundWorkNextAction {
        self.next_action
    }

    /// Whether this handoff schedules a job.
    pub fn is_job(&self) -> bool {
        self.request.is_job()
    }

    /// Whether this handoff reports progress.
    pub fn is_progress(&self) -> bool {
        self.request.is_progress()
    }

    /// Whether this handoff cancels a job.
    pub fn is_cancel(&self) -> bool {
        self.request.is_cancel()
    }

    /// Whether this handoff pauses a job.
    pub fn is_pause(&self) -> bool {
        self.request.is_pause()
    }

    /// Whether this handoff resumes a job.
    pub fn is_resume(&self) -> bool {
        self.request.is_resume()
    }

    /// Whether this handoff requires a worker pool.
    pub fn is_worker_pool(&self) -> bool {
        self.request.is_worker_pool()
    }

    /// Whether this handoff escalates to a helper process.
    pub fn is_helper_process(&self) -> bool {
        self.request.is_helper_process()
    }

    /// Job descriptor when present.
    pub fn descriptor_ref(&self) -> Option<&JobDescriptor> {
        self.request.descriptor()
    }

    /// Progress update when present.
    pub fn progress_ref(&self) -> Option<&JobProgress> {
        self.request.progress()
    }

    /// Job id when present.
    pub fn job_id(&self) -> Option<&str> {
        self.request.job_id()
    }

    /// Content-safe handoff summary.
    pub fn to_text(&self) -> String {
        format!("background work handoff: {}", self.request.to_text())
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

fn validate_background_reason(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.len() <= 256,
        "{label} cannot be longer than 256 bytes"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
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

    #[derive(Serialize)]
    struct LargeJob {
        id: String,
        payload: String,
    }

    impl BackgroundJob for LargeJob {
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
        assert_eq!(JobPriority::Critical.key(), "critical");
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
        assert!(policy.has_retries());
        assert!(policy.has_backoff());
        assert_eq!(
            policy.to_text(),
            "retry policy: retries true, delay true, backoff true"
        );
    }

    #[test]
    fn test_retry_policy_bounds_and_caps_backoff_delay() {
        let policy = RetryPolicy {
            max_retries: 100,
            delay_ms: 1_000,
            backoff_multiplier: 100.0,
        };
        assert_eq!(
            policy.delay_for_attempt(1),
            Some(std::time::Duration::from_secs(1))
        );
        assert_eq!(
            policy.delay_for_attempt(100),
            Some(std::time::Duration::from_secs(60 * 60))
        );
        assert_eq!(policy.delay_for_attempt(0), None);
        assert_eq!(policy.delay_for_attempt(101), None);

        assert!(
            RetryPolicy {
                max_retries: 101,
                ..RetryPolicy::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            RetryPolicy {
                delay_ms: MAX_RETRY_DELAY_MS + 1,
                ..RetryPolicy::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            RetryPolicy {
                backoff_multiplier: 100.1,
                ..RetryPolicy::default()
            }
            .validate()
            .is_err()
        );
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
    fn test_job_descriptor_and_state_summaries_are_content_safe() {
        let descriptor = JobDescriptor::new("export/private-video")
            .with_priority(JobPriority::Critical)
            .with_retry_policy(RetryPolicy {
                max_retries: 2,
                delay_ms: 250,
                backoff_multiplier: 1.5,
            })
            .with_dependencies(vec![
                "index/private-project".to_string(),
                "download/source".to_string(),
            ]);

        assert!(descriptor.has_retry_policy());
        assert_eq!(descriptor.dependency_count(), 2);
        assert!(!descriptor.is_cancelled());
        assert_eq!(
            descriptor.to_text(),
            "job descriptor: priority critical, retry true, dependencies 2, cancelled false"
        );
        assert!(!descriptor.to_text().contains("private-video"));
        assert!(!descriptor.to_text().contains("private-project"));

        let status = JobStatus::Retrying { attempt: 3 };
        assert_eq!(status.key(), "retrying");
        assert!(status.is_active());
        assert!(!status.is_terminal());
        assert_eq!(
            status.to_text(),
            "job status: retrying, active true, terminal false"
        );
        assert!(!status.to_text().contains("3"));

        let progress = JobProgress {
            job_id: "export/private-video".to_string(),
            percent: 42.5,
            message: Some("Rendering scene 9".to_string()),
        };
        assert!(progress.has_message());
        assert_eq!(
            progress.to_text(),
            "job progress: message true, complete false"
        );
        assert!(!progress.to_text().contains("42.5"));
        assert!(!progress.to_text().contains("Rendering"));

        let info = JobInfo {
            id: "export/private-video".to_string(),
            status,
            priority: JobPriority::Critical,
            progress: Some(progress),
            retry_count: 3,
            created_at: Instant::now(),
            started_at: Some(Instant::now()),
            completed_at: None,
        };
        assert!(info.has_progress());
        assert!(info.has_started());
        assert!(!info.has_completed());
        assert_eq!(
            info.to_text(),
            "job info: status retrying, priority critical, progress true, retries true, started true, completed false"
        );
        assert!(!info.to_text().contains("private-video"));
        assert!(!info.to_text().contains("Rendering"));
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
        assert!(
            JobDescriptor::new("job")
                .with_dependencies(
                    (0..=MAX_JOB_DEPENDENCIES)
                        .map(|index| format!("dependency-{index}"))
                        .collect()
                )
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_background_work_handoff_guides_generated_worker_routing() {
        let schedule = BackgroundWorkHandoffBuilder::job("export/video")
            .build_checked()
            .unwrap();
        assert!(schedule.is_job());
        assert_eq!(
            schedule.next_action(),
            BackgroundWorkNextAction::ScheduleJob
        );
        assert_eq!(schedule.job_id(), Some("export/video"));
        assert_eq!(
            schedule.to_text(),
            "background work handoff: background work request: action schedule-job, job descriptor: priority normal, retry false, dependencies 0, cancelled false"
        );
        assert!(!schedule.to_text().contains("export/video"));

        let dependency_handoff = BackgroundWorkHandoff::descriptor(
            JobDescriptor::new("export/video")
                .with_priority(JobPriority::High)
                .with_dependencies(vec!["scan/workspace".to_string()])
                .with_retry_policy(RetryPolicy {
                    max_retries: 2,
                    delay_ms: 100,
                    backoff_multiplier: 1.5,
                }),
        )
        .unwrap();
        assert!(dependency_handoff.is_job());
        assert_eq!(
            dependency_handoff.next_action(),
            BackgroundWorkNextAction::WaitForDependencies
        );
        assert_eq!(
            dependency_handoff
                .descriptor_ref()
                .unwrap()
                .dependency_count(),
            1
        );
        assert!(!dependency_handoff.to_text().contains("scan/workspace"));

        let progress = BackgroundWorkHandoffBuilder::progress("export/video", 42.0)
            .progress_message("Rendering private timeline")
            .build_checked()
            .unwrap();
        assert!(progress.is_progress());
        assert_eq!(
            progress.next_action(),
            BackgroundWorkNextAction::ReportProgress
        );
        assert!(progress.progress_ref().unwrap().has_message());
        assert!(!progress.to_text().contains("42"));
        assert!(!progress.to_text().contains("private timeline"));

        let cancel = BackgroundWorkHandoff::cancel("export/video").unwrap();
        assert!(cancel.is_cancel());
        assert_eq!(cancel.next_action(), BackgroundWorkNextAction::CancelJob);
        assert_eq!(cancel.job_id(), Some("export/video"));
        assert!(!cancel.to_text().contains("export/video"));

        let pause = BackgroundWorkHandoff::pause("export/video").unwrap();
        assert!(pause.is_pause());
        assert_eq!(pause.next_action(), BackgroundWorkNextAction::PauseJob);

        let resume = BackgroundWorkHandoff::resume("export/video").unwrap();
        assert!(resume.is_resume());
        assert_eq!(resume.next_action(), BackgroundWorkNextAction::ResumeJob);

        let worker_pool =
            BackgroundWorkHandoff::worker_pool("CPU-heavy indexer should not block UI").unwrap();
        assert!(worker_pool.is_worker_pool());
        assert_eq!(
            worker_pool.next_action(),
            BackgroundWorkNextAction::UseWorkerPool
        );
        assert!(!worker_pool.to_text().contains("CPU-heavy"));

        let helper =
            BackgroundWorkHandoff::helper_process("Native module isolation required").unwrap();
        assert!(helper.is_helper_process());
        assert_eq!(
            helper.next_action(),
            BackgroundWorkNextAction::UseHelperProcess
        );
        assert_eq!(
            BackgroundWorkNextAction::UseHelperProcess.key(),
            "use-helper-process"
        );
        assert!(!helper.to_text().contains("Native module"));
    }

    #[test]
    fn test_background_work_handoff_rejects_invalid_generated_requests() {
        assert!(BackgroundWorkHandoff::job("bad job").is_err());
        assert!(
            BackgroundWorkHandoff::descriptor(
                JobDescriptor::new("job").with_dependencies(vec!["bad dep".to_string()])
            )
            .is_err()
        );
        assert!(BackgroundWorkHandoff::progress("job", f64::NAN).is_err());
        assert!(BackgroundWorkHandoff::progress("job", 101.0).is_err());
        assert!(
            BackgroundWorkHandoffBuilder::progress("job", 50.0)
                .progress_message(" private step")
                .build_checked()
                .is_err()
        );
        assert!(BackgroundWorkHandoff::cancel("bad job").is_err());
        assert!(BackgroundWorkHandoff::pause("bad job").is_err());
        assert!(BackgroundWorkHandoff::resume("bad job").is_err());
        assert!(BackgroundWorkHandoff::worker_pool("").is_err());
        assert!(BackgroundWorkHandoff::helper_process("bad\nreason").is_err());
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
    fn test_scheduler_rejects_oversized_requests_and_results() {
        let scheduler = JobScheduler::new().with_max_concurrent(0);
        assert!(
            scheduler
                .schedule(LargeJob {
                    id: "large-job".to_string(),
                    payload: "x".repeat(MAX_JOB_PAYLOAD_BYTES),
                })
                .is_err()
        );
        assert!(scheduler.status("large-job").is_none());

        let id = scheduler
            .schedule(TestJob {
                id: "large-result".to_string(),
            })
            .unwrap();
        let pending = scheduler.pending.lock().unwrap().get(&id).cloned().unwrap();
        assert!(
            (pending.decode_result)(serde_json::Value::String("x".repeat(MAX_JOB_RESULT_BYTES)))
                .is_err()
        );
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

        let progress = JobProgress {
            job_id: id.clone(),
            percent: f64::NAN,
            message: None,
        };
        assert!(scheduler.report_progress(progress).is_err());

        let progress = JobProgress {
            job_id: id,
            percent: 50.0,
            message: Some(" invalid message".to_string()),
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
        assert!(scheduler.pending.lock().unwrap().contains_key(&id));
    }

    #[test]
    fn test_concurrency_slot_acquisition_is_atomic() {
        let scheduler = Arc::new(JobScheduler::new().with_max_concurrent(2));
        let barrier = Arc::new(std::sync::Barrier::new(17));
        let mut threads = Vec::new();
        for _ in 0..16 {
            let scheduler = scheduler.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                scheduler.try_acquire_slot()
            }));
        }
        barrier.wait();
        let acquired = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(|acquired| *acquired)
            .count();
        assert_eq!(acquired, 2);
        assert_eq!(scheduler.running_count(), 2);
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
    fn test_dependency_cycle_is_rejected_without_partial_insertion() {
        let scheduler = JobScheduler::new();
        scheduler
            .schedule_with_descriptor(
                TestJob {
                    id: "job-a".to_string(),
                },
                JobDescriptor::new("job-a").with_dependencies(vec!["job-b".to_string()]),
            )
            .unwrap();

        assert!(
            scheduler
                .schedule_with_descriptor(
                    TestJob {
                        id: "job-b".to_string(),
                    },
                    JobDescriptor::new("job-b").with_dependencies(vec!["job-a".to_string()]),
                )
                .is_err()
        );
        assert!(scheduler.status("job-b").is_none());
        assert!(!scheduler.pending.lock().unwrap().contains_key("job-b"));
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
        assert_eq!(
            scheduler.retry_delay(&id),
            Some(std::time::Duration::from_millis(100))
        );
        assert!(scheduler.pending.lock().unwrap().contains_key(&id));

        scheduler.retry(&id).unwrap();
        assert_eq!(
            scheduler.status(&id),
            Some(JobStatus::Retrying { attempt: 2 })
        );
        scheduler.retry(&id).unwrap();
        assert_eq!(
            scheduler.status(&id),
            Some(JobStatus::Retrying { attempt: 3 })
        );
        scheduler.retry(&id).unwrap();
        assert_eq!(scheduler.status(&id), Some(JobStatus::Failed));
        assert!(scheduler.retry_delay(&id).is_none());
        assert!(!scheduler.pending.lock().unwrap().contains_key(&id));
        assert!(scheduler.retry(&id).is_err());
    }

    #[test]
    fn test_cancelled_and_terminal_jobs_release_retained_state() {
        let scheduler = JobScheduler::new().with_max_concurrent(0);
        let id = scheduler
            .schedule(TestJob {
                id: "cleanup-job".to_string(),
            })
            .unwrap();
        assert!(scheduler.pending.lock().unwrap().contains_key(&id));

        scheduler.cancel(&id).unwrap();
        assert!(!scheduler.pending.lock().unwrap().contains_key(&id));
        scheduler
            .results
            .lock()
            .unwrap()
            .insert(id.clone(), serde_json::json!("stale"));
        scheduler.remove_terminal(&id).unwrap();
        assert!(scheduler.status(&id).is_none());
        assert!(scheduler.result(&id).is_none());
        assert!(scheduler.remove_terminal(&id).is_err());
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

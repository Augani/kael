use std::time::Duration;

/// Policy controlling retry behavior with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Base delay in milliseconds before the first retry.
    pub base_delay_ms: u64,
    /// Maximum delay cap in milliseconds.
    pub max_delay_ms: u64,
    /// Multiplicative factor applied per attempt for exponential backoff.
    pub backoff_factor: f64,
    /// Fractional randomization applied to each delay to reduce retry herding.
    pub jitter_ratio: f64,
}

impl RetryPolicy {
    /// Create a new retry policy with sensible defaults.
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            base_delay_ms: 100,
            max_delay_ms: 30_000,
            backoff_factor: 2.0,
            jitter_ratio: 0.1,
        }
    }

    /// Set the backoff multiplication factor.
    pub fn with_backoff(mut self, factor: f64) -> Self {
        self.backoff_factor = finite_non_negative(factor, 1.0);
        self
    }

    /// Set the base delay in milliseconds.
    pub fn with_base_delay(mut self, ms: u64) -> Self {
        self.base_delay_ms = ms;
        self
    }

    /// Set the maximum delay cap in milliseconds.
    pub fn with_max_delay(mut self, ms: u64) -> Self {
        self.max_delay_ms = ms;
        self
    }

    /// Set the symmetric jitter ratio applied to the computed delay.
    pub fn with_jitter(mut self, ratio: f64) -> Self {
        self.jitter_ratio = if ratio.is_finite() {
            ratio.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }

    /// Calculate the delay duration for a given attempt number (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if self.base_delay_ms == 0 || self.max_delay_ms == 0 {
            return Duration::ZERO;
        }

        // The fields are public and may have been modified without the builder,
        // so contain invalid floating-point values again at the use boundary.
        let factor = finite_non_negative(self.backoff_factor, 1.0);
        let exponential = self.base_delay_ms as f64 * factor.powf(f64::from(attempt));
        let capped_ms = if exponential.is_finite() {
            exponential.min(self.max_delay_ms as f64)
        } else {
            self.max_delay_ms as f64
        };
        let jitter_ratio = if self.jitter_ratio.is_finite() {
            self.jitter_ratio.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let jittered_ms = if jitter_ratio > 0.0 {
            let jitter = fastrand::f64() * (jitter_ratio * 2.0) - jitter_ratio;
            (capped_ms * (1.0 + jitter)).clamp(0.0, self.max_delay_ms as f64)
        } else {
            capped_ms
        };
        Duration::from_millis(jittered_ms.round() as u64)
    }

    /// Determine whether a retry should be attempted given the attempt number and status code.
    ///
    /// Retries on server errors (5xx), 408 (Request Timeout), and 429 (Too Many Requests).
    pub fn should_retry(&self, attempt: u32, status: u16) -> bool {
        if attempt >= self.max_retries {
            return false;
        }
        matches!(status, 408 | 429 | 500..=599)
    }
}

fn finite_non_negative(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults() {
        let policy = RetryPolicy::new(3);
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.base_delay_ms, 100);
        assert_eq!(policy.max_delay_ms, 30_000);
        assert!((policy.backoff_factor - 2.0).abs() < f64::EPSILON);
        assert!((policy.jitter_ratio - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_with_backoff() {
        let policy = RetryPolicy::new(3).with_backoff(1.5);
        assert!((policy.backoff_factor - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_with_base_delay() {
        let policy = RetryPolicy::new(3).with_base_delay(200);
        assert_eq!(policy.base_delay_ms, 200);
    }

    #[test]
    fn test_with_max_delay() {
        let policy = RetryPolicy::new(3).with_max_delay(5000);
        assert_eq!(policy.max_delay_ms, 5000);
    }

    #[test]
    fn test_delay_for_attempt_exponential() {
        let policy = RetryPolicy::new(5)
            .with_base_delay(100)
            .with_backoff(2.0)
            .with_jitter(0.0);
        assert_eq!(policy.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(800));
    }

    #[test]
    fn test_delay_capped_at_max() {
        let policy = RetryPolicy::new(10)
            .with_base_delay(1000)
            .with_max_delay(5000)
            .with_backoff(3.0)
            .with_jitter(0.0);
        let delay = policy.delay_for_attempt(5);
        assert_eq!(delay, Duration::from_millis(5000));
    }

    #[test]
    fn test_jitter_stays_within_expected_range() {
        let policy = RetryPolicy::new(1)
            .with_base_delay(1000)
            .with_jitter(0.25)
            .with_backoff(1.0);
        let delay = policy.delay_for_attempt(0);
        assert!(delay >= Duration::from_millis(750));
        assert!(delay <= Duration::from_millis(1250));
    }

    #[test]
    fn test_should_retry_server_errors() {
        let policy = RetryPolicy::new(3);
        assert!(policy.should_retry(0, 500));
        assert!(policy.should_retry(0, 502));
        assert!(policy.should_retry(0, 503));
        assert!(policy.should_retry(1, 500));
    }

    #[test]
    fn test_should_retry_timeout_and_rate_limit() {
        let policy = RetryPolicy::new(3);
        assert!(policy.should_retry(0, 408));
        assert!(policy.should_retry(0, 429));
    }

    #[test]
    fn test_should_not_retry_client_errors() {
        let policy = RetryPolicy::new(3);
        assert!(!policy.should_retry(0, 400));
        assert!(!policy.should_retry(0, 401));
        assert!(!policy.should_retry(0, 403));
        assert!(!policy.should_retry(0, 404));
    }

    #[test]
    fn test_should_not_retry_success() {
        let policy = RetryPolicy::new(3);
        assert!(!policy.should_retry(0, 200));
        assert!(!policy.should_retry(0, 201));
    }

    #[test]
    fn test_should_not_retry_past_max() {
        let policy = RetryPolicy::new(3);
        assert!(!policy.should_retry(3, 500));
        assert!(!policy.should_retry(4, 500));
    }

    #[test]
    fn test_zero_retries_never_retries() {
        let policy = RetryPolicy::new(0);
        assert!(!policy.should_retry(0, 500));
    }

    #[test]
    fn invalid_float_configuration_is_contained() {
        let policy = RetryPolicy::new(3)
            .with_base_delay(250)
            .with_max_delay(1_000)
            .with_backoff(f64::NAN)
            .with_jitter(f64::INFINITY);
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(250));

        let mut directly_mutated = policy;
        directly_mutated.backoff_factor = f64::INFINITY;
        directly_mutated.jitter_ratio = f64::NAN;
        assert_eq!(
            directly_mutated.delay_for_attempt(u32::MAX),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn huge_attempts_saturate_at_the_delay_cap() {
        let policy = RetryPolicy::new(3)
            .with_base_delay(1)
            .with_max_delay(1234)
            .with_backoff(2.0)
            .with_jitter(0.0);
        assert_eq!(
            policy.delay_for_attempt(u32::MAX),
            Duration::from_millis(1234)
        );
    }
}

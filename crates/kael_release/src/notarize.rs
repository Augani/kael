//! macOS notarization status tracking and log parsing.

use serde::{Deserialize, Serialize};

/// Status of a macOS notarization submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotarizeStatus {
    /// Submission created but not yet sent.
    Pending,
    /// Submission is being processed by Apple.
    InProgress,
    /// Notarization succeeded.
    Accepted,
    /// Notarization was rejected with a reason.
    Rejected(String),
    /// Submission was invalid with an error description.
    Invalid(String),
}

/// Result of a notarization submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotarizeResult {
    /// Current status.
    pub status: NotarizeStatus,
    /// Apple-assigned submission identifier.
    pub submission_id: Option<String>,
    /// Raw log entries from the notarization process.
    pub log_entries: Vec<String>,
}

impl NotarizeResult {
    /// Returns whether notarization was accepted.
    pub fn is_success(&self) -> bool {
        self.status == NotarizeStatus::Accepted
    }

    /// Extracts warning messages from log entries (lines containing "warning").
    pub fn warnings(&self) -> Vec<&str> {
        self.log_entries
            .iter()
            .filter(|line| contains_ascii_case_insensitive(line, b"warning"))
            .map(|s| s.as_str())
            .collect()
    }

    /// Extracts error messages from log entries (lines containing "error").
    pub fn errors(&self) -> Vec<&str> {
        self.log_entries
            .iter()
            .filter(|line| contains_ascii_case_insensitive(line, b"error"))
            .map(|s| s.as_str())
            .collect()
    }

    /// Parses log entries to extract all warnings and errors.
    pub fn parse_log(&self) -> (Vec<&str>, Vec<&str>) {
        (self.warnings(), self.errors())
    }
}

fn contains_ascii_case_insensitive(value: &str, needle: &[u8]) -> bool {
    value
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_success_accepted() {
        let result = NotarizeResult {
            status: NotarizeStatus::Accepted,
            submission_id: Some("sub-123".to_string()),
            log_entries: vec![],
        };
        assert!(result.is_success());
    }

    #[test]
    fn is_success_rejected() {
        let result = NotarizeResult {
            status: NotarizeStatus::Rejected("hardened runtime missing".to_string()),
            submission_id: Some("sub-456".to_string()),
            log_entries: vec![],
        };
        assert!(!result.is_success());
    }

    #[test]
    fn is_success_pending() {
        let result = NotarizeResult {
            status: NotarizeStatus::Pending,
            submission_id: None,
            log_entries: vec![],
        };
        assert!(!result.is_success());
    }

    #[test]
    fn parse_log_extracts_warnings_and_errors() {
        let result = NotarizeResult {
            status: NotarizeStatus::Accepted,
            submission_id: None,
            log_entries: vec![
                "INFO: Processing started".to_string(),
                "WARNING: unsigned framework detected".to_string(),
                "ERROR: missing entitlement".to_string(),
                "INFO: Completed".to_string(),
                "Warning: deprecated API usage".to_string(),
            ],
        };
        let (warnings, errors) = result.parse_log();
        assert_eq!(warnings.len(), 2);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn parse_log_empty() {
        let result = NotarizeResult {
            status: NotarizeStatus::InProgress,
            submission_id: None,
            log_entries: vec![],
        };
        let (warnings, errors) = result.parse_log();
        assert!(warnings.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn notarize_status_serialization() {
        let statuses = vec![
            NotarizeStatus::Pending,
            NotarizeStatus::InProgress,
            NotarizeStatus::Accepted,
            NotarizeStatus::Rejected("reason".to_string()),
            NotarizeStatus::Invalid("bad input".to_string()),
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let restored: NotarizeStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, status);
        }
    }

    #[test]
    fn notarize_result_roundtrip() {
        let result = NotarizeResult {
            status: NotarizeStatus::Accepted,
            submission_id: Some("sub-789".to_string()),
            log_entries: vec!["INFO: done".to_string()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: NotarizeResult = serde_json::from_str(&json).unwrap();
        assert!(restored.is_success());
        assert_eq!(restored.submission_id, Some("sub-789".to_string()));
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed errors. Every error carries a kind, a retry hint and the tail of the transcript.

use std::fmt;
use std::time::Duration;

/// What went wrong, in terms a caller can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// The rig did not answer before the command's deadline.
    Timeout,
    /// The rig answered with a refusal (CI-V NG, Kenwood `?;`).
    Rejected,
    /// The rig or its description does not support the operation.
    Unsupported,
    /// The link to the rig is down or the actor has stopped.
    LinkDown,
    /// The command was abandoned before completion (preempted or shut down).
    Cancelled,
    /// A transmit-capable command was refused because the rig's transmit gate is closed.
    TransmitGateClosed,
    /// The rig sent bytes the protocol engine could not make sense of.
    Malformed,
    /// The rig's command queue is full.
    QueueFull,
}

/// Whether and when a caller may usefully try again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryHint {
    /// Retrying the same command cannot succeed.
    Never,
    /// A retry may succeed at once.
    Now,
    /// A retry may succeed after the given delay.
    After(Duration),
}

/// The error type of every radioctl operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigError {
    /// The class of failure.
    pub kind: ErrorKind,
    /// Whether to retry.
    pub retry: RetryHint,
    /// A short human-readable description.
    pub detail: String,
    /// The last wire lines (hex, direction-prefixed) before the failure, oldest first.
    pub transcript_tail: Vec<String>,
}

impl RigError {
    // [@ANCHOR: radioctl_core_error_new]
    /// Builds an error with the default retry hint for its kind and an empty transcript.
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> RigError {
        let retry = match kind {
            ErrorKind::Timeout | ErrorKind::QueueFull | ErrorKind::Cancelled => RetryHint::Now,
            ErrorKind::LinkDown => RetryHint::After(Duration::from_secs(1)),
            ErrorKind::Rejected
            | ErrorKind::Unsupported
            | ErrorKind::TransmitGateClosed
            | ErrorKind::Malformed => RetryHint::Never,
        };
        RigError {
            kind,
            retry,
            detail: detail.into(),
            transcript_tail: Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_core_error_with_transcript]
    /// Attaches the transcript tail.
    pub fn with_transcript(mut self, tail: Vec<String>) -> RigError {
        self.transcript_tail = tail;
        self
    }
}

impl fmt::Display for RigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for RigError {}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_error_new]
    #[test]
    fn retry_hints_follow_the_kind() {
        assert_eq!(RigError::new(ErrorKind::Timeout, "x").retry, RetryHint::Now);
        assert_eq!(
            RigError::new(ErrorKind::Rejected, "x").retry,
            RetryHint::Never
        );
        assert_eq!(
            RigError::new(ErrorKind::TransmitGateClosed, "x").retry,
            RetryHint::Never
        );
        assert!(matches!(
            RigError::new(ErrorKind::LinkDown, "x").retry,
            RetryHint::After(_)
        ));
        assert_eq!(
            RigError::new(ErrorKind::Timeout, "slow").to_string(),
            "Timeout: slow"
        );
    }

    // Tests [@ANCHOR: radioctl_core_error_with_transcript]
    #[test]
    fn transcript_tail_is_kept() {
        let err = RigError::new(ErrorKind::Timeout, "x").with_transcript(vec!["TX fe fe".into()]);
        assert_eq!(err.transcript_tail, vec!["TX fe fe".to_string()]);
    }
}

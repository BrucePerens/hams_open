// SPDX-License-Identifier: AGPL-3.0-or-later
//! The error type of the station layer.

use std::fmt;

/// What went wrong, coarsely, so a caller can branch without parsing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationErrorKind {
    /// A configuration or map is invalid.
    Config,
    /// An interlock or dependency refused the command; nothing was switched.
    Refused,
    /// The keyed state of a rig could not be determined: an RF-path command is refused (power-off
    /// and a radio's power-on pass; Bruce, 2026-10-31).
    KeyedUnknown,
    /// The driver could not reach the hardware or it answered something unknown.
    Driver,
    /// The read-back did not match the commanded state.
    NotConfirmed,
    /// A wait ran out.
    Timeout,
    /// An unknown channel, device, profile or radio.
    Unknown,
    /// The durable record could not be read or written.
    Store,
}

/// An error with a kind and a human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationError {
    /// The class of error.
    pub kind: StationErrorKind,
    /// What happened.
    pub detail: String,
}

impl StationError {
    // [@ANCHOR: radioctl_station_error_new]
    /// Builds an error.
    pub fn new(kind: StationErrorKind, detail: impl Into<String>) -> StationError {
        StationError {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for StationError {
    // [@ANCHOR: radioctl_station_error_display]
    /// `kind: detail`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for StationError {}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_station_error_new]
    // Tests [@ANCHOR: radioctl_station_error_display]
    #[test]
    fn errors_carry_kind_and_text() {
        let e = StationError::new(StationErrorKind::Refused, "keyed");
        assert_eq!(e.kind, StationErrorKind::Refused);
        assert_eq!(e.to_string(), "Refused: keyed");
    }
}

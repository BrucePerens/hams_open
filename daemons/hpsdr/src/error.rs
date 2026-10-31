// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one error type of the crate.

use std::fmt;

/// Why an operation or a datagram was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HpsdrError {
    /// The datagram is shorter than the format needs.
    Truncated,
    /// The datagram does not start with the expected magic, sync bytes or command.
    BadMagic,
    /// A field holds a value this crate does not support (message names it).
    Unsupported(String),
    /// A caller-supplied setting is outside the legal range (message names it).
    Config(String),
    /// A transmit-capable operation was refused because the transmit gate is closed.
    GateClosed,
    /// A socket or task failure (the I/O error text).
    Io(String),
    /// Nothing arrived in time.
    Timeout,
    /// The session has been stopped.
    Stopped,
}

impl fmt::Display for HpsdrError {
    // [@ANCHOR: hpsdr_error_fmt]
    /// Display text of a rejection.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "datagram too short"),
            Self::BadMagic => write!(f, "unexpected magic, sync or command byte"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Config(m) => write!(f, "invalid setting: {m}"),
            Self::GateClosed => write!(f, "transmit gate is closed"),
            Self::Io(m) => write!(f, "i/o error: {m}"),
            Self::Timeout => write!(f, "timed out"),
            Self::Stopped => write!(f, "session stopped"),
        }
    }
}

impl std::error::Error for HpsdrError {}

impl From<std::io::Error> for HpsdrError {
    // [@ANCHOR: hpsdr_error_from_io]
    /// Wraps an I/O error.
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_from_io() {
        // Tests [@ANCHOR: hpsdr_error_fmt]
        // Tests [@ANCHOR: hpsdr_error_from_io]
        assert_eq!(
            HpsdrError::GateClosed.to_string(),
            "transmit gate is closed"
        );
        assert!(HpsdrError::Unsupported("x".into())
            .to_string()
            .contains('x'));
        assert!(HpsdrError::Config("y".into()).to_string().contains('y'));
        for e in [
            HpsdrError::Truncated,
            HpsdrError::BadMagic,
            HpsdrError::Timeout,
            HpsdrError::Stopped,
        ] {
            assert!(!e.to_string().is_empty());
        }
        let io = std::io::Error::other("boom");
        assert_eq!(HpsdrError::from(io), HpsdrError::Io("boom".into()));
    }
}

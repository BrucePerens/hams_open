// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared plumbing for the vendor serial engines of [`Family::SerialVendor`] (Ten-Tec, Drake,
//! Racal, ...): the receive buffer with the stale-partial rule, decimal-digit parsing, and the
//! dispatch from a description's `vendor.protocol` to its engine and virtual twin.
//!
//! The virtual twins live in each engine's file (as `VirtualFlex` does); they implement
//! [`VendorTwin`], so the conformance harness drives every family the same way.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription, VendorProtocol};
use radioctl_core::engine::{ProtocolEngine, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::transcript::Panel;

/// A virtual radio of one vendor serial family.
pub trait VendorTwin: Send {
    /// Consumes the controller's bytes at `now`; returns what the radio sends back.
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>;
    /// Turns a knob at the radio; `Err` names a knob this twin has not got.
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>;
}

/// Receive buffer that drops a partial frame older than [`PARTIAL_FRAME_TIMEOUT`].
#[derive(Debug, Default)]
pub struct RxBuffer {
    bytes: Vec<u8>,
    last_at: Duration,
}

impl RxBuffer {
    // [@ANCHOR: radioctl_vendor_rxbuffer_push]
    /// Appends `bytes` received at `now`. Returns the stale partial frame if the buffer held
    /// one older than the timeout (it is discarded, not kept).
    pub fn push(&mut self, bytes: &[u8], now: Duration) -> Option<Vec<u8>> {
        let stale =
            if !self.bytes.is_empty() && now.saturating_sub(self.last_at) > PARTIAL_FRAME_TIMEOUT {
                Some(std::mem::take(&mut self.bytes))
            } else {
                None
            };
        self.last_at = now;
        self.bytes.extend_from_slice(bytes);
        stale
    }

    // [@ANCHOR: radioctl_vendor_rxbuffer_take_line]
    /// Removes and returns the first line up to and excluding `terminator`.
    pub fn take_line(&mut self, terminator: u8) -> Option<Vec<u8>> {
        let end = self.bytes.iter().position(|b| *b == terminator)?;
        let mut line: Vec<u8> = self.bytes.drain(..=end).collect();
        line.pop();
        Some(line)
    }

    // [@ANCHOR: radioctl_vendor_rxbuffer_take]
    /// Removes and returns the first `n` bytes if that many have arrived.
    pub fn take(&mut self, n: usize) -> Option<Vec<u8>> {
        (self.bytes.len() >= n).then(|| self.bytes.drain(..n).collect())
    }

    // [@ANCHOR: radioctl_vendor_rxbuffer_peek]
    /// The bytes held so far.
    pub fn peek(&self) -> &[u8] {
        &self.bytes
    }

    // [@ANCHOR: radioctl_vendor_rxbuffer_take_all]
    /// Removes and returns everything held.
    pub fn take_all(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

// [@ANCHOR: radioctl_vendor_parse_decimal]
/// The value of a non-empty run of ASCII digits (at most 18 of them), or `None`.
pub fn parse_decimal(text: &[u8]) -> Option<u64> {
    if text.is_empty() || text.len() > 18 || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    text.iter().try_fold(0u64, |acc, d| {
        acc.checked_mul(10)?.checked_add(u64::from(d - b'0'))
    })
}

// [@ANCHOR: radioctl_vendor_vendor_of]
/// The vendor protocol of a description, or why it has none.
pub fn vendor_of(desc: &RigDescription) -> Result<(VendorProtocol, &str), RigError> {
    match (&desc.family, &desc.vendor) {
        (Family::SerialVendor, Some(v)) => Ok((v.protocol, v.dialect.as_str())),
        _ => Err(RigError::new(
            ErrorKind::Unsupported,
            "not a vendor serial description",
        )),
    }
}

// [@ANCHOR: radioctl_vendor_engine_for]
/// The engine of a vendor serial description.
pub fn engine_for(desc: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError> {
    Ok(match vendor_of(desc)?.0 {
        VendorProtocol::Tentec => Box::new(crate::tentec::TentecEngine::new(desc.clone())?),
    })
}

// [@ANCHOR: radioctl_vendor_twin_for]
/// The virtual radio of a vendor serial description.
pub fn twin_for(desc: &RigDescription) -> Result<Box<dyn VendorTwin>, RigError> {
    Ok(match vendor_of(desc)?.0 {
        VendorProtocol::Tentec => Box::new(crate::tentec::TentecTwin::new(desc)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    // Tests [@ANCHOR: radioctl_vendor_rxbuffer_push]
    // Tests [@ANCHOR: radioctl_vendor_rxbuffer_take_line]
    // Tests [@ANCHOR: radioctl_vendor_rxbuffer_take]
    // Tests [@ANCHOR: radioctl_vendor_rxbuffer_peek]
    // Tests [@ANCHOR: radioctl_vendor_rxbuffer_take_all]
    #[test]
    fn the_buffer_splits_lines_counts_bytes_and_drops_stale_partials() {
        let mut b = RxBuffer::default();
        assert_eq!(b.push(b"ab\rc", T0), None);
        assert_eq!(b.take_line(b'\r'), Some(b"ab".to_vec()));
        assert_eq!(b.take_line(b'\r'), None);
        assert_eq!(b.peek(), b"c");
        assert_eq!(b.take(2), None);
        let later = T0 + PARTIAL_FRAME_TIMEOUT + Duration::from_millis(1);
        assert_eq!(b.push(b"d", later), Some(b"c".to_vec()));
        assert_eq!(b.take(1), Some(b"d".to_vec()));
        b.push(b"xyz", later);
        assert_eq!(b.take_all(), b"xyz");
        assert!(b.peek().is_empty());
    }

    // Tests [@ANCHOR: radioctl_vendor_parse_decimal]
    #[test]
    fn decimals_are_digits_only_and_bounded() {
        assert_eq!(parse_decimal(b"14200000"), Some(14_200_000));
        assert_eq!(parse_decimal(b"01800000"), Some(1_800_000));
        assert_eq!(parse_decimal(b""), None);
        assert_eq!(parse_decimal(b"12a"), None);
        assert_eq!(parse_decimal(b"-5"), None);
        assert_eq!(parse_decimal(b"1234567890123456789"), None);
    }

    // Tests [@ANCHOR: radioctl_vendor_vendor_of]
    // Tests [@ANCHOR: radioctl_vendor_engine_for]
    // Tests [@ANCHOR: radioctl_vendor_twin_for]
    #[test]
    fn other_families_are_refused() {
        let desc = RigDescription::ic7300();
        assert!(vendor_of(&desc).is_err());
        assert!(engine_for(&desc).is_err());
        assert!(twin_for(&desc).is_err());
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! USB transfers to the VNWA: vendor control transfers on endpoint 0 only (`docs/PROTOCOL.md`). Nothing here claims an
//! interface, resets the device, or sends anything that is not a `Request` built by this crate, and no firmware is touched.
//!
//! A transfer that fails with a transient error (the intermittent EPROTO, a stall, a timeout) is sent again a bounded number
//! of times; every resend is counted, so a caller can see that the link is unhealthy. A vanished unit is reported at once.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::protocol::Request;
use crate::transport::{RusbTransport, TransferError, Transport};
use crate::{PID, VID};

/// How many times one transfer is sent again after a transient error.
pub const TRANSFER_RETRIES: u32 = 3;
/// Short on purpose: the unit starts emitting when it has the set-up block and the 0x0d read, and a long pause before a resend
/// would shift the analysis window of a measurement (the pause was 50 ms and made a third of the points unsteady).
pub const RETRY_PAUSE: Duration = Duration::from_millis(5);

pub struct Vnwa {
    link: Box<dyn Transport>,
    resends: AtomicU32,
}

impl Vnwa {
    /// Opens the first VNWA found on the USB bus.
    pub fn open() -> Result<Vnwa, String> {
        Ok(Vnwa::with_transport(Box::new(RusbTransport::open(
            VID, PID,
        )?)))
    }

    /// A unit over any transport (the libusb one in production).
    pub fn with_transport(link: Box<dyn Transport>) -> Vnwa {
        Vnwa {
            link,
            resends: AtomicU32::new(0),
        }
    }

    /// How many transfers were sent again after a transient error since this handle was opened.
    pub fn resends(&self) -> u32 {
        self.resends.load(Ordering::Relaxed)
    }

    /// Runs `op` until it succeeds, fails for good, or the retry bound is used up.
    fn attempt<T>(
        &self,
        what: &str,
        mut op: impl FnMut() -> Result<T, TransferError>,
    ) -> Result<T, String> {
        // [@ANCHOR: vnwa_open_transfer_retry]
        let mut last = String::new();
        for n in 0..=TRANSFER_RETRIES {
            match op() {
                Ok(v) => return Ok(v),
                Err(TransferError::Transient(m)) => {
                    last = m;
                    self.resends.fetch_add(1, Ordering::Relaxed);
                    if n < TRANSFER_RETRIES {
                        std::thread::sleep(RETRY_PAUSE);
                    }
                }
                Err(e) => return Err(format!("{what}: {e}")),
            }
        }
        Err(format!(
            "{what}: still failing after {TRANSFER_RETRIES} resends ({last})"
        ))
    }

    /// One host-to-device vendor request.
    pub fn send(&self, r: &Request) -> Result<(), String> {
        let what = format!("request {:#04x}", r.request);
        let sent = self.attempt(&what, || {
            self.link
                .write(r.request_type, r.request, r.value, r.index, &r.data)
        })?;
        if sent != r.data.len() {
            return Err(format!("{what}: {sent} of {} bytes sent", r.data.len()));
        }
        Ok(())
    }

    /// The requests of a block, in order.
    pub fn send_all(&self, block: &[Request]) -> Result<(), String> {
        block.iter().try_for_each(|r| self.send(r))
    }

    /// A device-to-host vendor request of at most `len` bytes (for example 0x3a, the generator's read-back).
    pub fn read(&self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; len];
        let got = self.attempt(&format!("read request {request:#04x}"), || {
            self.link.read(request, value, index, &mut buf)
        })?;
        buf.truncate(got);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Scripted, Seen};
    use std::sync::Arc;

    struct Shared(Arc<Scripted>);
    impl Transport for Shared {
        fn write(&self, t: u8, r: u8, v: u16, i: u16, d: &[u8]) -> Result<usize, TransferError> {
            self.0.write(t, r, v, i, d)
        }
        fn read(&self, r: u8, v: u16, i: u16, b: &mut [u8]) -> Result<usize, TransferError> {
            self.0.read(r, v, i, b)
        }
    }

    fn unit(outcomes: Vec<Result<Vec<u8>, TransferError>>) -> (Vnwa, Arc<Scripted>) {
        let s = Arc::new(Scripted::new(outcomes));
        (Vnwa::with_transport(Box::new(Shared(s.clone()))), s)
    }

    fn eproto() -> Result<Vec<u8>, TransferError> {
        Err(TransferError::Transient("Input/Output Error".into()))
    }

    // Tests [@ANCHOR: vnwa_open_transfer_retry]
    #[test]
    fn a_transient_error_is_sent_again_and_counted() {
        let (u, s) = unit(vec![eproto(), eproto(), Ok(vec![1, 2])]);
        assert_eq!(u.read(0x3a, 0, 0, 8).unwrap(), vec![1, 2]);
        assert_eq!(u.resends(), 2);
        assert_eq!(s.seen().len(), 3);
        assert!(s.seen().iter().all(|x| *x
            == Seen {
                write: false,
                request: 0x3a
            }));
    }

    #[test]
    fn a_transfer_that_keeps_failing_is_a_clear_error_after_the_bound() {
        let (u, s) = unit((0..10).map(|_| eproto()).collect());
        let e = u.read(0x58, 0, 0, 64).unwrap_err();
        assert!(e.contains("still failing after 3 resends"), "{e}");
        assert_eq!(s.seen().len(), 1 + TRANSFER_RETRIES as usize);
    }

    #[test]
    fn a_vanished_unit_is_reported_without_resending() {
        let (u, s) = unit(vec![Err(TransferError::NoDevice("No such device".into()))]);
        let e = u.read(0x58, 0, 0, 64).unwrap_err();
        assert!(e.contains("No such device"), "{e}");
        assert_eq!(s.seen().len(), 1);
        assert_eq!(u.resends(), 0);
    }

    // Tests [@ANCHOR: vnwa_open_transfer_classes]
    #[test]
    fn libusb_errors_are_classified() {
        assert!(matches!(
            crate::transport::classify(rusb::Error::Io),
            TransferError::Transient(_)
        ));
        assert!(matches!(
            crate::transport::classify(rusb::Error::Pipe),
            TransferError::Transient(_)
        ));
        assert!(matches!(
            crate::transport::classify(rusb::Error::NoDevice),
            TransferError::NoDevice(_)
        ));
        assert!(matches!(
            crate::transport::classify(rusb::Error::Access),
            TransferError::Other(_)
        ));
    }
}

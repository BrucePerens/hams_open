// SPDX-License-Identifier: AGPL-3.0-or-later
//! USB transfers to the VNWA: vendor control transfers on endpoint 0 only (`docs/PROTOCOL.md`). Nothing here claims an
//! interface, resets the device, or sends anything that is not a `Request` built by this crate, and no firmware is touched.
//!
//! A transfer that fails with a transient error (the intermittent EPROTO, a stall, a timeout) is sent again a bounded number
//! of times; every resend is counted, so a caller can see that the link is unhealthy. A vanished unit is reported at once.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::protocol::Request;
use crate::transport::{RusbTransport, TransferError, Transport};
use crate::{PID, VID};

/// How many times one transfer is sent again after a transient error.
pub const TRANSFER_RETRIES: u32 = 3;
/// Short on purpose: the unit starts emitting when it has the set-up block and the 0x0d read, and a long pause before a resend
/// would shift the analysis window of a measurement (the pause was 50 ms and made a third of the points unsteady).
pub const RETRY_PAUSE: Duration = Duration::from_millis(5);

/// How many transfers `Vnwa::recent_transfers` remembers.
pub const TRANSFER_LOG: usize = 16;

pub struct Vnwa {
    link: Box<dyn Transport>,
    resends: AtomicU32,
    /// The last `TRANSFER_LOG` transfer tries with their outcome, for the evidence record of a failure.
    recent: Mutex<VecDeque<String>>,
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
            recent: Mutex::new(VecDeque::new()),
        }
    }

    /// How many transfers were sent again after a transient error since this handle was opened.
    pub fn resends(&self) -> u32 {
        self.resends.load(Ordering::Relaxed)
    }

    /// The last transfer tries, oldest first: epoch milliseconds, what, and the outcome (the error text of a failed one).
    pub fn recent_transfers(&self) -> Vec<String> {
        // [@ANCHOR: vnwa_open_transfer_log]
        self.recent.lock().unwrap().iter().cloned().collect()
    }

    fn remember(&self, what: &str, outcome: &str) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let mut r = self.recent.lock().unwrap();
        if r.len() >= TRANSFER_LOG {
            r.pop_front();
        }
        r.push_back(format!("{ms} {what}: {outcome}"));
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
                Ok(v) => {
                    self.remember(what, "ok");
                    return Ok(v);
                }
                Err(TransferError::Transient(m)) => {
                    self.remember(what, &format!("{m} (transient, try {})", n + 1));
                    last = m;
                    self.resends.fetch_add(1, Ordering::Relaxed);
                    if n < TRANSFER_RETRIES {
                        std::thread::sleep(RETRY_PAUSE);
                    }
                }
                Err(e) => {
                    self.remember(what, &format!("{e}"));
                    return Err(format!("{what}: {e}"));
                }
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

    /// Like `read`, but a transient error is followed by the given pauses (milliseconds), one per resend, instead of the default
    /// `TRANSFER_RETRIES` of `RETRY_PAUSE`. For the 0x0d read, which the unit stalls while it is busy.
    pub fn read_patient(
        &self,
        request: u8,
        value: u16,
        index: u16,
        len: usize,
        pauses_ms: &[u64],
    ) -> Result<Vec<u8>, String> {
        // [@ANCHOR: vnwa_open_patient_read]
        let mut buf = vec![0u8; len];
        let what = format!("read request {request:#04x}");
        let mut last = String::new();
        for n in 0..=pauses_ms.len() {
            match self.link.read(request, value, index, &mut buf) {
                Ok(got) => {
                    self.remember(&what, "ok");
                    buf.truncate(got);
                    return Ok(buf);
                }
                Err(TransferError::Transient(m)) => {
                    self.remember(&what, &format!("{m} (transient, try {})", n + 1));
                    self.resends.fetch_add(1, Ordering::Relaxed);
                    last = m;
                    if let Some(p) = pauses_ms.get(n) {
                        std::thread::sleep(Duration::from_millis(*p));
                    }
                }
                Err(e) => {
                    self.remember(&what, &format!("{e}"));
                    return Err(format!("{what}: {e}"));
                }
            }
        }
        Err(format!(
            "{what}: still failing after {} resends ({last})",
            pauses_ms.len()
        ))
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

    // Tests [@ANCHOR: vnwa_open_transfer_log]
    #[test]
    fn the_last_transfer_tries_are_kept_with_their_outcome() {
        let (u, _) = unit(vec![
            eproto(),
            Ok(vec![1]),
            Err(TransferError::NoDevice("gone".into())),
        ]);
        u.read(0x3a, 0, 0, 2).unwrap();
        u.read(0x58, 0, 0, 2).unwrap_err();
        let log = u.recent_transfers();
        assert_eq!(log.len(), 3);
        assert!(
            log[0].contains("0x3a") && log[0].contains("transient, try 1"),
            "{log:?}"
        );
        assert!(log[1].ends_with(": ok"), "{log:?}");
        assert!(
            log[2].contains("0x58") && log[2].contains("gone"),
            "{log:?}"
        );
        for _ in 0..40 {
            let _ = u.read(0x3a, 0, 0, 2);
        }
        assert_eq!(u.recent_transfers().len(), TRANSFER_LOG);
    }

    #[test]
    fn a_transfer_that_keeps_failing_is_a_clear_error_after_the_bound() {
        let (u, s) = unit((0..10).map(|_| eproto()).collect());
        let e = u.read(0x58, 0, 0, 64).unwrap_err();
        assert!(e.contains("still failing after 3 resends"), "{e}");
        assert_eq!(s.seen().len(), 1 + TRANSFER_RETRIES as usize);
    }

    // Tests [@ANCHOR: vnwa_open_patient_read]
    #[test]
    fn a_patient_read_follows_its_own_pauses_and_gives_up_after_the_last() {
        let (u, s) = unit(vec![
            eproto(),
            eproto(),
            eproto(),
            eproto(),
            eproto(),
            Ok(vec![7]),
        ]);
        let t = std::time::Instant::now();
        assert_eq!(
            u.read_patient(0x0d, 0x0303, 0, 64, &[1, 1, 1, 1, 1])
                .unwrap(),
            vec![7]
        );
        assert_eq!(s.seen().len(), 6);
        assert_eq!(u.resends(), 5);
        assert!(t.elapsed() >= Duration::from_millis(5));
        let (u, s) = unit((0..10).map(|_| eproto()).collect());
        let e = u.read_patient(0x0d, 0x0303, 0, 64, &[1, 1]).unwrap_err();
        assert!(e.contains("still failing after 2 resends"), "{e}");
        assert_eq!(s.seen().len(), 3);
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

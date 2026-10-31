// SPDX-License-Identifier: AGPL-3.0-or-later
//! Keying a transmitter with a serial control line (RTS or DTR), behind the rig's transmit gate.
//!
//! Keying is transmit-capable, so it is refused unless the rig's transmit gate is open (the same
//! gate that guards `SetPtt` and CW); releasing is always allowed and never waits. The line is an
//! immediate control-line call, not a queued command: it does not wait behind a pending read.
//! A [`LineKeyer`] makes the line inactive when it is built.

use crate::error::{ErrorKind, RigError};
use crate::rig::RigHandle;

/// The serial control lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLine {
    /// Request To Send.
    Rts,
    /// Data Terminal Ready.
    Dtr,
}

/// Something with serial control lines. A serial link implements it; a keyer works on any.
pub trait ControlLines: Send + Sync {
    /// Sets a line to the given electrical level (`true` is asserted).
    fn set_line(&self, line: KeyLine, asserted: bool) -> Result<(), RigError>;
}

/// Keys a transmitter through one control line.
#[derive(Debug)]
pub struct LineKeyer<C: ControlLines> {
    lines: C,
    line: KeyLine,
    active_high: bool,
}

impl<C: ControlLines> LineKeyer<C> {
    // [@ANCHOR: radioctl_core_keyer_new]
    /// A keyer on `line`; `active_high` says whether keying asserts the line. The line is made
    /// inactive at once, so a failure to do that is reported rather than hidden.
    pub fn new(lines: C, line: KeyLine, active_high: bool) -> Result<LineKeyer<C>, RigError> {
        let keyer = LineKeyer {
            lines,
            line,
            active_high,
        };
        keyer.lines.set_line(line, !active_high)?;
        Ok(keyer)
    }

    // [@ANCHOR: radioctl_core_keyer_key]
    /// Keys the transmitter. Refused with [`ErrorKind::TransmitGateClosed`] unless `rig`'s gate
    /// is open; nothing reaches the line in that case.
    pub fn key(&self, rig: &RigHandle) -> Result<(), RigError> {
        if rig.transmit_permit().is_none() {
            return Err(RigError::new(
                ErrorKind::TransmitGateClosed,
                "transmit gate is closed for this rig",
            ));
        }
        self.lines.set_line(self.line, self.active_high)
    }

    // [@ANCHOR: radioctl_core_keyer_release]
    /// Unkeys the transmitter. Always allowed.
    pub fn release(&self) -> Result<(), RigError> {
        self.lines.set_line(self.line, !self.active_high)
    }
}

impl<C: ControlLines> Drop for LineKeyer<C> {
    // [@ANCHOR: radioctl_core_keyer_drop]
    /// A keyer that goes away (its holder returned, panicked or was dropped while keyed) makes
    /// the line inactive, so RF never stays on the air for want of a release call.
    fn drop(&mut self) {
        let _ = self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<(KeyLine, bool)>>>);

    impl ControlLines for Recorder {
        fn set_line(&self, line: KeyLine, asserted: bool) -> Result<(), RigError> {
            self.0
                .lock()
                .map_err(|_| RigError::new(ErrorKind::LinkDown, "poisoned"))?
                .push((line, asserted));
            Ok(())
        }
    }

    // Tests [@ANCHOR: radioctl_core_keyer_new]
    // Tests [@ANCHOR: radioctl_core_keyer_release]
    // Tests [@ANCHOR: radioctl_core_keyer_drop]
    #[test]
    fn a_new_keyer_makes_its_line_inactive_and_release_and_drop_do_too() {
        let high = Recorder::default();
        let keyer = LineKeyer::new(high.clone(), KeyLine::Rts, true).expect("keyer");
        keyer.release().expect("release");
        drop(keyer);
        let low = Recorder::default();
        let keyer = LineKeyer::new(low.clone(), KeyLine::Dtr, false).expect("keyer");
        keyer.release().expect("release");
        drop(keyer);
        let calls = high.0.lock().expect("lock").clone();
        assert_eq!(
            calls,
            vec![
                (KeyLine::Rts, false),
                (KeyLine::Rts, false),
                (KeyLine::Rts, false)
            ]
        );
        let calls = low.0.lock().expect("lock").clone();
        assert_eq!(
            calls,
            vec![
                (KeyLine::Dtr, true),
                (KeyLine::Dtr, true),
                (KeyLine::Dtr, true)
            ]
        );
    }

    // Tests [@ANCHOR: radioctl_core_keyer_drop]
    // Tests [@ANCHOR: radioctl_core_keyer_key]
    #[test]
    fn a_keyer_dropped_while_keyed_releases_the_line() {
        let lines = Recorder::default();
        let keyer = LineKeyer::new(lines.clone(), KeyLine::Rts, true).expect("keyer");
        // Key through the line directly (the gate check needs a rig; it is tested with one).
        keyer.lines.set_line(KeyLine::Rts, true).expect("key");
        drop(keyer);
        let last = lines.0.lock().expect("lock").last().copied();
        assert_eq!(last, Some((KeyLine::Rts, false)));
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Microtelecom Perseus receiver board: sans-IO client engine for the `perseus` USB protocol.
//!
//! **Sources.** None beyond [`crate::perseus`] (the public libperseus-sdr headers, facts only).
//! Unverified on a board.
//!
//! The engine does not speak to the board. It encodes each operation as one control transfer to
//! the [`crate::perseus::PerseusSession`] that the loader backend opened, which owns the board's
//! front-end state and the multi-step bulk sequences:
//!
//! * set frequency: tune (`SESSION_TUNE`), the preselector following the tuned frequency;
//! * `Level::Attenuator` in 0, 10, 20 or 30 dB (`levels.attenuator_db` of the description);
//! * `Level::Preamp` 0 or 1: the ADC preamplifier;
//! * `Level::Dither` 0 or 1: ADC dither;
//! * `Level::Preselector` 0 or 1: 1 follows the tuned frequency with the band filters, 0 is the
//!   wide-band path.
//!
//! The board cannot be read back, so no read operation is encoded (the daemon answers from the
//! values it set). There is **no transmit path**: the description carries no PTT or CW
//! capability and every transmitting operation is refused before anything is encoded.

use std::time::Duration;

use radioctl_core::description::{RigDescription, UsbProtocol};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::units::{Level, Vfo};
use radioctl_core::usb_wire::{FrameError, UsbReply, UsbStatus, UsbTransfer};

use crate::perseus::{
    MAX_FREQUENCY_HZ, SESSION_ATTENUATOR, SESSION_DITHER, SESSION_PREAMP, SESSION_PRESELECTOR,
    SESSION_REQUEST_TYPE, SESSION_TUNE,
};

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

/// The Perseus client engine.
#[derive(Debug)]
pub struct PerseusEngine {
    desc: RigDescription,
    waiting: bool,
    rx: Vec<u8>,
}

impl PerseusEngine {
    // [@ANCHOR: radioctl_perseus_engine_new]
    /// Builds an engine from a `perseus` USB description.
    pub fn new(desc: RigDescription) -> Result<PerseusEngine, RigError> {
        match desc.usb.as_ref().map(|u| u.protocol) {
            Some(UsbProtocol::Perseus) => Ok(PerseusEngine {
                desc,
                waiting: false,
                rx: Vec::new(),
            }),
            _ => Err(unsupported("not a Perseus description")),
        }
    }

    // [@ANCHOR: radioctl_perseus_engine_transfer]
    fn transfer(request: u8, value: u16, data: Vec<u8>) -> Vec<u8> {
        UsbTransfer::ControlOut {
            request_type: SESSION_REQUEST_TYPE,
            request,
            value,
            index: 0,
            data,
        }
        .encode()
    }
}

impl ProtocolEngine for PerseusEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_perseus_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if op.transmits() || matches!(op, Op::SetPtt(_)) {
            return Err(unsupported("the Perseus is a receiver and cannot transmit"));
        }
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let bytes = match op {
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 as f64 > MAX_FREQUENCY_HZ {
                    return Err(unsupported("the Perseus tunes 0 to 40 MHz"));
                }
                Self::transfer(SESSION_TUNE, 0, hz.0.to_le_bytes().to_vec())
            }
            Op::SetLevel(Level::Attenuator, db) => {
                crate::attenuation::step_of(&self.desc, Level::Attenuator, *db)?;
                Self::transfer(SESSION_ATTENUATOR, *db, Vec::new())
            }
            Op::SetLevel(Level::Preamp, step) => {
                crate::attenuation::step_of(&self.desc, Level::Preamp, *step)?;
                Self::transfer(SESSION_PREAMP, *step, Vec::new())
            }
            Op::SetLevel(Level::Dither, on) if *on <= 1 => {
                Self::transfer(SESSION_DITHER, *on, Vec::new())
            }
            Op::SetLevel(Level::Preselector, on) if *on <= 1 => {
                Self::transfer(SESSION_PRESELECTOR, *on, Vec::new())
            }
            Op::SetLevel(Level::Dither | Level::Preselector, _) => {
                return Err(unsupported("this level is 0 or 1"))
            }
            _ => {
                return Err(unsupported(
                    "not available on the Perseus (receive only, write only)",
                ))
            }
        };
        self.waiting = true;
        self.rx.clear();
        Ok(Request {
            bytes,
            reply: ReplyKind::Ack,
        })
    }

    // [@ANCHOR: radioctl_perseus_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        self.rx.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let (reply, used) = match UsbReply::decode(&self.rx) {
                Ok(found) => found,
                Err(FrameError::Incomplete) => break,
                Err(FrameError::Malformed) => {
                    out.push(Inbound::Unrecognised(std::mem::take(&mut self.rx)));
                    break;
                }
            };
            self.rx.drain(..used);
            if !std::mem::replace(&mut self.waiting, false) {
                out.push(Inbound::Unrecognised(reply.encode()));
                continue;
            }
            match reply.status {
                UsbStatus::Timeout | UsbStatus::NoDevice => {}
                UsbStatus::Ok => out.push(Inbound::Ack),
                _ => out.push(Inbound::Nak),
            }
        }
        out
    }
}

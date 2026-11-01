// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared parts of the receiver and scanner engines (AOR, Uniden, JRC, Icom PCR, Icom marine,
//! Rohde and Schwarz): a line-oriented sans-IO engine that a [`Dialect`] fills in, and the line
//! side of the virtual radios ([`VirtualReceiver`]).
//!
//! These radios share a shape: the controller sends one short command ended by a terminator and
//! the radio answers with one line, or with nothing. The dialect says how each [`Op`] is spelled
//! and how each answer is read; this module owns the buffering, the stale-partial-line rule, the
//! capability gate and the refusal convention (an answer that starts with `?` is a refusal).
//! Receivers have no transmit path: a dialect never encodes `SetPtt` or `SendCw` unless the
//! description lists the capability (only the transceivers of these families do).

use std::fmt::Debug;
use std::time::Duration;

use radioctl_core::description::{Family, ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;

use crate::virtual_rig::RigState;

/// How one family spells commands and reads answers.
pub trait Dialect: Debug + Send + 'static {
    /// The dialect of a description, or why it has none.
    fn from_description(desc: &RigDescription) -> Result<Self, RigError>
    where
        Self: Sized;

    /// The bytes and expected reply for an operation, or why the model cannot do it.
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError>;

    /// What one received line means, given the reply that was expected. `line` has no
    /// terminator and no line feed.
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound>;

    /// The byte that ends a received line.
    fn terminator(&self) -> u8 {
        b'\r'
    }

    /// True while the dialect expects more answer lines for the request it last decoded (a
    /// request that was sent as several commands).
    fn more_lines(&self) -> bool {
        false
    }

    /// The commands that make a freshly opened or reconnected receiver ready to talk, in order
    /// (an Icom PCR is powered on and told to stop its automatic updates). The default sends
    /// nothing. Never a command that transmits (a receiver cannot).
    // [@ANCHOR: radioctl_receiver_dialect_open_commands_default]
    fn open_commands(&mut self, _desc: &RigDescription) -> Vec<(Vec<u8>, ReplyKind)> {
        Vec::new()
    }

    /// Whether a line is a bare acknowledgement or refusal, before [`Dialect::decode`] sees it.
    /// The default: an empty line acknowledges a setting, a line starting `?` refuses.
    // [@ANCHOR: radioctl_receiver_dialect_status_default]
    fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        if line.is_empty() {
            return Some(if expecting == ReplyKind::Ack {
                vec![Inbound::Ack]
            } else {
                Vec::new()
            });
        }
        line.starts_with('?').then(|| vec![Inbound::Nak])
    }
}

/// A protocol engine for a line-oriented receiver.
#[derive(Debug)]
pub struct LineEngine<D: Dialect> {
    desc: RigDescription,
    dialect: D,
    expecting: ReplyKind,
    buffer: Vec<u8>,
    last_byte_at: Duration,
}

impl<D: Dialect> LineEngine<D> {
    // [@ANCHOR: radioctl_receiver_engine_new]
    /// Builds the engine of a [`Family::Receiver`] description of protocol `protocol`.
    pub fn new(
        desc: RigDescription,
        protocol: ReceiverProtocol,
    ) -> Result<LineEngine<D>, RigError> {
        let right = desc.family == Family::Receiver
            && desc
                .receiver
                .as_ref()
                .is_some_and(|r| r.protocol == protocol);
        if !right {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                format!("not a {} description", protocol.name()),
            ));
        }
        let dialect = D::from_description(&desc)?;
        Ok(LineEngine {
            desc,
            dialect,
            expecting: ReplyKind::None,
            buffer: Vec::new(),
            last_byte_at: Duration::ZERO,
        })
    }

    // [@ANCHOR: radioctl_receiver_engine_split_lines]
    /// Takes every complete line out of the buffer: the text without line feeds and spaces at
    /// the ends, one entry per terminator (so an empty entry is an empty line).
    fn split_lines(&mut self) -> Vec<String> {
        let term = self.dialect.terminator();
        let mut lines = Vec::new();
        while let Some(at) = self.buffer.iter().position(|b| *b == term) {
            let raw: Vec<u8> = self.buffer.drain(..=at).collect();
            let text: String = raw
                .iter()
                .take(raw.len() - 1)
                .filter(|b| **b != b'\n')
                .map(|b| char::from(*b))
                .collect();
            lines.push(text.trim().to_string());
        }
        lines
    }
}

impl<D: Dialect> ProtocolEngine for LineEngine<D> {
    // [@ANCHOR: radioctl_receiver_engine_description]
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_receiver_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "capability not in the rig description",
            ));
        }
        let (bytes, reply) = self.dialect.encode(&self.desc, op)?;
        self.expecting = reply;
        self.buffer.clear();
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_receiver_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if !self.buffer.is_empty() && now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT
        {
            out.push(Inbound::Unrecognised(std::mem::take(&mut self.buffer)));
        }
        self.last_byte_at = now;
        self.buffer.extend_from_slice(bytes);
        for line in self.split_lines() {
            let expecting = self.expecting;
            match self.dialect.status(expecting, &line) {
                Some(found) => out.extend(found),
                None => out.extend(self.dialect.decode(&self.desc, expecting, &line)),
            }
            if !self.dialect.more_lines() {
                self.expecting = ReplyKind::None;
            }
        }
        out
    }

    // [@ANCHOR: radioctl_receiver_engine_open_requests]
    fn open_requests(&mut self) -> Vec<Request> {
        let requests: Vec<Request> = self
            .dialect
            .open_commands(&self.desc)
            .into_iter()
            .map(|(bytes, reply)| Request { bytes, reply })
            .collect();
        // The actor writes and waits for them one at a time, so the answer being decoded is
        // always the one for the request just written; they share one reply kind.
        if let Some(last) = requests.last() {
            self.buffer.clear();
            self.expecting = last.reply;
        }
        requests
    }

    // [@ANCHOR: radioctl_receiver_engine_link_restored]
    fn link_restored(&mut self) -> Vec<Request> {
        // A half line or an unanswered request of the old link must not meet the new one.
        self.buffer.clear();
        self.expecting = ReplyKind::None;
        self.open_requests()
    }
}

/// How one family's radio answers a line, for [`VirtualReceiver`].
pub trait ReceiverTwin: Debug + Send {
    /// The bytes the radio sends for one command line (without its terminator), changing `state`
    /// as the command would; empty when it sends nothing.
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8>;

    /// The byte that ends a command line.
    fn terminator(&self) -> u8 {
        b'\r'
    }

    // [@ANCHOR: radioctl_receiver_twin_raw_default]
    /// For a protocol that is not line-oriented: the bytes the radio sends for these bytes the
    /// controller wrote, or `None` (the default) to have the line framing handle them.
    fn raw(&mut self, _state: &mut RigState, _bytes: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// The line side of a virtual receiver: collects bytes into command lines for a twin.
#[derive(Debug)]
pub struct VirtualReceiver {
    twin: Box<dyn ReceiverTwin>,
    buffer: Vec<u8>,
    last_byte_at: Duration,
}

impl VirtualReceiver {
    // [@ANCHOR: radioctl_receiver_virtual_new]
    /// The virtual radio of a description (the twin of its protocol).
    pub fn new(desc: &RigDescription) -> Result<VirtualReceiver, RigError> {
        let params = desc
            .receiver
            .as_ref()
            .ok_or_else(|| RigError::new(ErrorKind::Unsupported, "description lacks [receiver]"))?;
        let twin: Box<dyn ReceiverTwin> = match params.protocol {
            ReceiverProtocol::Aor if crate::ar7030::handles(desc) => {
                Box::new(crate::ar7030::Ar7030Twin::new(desc)?)
            }
            ReceiverProtocol::Aor if crate::aor_classic::handles(desc) => {
                Box::new(crate::aor_classic::AorClassicTwin::new(desc)?)
            }
            ReceiverProtocol::Aor => Box::new(crate::aor::AorTwin::new(desc)?),
            ReceiverProtocol::Uniden if crate::uniden_digital::handles(desc) => {
                Box::new(crate::uniden_digital::UnidenDigitalTwin::new(desc)?)
            }
            ReceiverProtocol::Uniden => Box::new(crate::uniden::UnidenTwin::new(desc)?),
            ReceiverProtocol::RohdeSchwarz if crate::rs_hf::handles(desc) => {
                Box::new(crate::rs_hf::RsHfTwin::new(desc)?)
            }
            ReceiverProtocol::RohdeSchwarz => Box::new(crate::rs::RsTwin::new(desc)?),
            ReceiverProtocol::IcomMarine => Box::new(crate::icom_marine::MarineTwin::new(desc)?),
            ReceiverProtocol::IcomPcr => Box::new(crate::icom_pcr::PcrTwin::new(desc)?),
            ReceiverProtocol::Jrc => Box::new(crate::jrc::JrcTwin::new(desc)?),
        };
        Ok(VirtualReceiver {
            twin,
            buffer: Vec::new(),
            last_byte_at: Duration::ZERO,
        })
    }

    // [@ANCHOR: radioctl_receiver_virtual_feed]
    /// Consumes bytes the controller wrote; the bytes the radio answers.
    pub fn feed(&mut self, bytes: &[u8], now: Duration, state: &mut RigState) -> Vec<u8> {
        if now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT {
            self.buffer.clear();
        }
        self.last_byte_at = now;
        if let Some(answer) = self.twin.raw(state, bytes) {
            return answer;
        }
        self.buffer.extend_from_slice(bytes);
        let term = self.twin.terminator();
        let mut out = Vec::new();
        while let Some(at) = self.buffer.iter().position(|b| *b == term) {
            let raw: Vec<u8> = self.buffer.drain(..=at).collect();
            let text: String = raw
                .iter()
                .take(raw.len() - 1)
                .filter(|b| **b != b'\n')
                .map(|b| char::from(*b))
                .collect();
            out.extend(self.twin.line(state, text.trim()));
        }
        out
    }
}

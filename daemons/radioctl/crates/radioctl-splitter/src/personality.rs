// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sans-IO interface between a wire protocol spoken to outside software and the splitter.
//!
//! A personality turns bytes from one client into [`Pending`] items (what the client wants), turns
//! the result of each into reply bytes, and turns rig changes into the unsolicited bytes that
//! client expects. It never touches the rig, a socket or a clock.

use std::time::Duration;

use radioctl_core::event::Change;
use radioctl_core::op::Observation;
use radioctl_core::units::{Hz, Mode, Vfo};

/// What a client asks of the rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Read a VFO's frequency.
    ReadFrequency(Vfo),
    /// Set a VFO's frequency.
    SetFrequency(Vfo, Hz),
    /// Read the mode.
    ReadMode,
    /// Set the mode.
    SetMode(Mode),
    /// Read the transmit state.
    ReadPtt,
    /// Key (`true`) or unkey (`false`).
    SetPtt(bool),
    /// Read the S-meter.
    ReadSignal,
    /// Read the power meter.
    ReadPower,
    /// Send Morse text.
    SendCw(String),
}

/// What the splitter did with a [`Request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A value (the answer to a read).
    Value(Observation),
    /// A set or key was done.
    Done,
    /// The request was refused (unsupported, gate closed, PTT owned by another client, failed).
    Refused,
}

/// What the splitter must do for one parsed item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Ask the rig; the personality's `reply` renders the outcome. `context` is the personality's
    /// own note about the item (the CI-V frame it answers).
    Rig {
        /// The request.
        request: Request,
        /// Opaque to the splitter, handed back to `reply`.
        context: Vec<u8>,
    },
    /// Answer locally with these bytes (identity, per-client flags, refusals of what the
    /// personality does not speak).
    Local(Vec<u8>),
    /// Nothing to do.
    Ignore,
}

/// One parsed item from a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// Bytes to send to the client at once (the CI-V bus echo of its own frame).
    pub echo: Vec<u8>,
    /// What to do.
    pub action: Action,
}

/// A wire protocol spoken to one client.
pub trait Personality: Send + 'static {
    /// Consumes bytes from the client at `now`.
    fn parse(&mut self, bytes: &[u8], now: Duration) -> Vec<Pending>;

    /// The bytes answering a rig-bound item, given its context and outcome.
    fn reply(&mut self, context: &[u8], request: &Request, outcome: &Outcome) -> Vec<u8>;

    /// The unsolicited bytes this client gets for a change of the rig (empty if none).
    fn unsolicited(&mut self, change: &Change) -> Vec<u8>;

    /// Forgets partial input and per-connection flags (the client disconnected).
    fn reset(&mut self);
}

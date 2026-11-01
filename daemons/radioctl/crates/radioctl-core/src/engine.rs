// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sans-IO protocol engine interface.
//!
//! An engine is a pure state machine: it turns an [`Op`] into the bytes to send and a statement
//! of what reply to expect, and turns received bytes into [`Inbound`] events. It never touches a
//! socket, a serial port, a runtime or a clock; the caller passes the time in as a [`Duration`]
//! since an arbitrary epoch of its own. That makes every engine replayable from a transcript
//! and fuzzable without a runtime.

use std::time::Duration;

use crate::description::RigDescription;
use crate::error::RigError;
use crate::op::{Field, Observation, Op};
use crate::scope::ScopeFrame;

/// A partial frame older than this when more bytes arrive is discarded as stale.
pub const PARTIAL_FRAME_TIMEOUT: Duration = Duration::from_millis(500);

/// What the actor should wait for after sending a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    /// Nothing comes back; the command is complete once written.
    None,
    /// An acknowledgement or a refusal.
    Ack,
    /// An observation of the given field.
    Observation(Field),
}

/// The bytes for one command and the reply to expect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Bytes to write to the transport.
    pub bytes: Vec<u8>,
    /// The reply to wait for.
    pub reply: ReplyKind,
}

/// Something decoded from received bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inbound {
    /// A value, whether asked for or sent unsolicited.
    Observation(Observation),
    /// A complete scope sweep. It is not a cache field: the actor publishes it on the scope
    /// frame stream (`seq` is assigned there).
    ScopeFrame(ScopeFrame),
    /// The rig accepted the last command.
    Ack,
    /// The rig refused the last command.
    Nak,
    /// Bytes that formed no known frame.
    Unrecognised(Vec<u8>),
}

/// A protocol engine for one rig. See the module note for the sans-IO contract.
pub trait ProtocolEngine: Send + 'static {
    /// The description the engine was built from.
    fn description(&self) -> &RigDescription;

    /// Encodes a command, or says why the rig cannot do it.
    fn encode(&mut self, op: &Op) -> Result<Request, RigError>;

    /// Consumes received bytes; `now` is the caller's monotonic time.
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>;

    /// The link to the rig came back after a loss. Whatever session state the engine had
    /// established on the old link (Kenwood and Elecraft `AI2;` auto-information, Flex
    /// subscriptions) is gone from the rig; the engine drops any half-received frame and
    /// unanswered request, and returns the requests that set the session up again, in order,
    /// or none if it had set nothing up. The actor writes them once, after the fail-closed
    /// unkey and before the link-restored event; the default sets up nothing.
    fn link_restored(&mut self) -> Vec<Request> {
        Vec::new()
    }
}

impl ProtocolEngine for Box<dyn ProtocolEngine> {
    // [@ANCHOR: radioctl_core_engine_box_description]
    /// The boxed engine's description, so one `spawn` serves every protocol family.
    fn description(&self) -> &RigDescription {
        self.as_ref().description()
    }

    // [@ANCHOR: radioctl_core_engine_box_encode]
    /// Encodes through the boxed engine.
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        self.as_mut().encode(op)
    }

    // [@ANCHOR: radioctl_core_engine_box_feed]
    /// Feeds the boxed engine.
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.as_mut().feed(bytes, now)
    }

    // [@ANCHOR: radioctl_core_engine_box_link_restored]
    /// Forwards the link-restored hook to the boxed engine.
    fn link_restored(&mut self) -> Vec<Request> {
        self.as_mut().link_restored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Hz;

    struct Echo(RigDescription);

    impl ProtocolEngine for Echo {
        fn description(&self) -> &RigDescription {
            &self.0
        }
        fn encode(&mut self, _op: &Op) -> Result<Request, RigError> {
            Ok(Request {
                bytes: b"x".to_vec(),
                reply: ReplyKind::None,
            })
        }
        fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
            vec![Inbound::Unrecognised(bytes.to_vec())]
        }
    }

    // Tests [@ANCHOR: radioctl_core_engine_box_description]
    // Tests [@ANCHOR: radioctl_core_engine_box_encode]
    // Tests [@ANCHOR: radioctl_core_engine_box_feed]
    // Tests [@ANCHOR: radioctl_core_engine_box_link_restored]
    #[test]
    fn a_boxed_engine_is_an_engine() {
        let mut boxed: Box<dyn ProtocolEngine> = Box::new(Echo(RigDescription::ic7300()));
        assert_eq!(ProtocolEngine::description(&boxed).id, "icom-ic-7300");
        let request = ProtocolEngine::encode(
            &mut boxed,
            &Op::SetFrequency(crate::units::Vfo::A, Hz(7_000_000)),
        )
        .expect("encode");
        assert_eq!(request.bytes, b"x");
        assert_eq!(
            ProtocolEngine::feed(&mut boxed, b"yz", Duration::ZERO),
            vec![Inbound::Unrecognised(b"yz".to_vec())]
        );
        assert!(
            ProtocolEngine::link_restored(&mut boxed).is_empty(),
            "an engine with no session state sets nothing up"
        );
    }
}

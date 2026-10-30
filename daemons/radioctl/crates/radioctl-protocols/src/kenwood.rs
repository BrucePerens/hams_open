// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kenwood-style ASCII: command tokenizer, sans-IO client engine.
//!
//! Commands are two letters, optional digits, and `;` (`FA00014074000;`). Setting commands get
//! no reply. A refusal is the single token `?;`. Sources: our own `tools/rig_simulator.py` and
//! the relay's `ts2000_bridge.rs`; no radio has verified this engine.
//!
//! Not implemented, on purpose: reading the transmit state. On a TS-2000 it comes from the
//! composite `IF;` answer, whose field layout is not verified here, and the engine refuses
//! rather than guess a column. The TS-2000 description therefore lacks `ReadPtt`.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

/// Command terminator.
pub const TERMINATOR: u8 = b';';
/// Buffered bytes beyond this without a terminator are dropped.
const MAX_PENDING: usize = 256;
/// Largest frequency the 11-digit field holds.
const MAX_HZ: u64 = 99_999_999_999;

/// What the tokenizer found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A command or reply without its terminator (`FA00014074000`).
    Command(String),
    /// Bytes that were not a printable-ASCII command.
    Junk(Vec<u8>),
}

/// Splits a byte stream on `;`. Holds a partial command between calls.
#[derive(Debug, Default)]
pub struct Tokenizer {
    pending: Vec<u8>,
    last_byte_at: Duration,
}

impl Tokenizer {
    // [@ANCHOR: radioctl_kenwood_tokenizer_feed]
    /// Adds bytes received at `now` and returns every complete token.
    ///
    /// A partial command older than [`PARTIAL_FRAME_TIMEOUT`] is discarded as junk first.
    pub fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Token> {
        let mut out = Vec::new();
        let stale = now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT;
        if stale && !self.pending.is_empty() {
            out.push(Token::Junk(std::mem::take(&mut self.pending)));
        }
        self.last_byte_at = now;
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == TERMINATOR) {
            let raw: Vec<u8> = self.pending.drain(..=end).collect();
            let body = raw.get(..end).unwrap_or(&[]);
            let printable = body
                .iter()
                .all(|b| (0x20..0x7F).contains(b) && *b != TERMINATOR);
            let text = String::from_utf8_lossy(body).trim().to_string();
            if printable && !text.is_empty() {
                out.push(Token::Command(text));
            } else {
                out.push(Token::Junk(raw));
            }
        }
        if self.pending.len() > MAX_PENDING {
            out.push(Token::Junk(std::mem::take(&mut self.pending)));
        }
        out
    }
}

// [@ANCHOR: radioctl_kenwood_parse_digits]
/// Parses a string of ASCII digits only (no sign, no spaces).
pub fn parse_digits(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 18 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The Kenwood-style client engine for one rig.
#[derive(Debug)]
pub struct KenwoodEngine {
    desc: RigDescription,
    tokenizer: Tokenizer,
}

impl KenwoodEngine {
    // [@ANCHOR: radioctl_kenwood_engine_new]
    /// Builds an engine from a Kenwood-style description.
    pub fn new(desc: RigDescription) -> Result<KenwoodEngine, RigError> {
        if desc.family != Family::KenwoodAscii || desc.kenwood.is_none() {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a Kenwood description",
            ));
        }
        Ok(KenwoodEngine {
            desc,
            tokenizer: Tokenizer::default(),
        })
    }

    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
    }

    fn command(text: String, reply: ReplyKind) -> Request {
        Request {
            bytes: text.into_bytes(),
            reply,
        }
    }

    fn decode(&self, text: &str) -> Inbound {
        if text == "?" {
            return Inbound::Nak;
        }
        let observation = match (text.get(..2), text.get(2..)) {
            (Some("FA"), Some(digits)) if digits.len() == 11 => {
                parse_digits(digits).map(|hz| Observation::Frequency(Vfo::A, Hz(hz)))
            }
            (Some("FB"), Some(digits)) if digits.len() == 11 => {
                parse_digits(digits).map(|hz| Observation::Frequency(Vfo::B, Hz(hz)))
            }
            (Some("MD"), Some(code)) if code.len() == 1 => code
                .bytes()
                .next()
                .and_then(|c| self.desc.code_mode(c))
                .map(Observation::Mode),
            (Some("SM"), Some(rest)) if rest.len() == 5 && rest.starts_with('0') => rest
                .get(1..)
                .and_then(parse_digits)
                .and_then(|v| u16::try_from(v).ok())
                .map(Observation::Signal),
            _ => None,
        };
        match observation {
            Some(value) => Inbound::Observation(value),
            None => Inbound::Unrecognised(format!("{text};").into_bytes()),
        }
    }
}

impl ProtocolEngine for KenwoodEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_kenwood_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let none = ReplyKind::None;
        let request = match op {
            Op::GetFrequency(vfo) => {
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(
                    format!("{name};"),
                    ReplyKind::Observation(Field::Frequency(*vfo)),
                )
            }
            Op::SetFrequency(vfo, hz) => {
                if hz.0 > MAX_HZ {
                    return Err(Self::unsupported("frequency above 11 digits"));
                }
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                Self::command(format!("{name}{:011};", hz.0), none)
            }
            Op::GetMode => Self::command("MD;".into(), ReplyKind::Observation(Field::Mode)),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                Self::command(format!("MD{};", char::from(code)), none)
            }
            Op::SetPtt(true) => Self::command("TX;".into(), none),
            Op::SetPtt(false) => Self::command("RX;".into(), none),
            Op::GetSignal => Self::command("SM0;".into(), ReplyKind::Observation(Field::Signal)),
            Op::SetTransceive(on) => Self::command(format!("AI{};", u8::from(*on)), none),
            Op::GetPtt | Op::GetPower | Op::SendCw(_) => {
                return Err(Self::unsupported("not implemented for the Kenwood engine"));
            }
        };
        Ok(request)
    }

    // [@ANCHOR: radioctl_kenwood_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.tokenizer
            .feed(bytes, now)
            .into_iter()
            .map(|token| match token {
                Token::Command(text) => self.decode(&text),
                Token::Junk(junk) => Inbound::Unrecognised(junk),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn engine() -> KenwoodEngine {
        KenwoodEngine::new(RigDescription::ts2000()).expect("engine")
    }

    // Tests [@ANCHOR: radioctl_kenwood_parse_digits]
    #[test]
    fn digits_only() {
        assert_eq!(parse_digits("00014074000"), Some(14_074_000));
        assert_eq!(parse_digits("+1"), None);
        assert_eq!(parse_digits(""), None);
        assert_eq!(parse_digits("1 2"), None);
        assert_eq!(parse_digits("1234567890123456789"), None);
    }

    // Tests [@ANCHOR: radioctl_kenwood_tokenizer_feed]
    #[test]
    fn tokenizer_splits_joins_and_flags_junk() {
        let mut tok = Tokenizer::default();
        assert!(tok.feed(b"FA0001", T0).is_empty());
        assert_eq!(
            tok.feed(b"4074000;MD2;", T0),
            vec![
                Token::Command("FA00014074000".into()),
                Token::Command("MD2".into())
            ]
        );
        assert_eq!(
            tok.feed(&[0x01, b';'], T0),
            vec![Token::Junk(vec![0x01, b';'])]
        );
        assert_eq!(tok.feed(b";", T0), vec![Token::Junk(vec![b';'])]);
        assert!(tok.feed(b"FA", T0).is_empty());
        let later = T0 + PARTIAL_FRAME_TIMEOUT + Duration::from_millis(1);
        assert_eq!(
            tok.feed(b"MD2;", later),
            vec![Token::Junk(b"FA".to_vec()), Token::Command("MD2".into())]
        );
        let flood = vec![b'A'; MAX_PENDING + 1];
        assert!(matches!(
            tok.feed(&flood, later).as_slice(),
            [Token::Junk(_)]
        ));
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_new]
    #[test]
    fn engine_refuses_a_civ_description() {
        let err = KenwoodEngine::new(RigDescription::ic7300()).expect_err("must refuse");
        assert_eq!(err.kind, ErrorKind::Unsupported);
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_encode]
    #[test]
    fn encode_matches_the_ts2000_commands() {
        let mut engine = engine();
        let get = engine.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(get.bytes, b"FA;");
        let set = engine
            .encode(&Op::SetFrequency(Vfo::B, Hz(7_040_000)))
            .expect("set");
        assert_eq!(set.bytes, b"FB00007040000;");
        assert_eq!(set.reply, ReplyKind::None);
        assert_eq!(
            engine.encode(&Op::SetMode(Mode::Usb)).expect("mode").bytes,
            b"MD2;"
        );
        assert_eq!(engine.encode(&Op::SetPtt(true)).expect("tx").bytes, b"TX;");
        assert_eq!(engine.encode(&Op::SetPtt(false)).expect("rx").bytes, b"RX;");
        assert_eq!(engine.encode(&Op::GetSignal).expect("sm").bytes, b"SM0;");
        assert_eq!(
            engine.encode(&Op::SetTransceive(true)).expect("ai").bytes,
            b"AI1;"
        );
        assert!(engine.encode(&Op::GetPtt).is_err());
        assert!(engine.encode(&Op::SendCw("cq".into())).is_err());
        assert!(engine
            .encode(&Op::SetFrequency(Vfo::A, Hz(100_000_000_000)))
            .is_err());
    }

    // Tests [@ANCHOR: radioctl_kenwood_engine_feed]
    #[test]
    fn feed_decodes_replies_and_refusals() {
        let mut engine = engine();
        assert_eq!(
            engine.feed(b"FA00014074000;MD2;SM00012;?;", T0),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_074_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
                Inbound::Observation(Observation::Signal(12)),
                Inbound::Nak,
            ]
        );
        assert!(matches!(
            engine.feed(b"FA123;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            engine.feed(b"MD8;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            engine.feed(b"ID019;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-2000 CAT personality: what outside software sees when it talks to the virtual port as if
//! it were a Kenwood TS-2000.
//!
//! Commands: `FA`/`FB` read or set a VFO (11 digits), `MD` read or set the mode (the TS-2000
//! digit table of `RigDescription::ts2000`), `TX`/`RX` key and unkey, `SM0` S-meter, `ID` (019),
//! `AI` auto-information on or off for this client, `PS` (power is on). Settings get no reply; a
//! refusal or an unknown command is `?;`. With `AI1` the client is sent `FA`, `FB` and `MD` when
//! the rig changes.
//!
//! `IF` is answered with the 38-character composite status of the TS-2000 (see
//! [`Ts2000Personality::status_answer`] for the layout and its sources), built from the frequency
//! of VFO A, the mode, the transmit state, RIT, XIT and its offset and split; a rig that cannot
//! read RIT, XIT or split reports them off. It is the same layout `radioctl-protocols::kenwood`
//! decodes, so what this endpoint says, a radioctl Kenwood client reads back. Data modes map to
//! the sideband they sit on, and S-meter values pass through in the real rig's units.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::event::Change;
use radioctl_core::op::Observation;
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::kenwood::{parse_digits, Token, Tokenizer};

use crate::personality::{Action, Outcome, Pending, Personality, Request, Status};

/// The TS-2000 emulated by a CAT endpoint.
pub struct Ts2000Personality {
    modes: Vec<(Mode, u8)>,
    tokenizer: Tokenizer,
    auto_info: bool,
}

impl Ts2000Personality {
    // [@ANCHOR: radioctl_splitter_ts2000_new]
    /// A TS-2000 personality with auto-information off.
    pub fn new() -> Ts2000Personality {
        Ts2000Personality {
            modes: RigDescription::ts2000().mode_codes,
            tokenizer: Tokenizer::default(),
            auto_info: false,
        }
    }

    // [@ANCHOR: radioctl_splitter_ts2000_code]
    /// The TS-2000 mode digit for a mode; data modes use the sideband they sit on.
    fn code(&self, mode: Mode) -> Option<u8> {
        let mode = match mode {
            Mode::Data => Mode::Usb,
            Mode::DataR => Mode::Lsb,
            other => other,
        };
        self.modes.iter().find(|(m, _)| *m == mode).map(|(_, c)| *c)
    }

    fn mode_of(&self, code: u8) -> Option<Mode> {
        self.modes.iter().find(|(_, c)| *c == code).map(|(m, _)| *m)
    }

    fn rig(request: Request) -> Action {
        Action::Rig {
            request,
            context: Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_splitter_ts2000_status_answer]
    /// The `IF` answer for a status. Layout (37 characters and the `;`): `IF`, P1 frequency of the
    /// receive VFO (11 digits; always VFO A here), P2 step (4 digits, reported as 0000), P3 RIT/XIT
    /// offset (sign and 5 digits), P4 RIT, P5 XIT, P6 and P7 memory bank and channel (000), P8
    /// transmit (1) or receive (0), P9 mode digit, P10 function (0, VFO A), P11 scan (0), P12 split,
    /// P13 to P15 tone and tone number (0000). Sources: TS-2000 Instruction Manual B62-1221-10,
    /// appendix 21 `IF` (page 125 as transcribed, see `radioctl-protocols::kenwood`); offsets of the
    /// fields cross-checked against Hamlib 4.6.2 `rigs/kenwood/kenwood.c` (`kenwood_get_ptt`,
    /// `kenwood_get_mode`, `kenwood_get_rit`/`xit` in `kenwood_get_func`, `get_split_vfo_if`),
    /// facts only. `None` when the mode has no TS-2000 digit.
    fn status_answer(&self, status: &Status) -> Option<Vec<u8>> {
        let mode = self.code(status.mode)?;
        let sign = if status.rit_offset < 0 { '-' } else { '+' };
        Some(
            format!(
                "IF{:011}0000{sign}{:05}{}{}000{}{}00{}0000;",
                status.frequency.0,
                status.rit_offset.unsigned_abs(),
                u8::from(status.rit),
                u8::from(status.xit),
                u8::from(status.ptt),
                char::from(mode),
                u8::from(status.split),
            )
            .into_bytes(),
        )
    }

    // [@ANCHOR: radioctl_splitter_ts2000_classify]
    /// Turns one command (without its `;`) into an action.
    fn classify(&mut self, text: &str) -> Action {
        let refuse = || Action::Local(b"?;".to_vec());
        let (name, arg) = (text.get(..2).unwrap_or(""), text.get(2..).unwrap_or(""));
        let vfo = |name: &str| if name == "FA" { Vfo::A } else { Vfo::B };
        match (name, arg) {
            ("FA" | "FB", "") => Self::rig(Request::ReadFrequency(vfo(name))),
            ("FA" | "FB", digits) if digits.len() == 11 => match parse_digits(digits) {
                Some(hz) => Self::rig(Request::SetFrequency(vfo(name), Hz(hz))),
                None => refuse(),
            },
            ("MD", "") => Self::rig(Request::ReadMode),
            ("MD", code) => match code.as_bytes() {
                [byte] => match self.mode_of(*byte) {
                    Some(mode) => Self::rig(Request::SetMode(mode)),
                    None => refuse(),
                },
                _ => refuse(),
            },
            ("TX", "" | "0" | "1") => Self::rig(Request::SetPtt(true)),
            ("RX", "" | "0") => Self::rig(Request::SetPtt(false)),
            ("SM", "" | "0") => Self::rig(Request::ReadSignal),
            ("IF", "") => Self::rig(Request::ReadStatus),
            ("ID", "") => Action::Local(b"ID019;".to_vec()),
            ("PS", "") => Action::Local(b"PS1;".to_vec()),
            ("AI", "") => Action::Local(format!("AI{};", u8::from(self.auto_info)).into_bytes()),
            ("AI", flag @ ("0" | "1")) => {
                self.auto_info = flag == "1";
                Action::Ignore
            }
            _ => refuse(),
        }
    }
}

impl Default for Ts2000Personality {
    fn default() -> Ts2000Personality {
        Ts2000Personality::new()
    }
}

impl Personality for Ts2000Personality {
    // [@ANCHOR: radioctl_splitter_ts2000_parse]
    fn parse(&mut self, bytes: &[u8], now: Duration) -> Vec<Pending> {
        let tokens = self.tokenizer.feed(bytes, now);
        tokens
            .into_iter()
            .map(|token| Pending {
                echo: Vec::new(),
                action: match token {
                    Token::Command(text) => self.classify(&text),
                    Token::Junk(_) => Action::Local(b"?;".to_vec()),
                },
            })
            .collect()
    }

    // [@ANCHOR: radioctl_splitter_ts2000_reply]
    fn reply(&mut self, _context: &[u8], request: &Request, outcome: &Outcome) -> Vec<u8> {
        match (outcome, request) {
            (Outcome::Refused, _) => b"?;".to_vec(),
            (Outcome::Done, _) => Vec::new(),
            (Outcome::Value(Observation::Frequency(vfo, hz)), _) => {
                let name = if *vfo == Vfo::A { "FA" } else { "FB" };
                format!("{name}{:011};", hz.0).into_bytes()
            }
            (Outcome::Value(Observation::Mode(mode)), _) => match self.code(*mode) {
                Some(code) => format!("MD{};", char::from(code)).into_bytes(),
                None => b"?;".to_vec(),
            },
            (Outcome::Value(Observation::Signal(level)), _) => {
                format!("SM0{level:04};").into_bytes()
            }
            (Outcome::Status(status), Request::ReadStatus) => {
                self.status_answer(status).unwrap_or_else(|| b"?;".to_vec())
            }
            (Outcome::Value(_) | Outcome::Status(_), _) => b"?;".to_vec(),
        }
    }

    // [@ANCHOR: radioctl_splitter_ts2000_unsolicited]
    fn unsolicited(&mut self, change: &Change) -> Vec<u8> {
        if !self.auto_info {
            return Vec::new();
        }
        match change.new {
            Observation::Frequency(vfo, hz) => {
                let name = if vfo == Vfo::A { "FA" } else { "FB" };
                format!("{name}{:011};", hz.0).into_bytes()
            }
            Observation::Mode(mode) => self
                .code(mode)
                .map(|code| format!("MD{};", char::from(code)).into_bytes())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_splitter_ts2000_reset]
    fn reset(&mut self) {
        self.tokenizer = Tokenizer::default();
        self.auto_info = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::op::Field;

    const T0: Duration = Duration::from_secs(1);

    fn actions(p: &mut Ts2000Personality, text: &str) -> Vec<Action> {
        p.parse(text.as_bytes(), T0)
            .into_iter()
            .map(|pending| pending.action)
            .collect()
    }

    fn rig(request: Request) -> Action {
        Action::Rig {
            request,
            context: Vec::new(),
        }
    }

    // Tests [@ANCHOR: radioctl_splitter_ts2000_new]
    // Tests [@ANCHOR: radioctl_splitter_ts2000_parse]
    // Tests [@ANCHOR: radioctl_splitter_ts2000_classify]
    #[test]
    fn commands_become_rig_actions_local_answers_or_refusals() {
        let mut p = Ts2000Personality::new();
        assert_eq!(
            actions(&mut p, "FA;FB00007040000;MD;MD2;TX;RX;SM0;IF;"),
            vec![
                rig(Request::ReadFrequency(Vfo::A)),
                rig(Request::SetFrequency(Vfo::B, Hz(7_040_000))),
                rig(Request::ReadMode),
                rig(Request::SetMode(Mode::Usb)),
                rig(Request::SetPtt(true)),
                rig(Request::SetPtt(false)),
                rig(Request::ReadSignal),
                rig(Request::ReadStatus),
            ]
        );
        assert_eq!(
            actions(&mut p, "ID;PS;AI;"),
            vec![
                Action::Local(b"ID019;".to_vec()),
                Action::Local(b"PS1;".to_vec()),
                Action::Local(b"AI0;".to_vec()),
            ]
        );
        for refused in ["ZZ;", "FA123;", "MD8;", "MD22;", "FAabcdefghijk;", "TX9;"] {
            assert_eq!(
                actions(&mut p, refused),
                vec![Action::Local(b"?;".to_vec())],
                "{refused}"
            );
        }
        assert_eq!(actions(&mut p, "AI1;"), vec![Action::Ignore]);
        assert_eq!(
            actions(&mut p, "AI;"),
            vec![Action::Local(b"AI1;".to_vec())]
        );
        assert_eq!(
            p.parse(&[0x01, b';'], T0)
                .into_iter()
                .map(|x| x.action)
                .collect::<Vec<_>>(),
            vec![Action::Local(b"?;".to_vec())]
        );
        assert!(p.parse(b"FA0001", T0).is_empty());
    }

    // Tests [@ANCHOR: radioctl_splitter_ts2000_reply]
    // Tests [@ANCHOR: radioctl_splitter_ts2000_code]
    #[test]
    fn replies_use_the_ts2000_formats() {
        let mut p = Ts2000Personality::new();
        let read = Request::ReadFrequency(Vfo::A);
        let value = Outcome::Value(Observation::Frequency(Vfo::B, Hz(7_040_000)));
        assert_eq!(p.reply(&[], &read, &value), b"FB00007040000;");
        assert_eq!(
            p.reply(
                &[],
                &Request::ReadMode,
                &Outcome::Value(Observation::Mode(Mode::Cw))
            ),
            b"MD3;"
        );
        assert_eq!(
            p.reply(
                &[],
                &Request::ReadMode,
                &Outcome::Value(Observation::Mode(Mode::Data))
            ),
            b"MD2;"
        );
        assert_eq!(
            p.reply(
                &[],
                &Request::ReadSignal,
                &Outcome::Value(Observation::Signal(6))
            ),
            b"SM00006;"
        );
        assert!(p.reply(&[], &read, &Outcome::Done).is_empty());
        assert_eq!(p.reply(&[], &read, &Outcome::Refused), b"?;");
        assert_eq!(
            p.reply(&[], &read, &Outcome::Value(Observation::Ptt(true))),
            b"?;"
        );
    }

    // Tests [@ANCHOR: radioctl_splitter_ts2000_status_answer]
    // Tests [@ANCHOR: radioctl_splitter_ts2000_reply]
    #[test]
    fn the_composite_status_has_the_ts2000_layout() {
        let mut p = Ts2000Personality::new();
        let mut status = Status {
            frequency: Hz(14_074_000),
            mode: Mode::Usb,
            ptt: false,
            rit: false,
            xit: false,
            rit_offset: 0,
            split: false,
        };
        let ask = |p: &mut Ts2000Personality, status: Status| {
            p.reply(&[], &Request::ReadStatus, &Outcome::Status(status))
        };
        // The same bytes the virtual TS-2000 answers for this state (radioctl-protocols).
        assert_eq!(
            ask(&mut p, status),
            b"IF000140740000000+0000000000020000000;"
        );
        status.rit = true;
        status.xit = true;
        status.rit_offset = -120;
        status.ptt = true;
        status.split = true;
        status.mode = Mode::Cw;
        let answer = String::from_utf8(ask(&mut p, status)).expect("ascii");
        assert_eq!(answer, "IF000140740000000-0012011000130010000;");
        assert_eq!(answer.len(), 38);
        status.mode = Mode::Am;
        assert!(ask(&mut p, status).starts_with(b"IF"));
        // A mode the TS-2000 has no digit for is refused rather than guessed.
        status.mode = Mode::Wfm;
        assert_eq!(ask(&mut p, status), b"?;");
        // A status is only the answer to a status request.
        assert_eq!(
            p.reply(&[], &Request::ReadMode, &Outcome::Status(status)),
            b"?;"
        );
    }

    // Tests [@ANCHOR: radioctl_splitter_ts2000_unsolicited]
    // Tests [@ANCHOR: radioctl_splitter_ts2000_reset]
    #[test]
    fn auto_information_is_per_client_and_only_when_switched_on() {
        let mut p = Ts2000Personality::new();
        let change = Change {
            field: Field::Frequency(Vfo::A),
            old: None,
            new: Observation::Frequency(Vfo::A, Hz(7_074_000)),
        };
        assert!(p.unsolicited(&change).is_empty());
        actions(&mut p, "AI1;");
        assert_eq!(p.unsolicited(&change), b"FA00007074000;");
        let mode = Change {
            field: Field::Mode,
            old: None,
            new: Observation::Mode(Mode::Lsb),
        };
        assert_eq!(p.unsolicited(&mode), b"MD1;");
        let ptt = Change {
            field: Field::Ptt,
            old: None,
            new: Observation::Ptt(true),
        };
        assert!(p.unsolicited(&ptt).is_empty());
        p.reset();
        assert!(p.unsolicited(&change).is_empty());
    }
}

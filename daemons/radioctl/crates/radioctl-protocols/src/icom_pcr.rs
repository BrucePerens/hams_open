// SPDX-License-Identifier: AGPL-3.0-or-later
//! Icom PCR computer-controlled receivers (IC-PCR100, PCR1000, PCR1500, PCR2500): sans-IO client
//! engine and virtual twin.
//!
//! **Source.** Hamlib 4.6.2 (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8), read for facts only:
//! `rigs/pcr/pcr.c` (`pcr_send`, `pcr_transaction`, `pcr_parse_answer`, `pcr_set_freq`,
//! `pcr_get_freq`, `pcr_set_mode`, `pcr_get_mode`, `pcr_get_level`, `pcr_open`) and
//! `pcr100.c`, `pcr1000.c`, `pcr1500.c`, `pcr2500.c` (the `reply_size` and `reply_offset` fields and
//! the mode lists). No code or comment was copied and no Icom manual was read (Icom's terms
//! forbid reuse of its data); every fact is `hamlib-source` and unverified.
//!
//! * Commands end with a line feed; answers are four characters and CR LF, except the PCR1000's,
//!   which are a line feed, the four characters and a CR (Hamlib reads six bytes from the first
//!   line feed or answer letter and parses from offset 1 on the PCR1000).
//! * A tuning command is `K0`, ten digits of hertz, `0`, a mode character, `0`, a filter
//!   character and `00`. It is the only way to set the frequency or the mode, so the engine
//!   remembers the last of each and resends both. `G000` accepts, `G001` rejects.
//! * Modes: `0` LSB, `1` USB, `2` AM, `3` CW, `5` FM, `6` WFM (`7` D-STAR and `8` P25 on the
//!   PCR2500 have no radioctl mode). Filters: `0` 2.8 kHz, `1` 6 kHz, `2` 15 kHz, `3` 50 kHz,
//!   `4` 230 kHz; the engine always sends `2` (15 kHz), the filter Hamlib starts with, and never changes it.
//! * The signal is `I1?`, answered `I1` and two hex digits. The receiver cannot report its
//!   frequency or mode, so Hamlib returns what it set last. The engine does the same: a read is
//!   sent as the liveness query `H1?` (`H101` power on, `H100` off) and answered from the
//!   remembered values.
//!
//! Open: like Hamlib, the engine powers the radio on (`H101`) and then stops its automatic
//! updates (`G300`) when the rig actor starts, and again after a link loss (`open_requests` and
//! `link_restored`; 9600 baud for the PCR100 and PCR1000 and 38400 for the others).
//!
//! Gaps: the second receiver of the PCR2500, squelch, volume, filters, noise blanker and AGC are not implemented.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The PCR engine.
pub type PcrEngine = LineEngine<PcrDialect>;

// [@ANCHOR: radioctl_pcr_engine]
/// The engine for an Icom PCR description.
pub fn engine(desc: RigDescription) -> Result<PcrEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::IcomPcr)
}

/// The filter character the engine uses and never changes: 15 kHz, the one Hamlib starts with.
const FILTER_15_KHZ: u8 = b'2';
/// The highest frequency the ten digits can hold.
const MAX_HZ: u64 = 9_999_999_999;

/// The PCR command set: it remembers what it last set because the radio cannot say.
#[derive(Debug, Clone)]
pub struct PcrDialect {
    /// The PCR1000 puts one extra character in front of each answer (`reply_offset` 1 in Hamlib).
    lead: bool,
    freq: Hz,
    mode: u8,
    /// A tuning command sent and not yet accepted: the frequency and mode it carried.
    pending: Option<(Hz, u8)>,
}

// [@ANCHOR: radioctl_pcr_tuning_command]
/// The `K0` tuning command for a frequency, mode character and filter character.
pub fn tuning_command(hz: Hz, mode: u8, filter: u8) -> Option<String> {
    (hz.0 <= MAX_HZ).then(|| {
        format!(
            "K0{:010}0{}0{}00\n",
            hz.0,
            char::from(mode),
            char::from(filter)
        )
    })
}

impl Dialect for PcrDialect {
    // [@ANCHOR: radioctl_pcr_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<PcrDialect, RigError> {
        match desc.receiver.as_ref().map_or("", |r| r.dialect.as_str()) {
            name @ ("pcr" | "pcr1000") => Ok(PcrDialect {
                lead: name == "pcr1000",
                freq: desc.default_frequency,
                mode: desc.mode_code(desc.default_mode).unwrap_or(b'5'),
                pending: None,
            }),
            other => Err(RigError::new(
                ErrorKind::Unsupported,
                format!("unknown PCR dialect {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_pcr_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        let tune = |this: &mut PcrDialect, hz: Hz, mode: u8| {
            let text = tuning_command(hz, mode, FILTER_15_KHZ)
                .ok_or_else(|| unsupported("frequency out of range"))?;
            this.pending = Some((hz, mode));
            Ok::<_, RigError>((text.into_bytes(), ReplyKind::Ack))
        };
        match op {
            Op::SetFrequency(Vfo::A, hz) => {
                let mode = self.mode;
                tune(self, *hz, mode)
            }
            Op::SetMode(mode) => {
                let code = desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                let hz = self.freq;
                tune(self, hz, code)
            }
            Op::GetFrequency(Vfo::A) => Ok((
                b"H1?\n".to_vec(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            )),
            Op::GetMode => Ok((b"H1?\n".to_vec(), ReplyKind::Observation(Field::Mode))),
            Op::GetSignal => Ok((b"I1?\n".to_vec(), ReplyKind::Observation(Field::Signal))),
            _ => Err(unsupported("not implemented for the PCR engine")),
        }
    }

    // [@ANCHOR: radioctl_pcr_dialect_open_commands]
    /// Hamlib's open: `H101` powers the receiver on, `G300` stops its automatic updates.
    fn open_commands(&mut self, _desc: &RigDescription) -> Vec<(Vec<u8>, ReplyKind)> {
        vec![
            (b"H101\n".to_vec(), ReplyKind::Ack),
            (b"G300\n".to_vec(), ReplyKind::Ack),
        ]
    }

    // [@ANCHOR: radioctl_pcr_dialect_terminator]
    fn terminator(&self) -> u8 {
        if self.lead {
            b'\r'
        } else {
            b'\n'
        }
    }

    // [@ANCHOR: radioctl_pcr_dialect_status]
    fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        let line = line
            .find(['G', 'H', 'I'])
            .and_then(|at| line.get(at..))
            .unwrap_or(line);
        match line.get(..4) {
            // The power state answer to an open request is a bare acknowledgement; to a
            // liveness read it is the observation `decode` builds.
            Some("H101") if !matches!(expecting, ReplyKind::Observation(_)) => {
                Some(if expecting == ReplyKind::Ack {
                    vec![Inbound::Ack]
                } else {
                    Vec::new()
                })
            }
            Some("G000") => {
                if let Some((hz, mode)) = self.pending.take() {
                    self.freq = hz;
                    self.mode = mode;
                }
                Some(if expecting == ReplyKind::Ack {
                    vec![Inbound::Ack]
                } else {
                    Vec::new()
                })
            }
            Some("G001") => {
                self.pending = None;
                Some(vec![Inbound::Nak])
            }
            _ => line.is_empty().then(Vec::new),
        }
    }

    // [@ANCHOR: radioctl_pcr_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            return unknown();
        };
        // The PCR1000 puts one character in front of its four-character answers.
        let line = line
            .find(['G', 'H', 'I'])
            .and_then(|at| line.get(at..))
            .unwrap_or(line);
        let found = match (field, line.get(..4)) {
            (_, Some("H100")) => return vec![Inbound::Nak],
            (Field::Frequency(_), Some("H101")) => Some(Observation::Frequency(Vfo::A, self.freq)),
            (Field::Mode, Some("H101")) => desc.code_mode(self.mode).map(Observation::Mode),
            (Field::Signal, Some(head)) if head.starts_with("I1") && line.len() == 4 => {
                u16::from_str_radix(&head[2..], 16)
                    .ok()
                    .map(Observation::Signal)
            }
            _ => None,
        };
        found.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

/// The virtual PCR receiver.
#[derive(Debug)]
pub struct PcrTwin {
    lead: bool,
    desc: RigDescription,
    filter: u8,
}

impl PcrTwin {
    // [@ANCHOR: radioctl_pcr_twin_new]
    /// The twin of an Icom PCR description.
    pub fn new(desc: &RigDescription) -> Result<PcrTwin, RigError> {
        let lead = PcrDialect::from_description(desc)?.lead;
        Ok(PcrTwin {
            lead,
            desc: desc.clone(),
            filter: FILTER_15_KHZ,
        })
    }
}

impl ReceiverTwin for PcrTwin {
    fn terminator(&self) -> u8 {
        b'\n'
    }

    // [@ANCHOR: radioctl_pcr_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let lead = self.lead;
        let reply = |text: &str| {
            if lead {
                format!("\n{text}\r").into_bytes()
            } else {
                format!("{text}\r\n").into_bytes()
            }
        };
        match line {
            "H1?" | "H101" => reply("H101"),
            "G300" | "G301" => reply("G000"),
            // The identity queries Hamlib sends at open: protocol, firmware, options and country.
            "G2?" => reply("G201"),
            "G4?" => reply("G401"),
            "GD?" => reply("GD00"),
            "GE?" => reply("GE01"),
            // Volume, squelch and the other J-level settings: accepted and not kept.
            _ if line.len() == 5
                && line.starts_with('J')
                && line[1..].bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                reply("G000")
            }
            "I1?" => reply(&format!("I1{:02X}", state.signal.min(255))),
            _ => {
                let b = line.as_bytes();
                let digits_ok = b.len() == 18
                    && (b.starts_with(b"K0") || b.starts_with(b"K1"))
                    && b.get(2..12)
                        .is_some_and(|d| d.iter().all(u8::is_ascii_digit))
                    && b.get(12) == Some(&b'0')
                    && b.get(14) == Some(&b'0')
                    && b.get(16..) == Some(b"00");
                let mode = b.get(13).and_then(|c| self.desc.code_mode(*c));
                match (
                    digits_ok,
                    mode,
                    line.get(2..12).and_then(|d| d.parse::<u64>().ok()),
                ) {
                    // The PCR2500's second receiver (K1): accepted, not modelled.
                    (true, Some(_), Some(_)) if b.get(1) == Some(&b'1') => reply("G000"),
                    (true, Some(mode), Some(hz)) => {
                        state.freq_a = Hz(hz);
                        state.mode = mode;
                        self.filter = b.get(15).copied().unwrap_or(FILTER_15_KHZ);
                        reply("G000")
                    }
                    _ => reply("G001"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use radioctl_core::engine::ProtocolEngine;
    use radioctl_core::units::Mode;

    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    fn bytes(e: &mut PcrEngine, op: Op) -> Vec<u8> {
        e.encode(&op).expect("encode").bytes
    }

    // Tests [@ANCHOR: radioctl_pcr_dialect_open_commands]
    // Tests [@ANCHOR: radioctl_receiver_engine_open_requests]
    // Tests [@ANCHOR: radioctl_receiver_engine_link_restored]
    // Tests [@ANCHOR: radioctl_pcr_dialect_status]
    #[test]
    fn the_open_requests_are_power_on_then_no_automatic_updates_and_their_answers_acknowledge() {
        for id in [
            "icom-ic-pcr100",
            "icom-ic-pcr1000",
            "icom-ic-pcr1500",
            "icom-ic-pcr2500",
        ] {
            let mut e = engine(shipped(id)).expect("engine");
            let open = e.open_requests();
            let wire: Vec<&[u8]> = open.iter().map(|r| r.bytes.as_slice()).collect();
            assert_eq!(wire, [b"H101\n".as_slice(), b"G300\n".as_slice()], "{id}");
            assert!(open.iter().all(|r| r.reply == ReplyKind::Ack), "{id}");
            // The radio's answers acknowledge the request being waited for, one each.
            let term: &[u8] = if id == "icom-ic-pcr1000" {
                b"\r"
            } else {
                b"\n"
            };
            for answer in ["H101", "G000"] {
                let mut line = answer.as_bytes().to_vec();
                line.extend_from_slice(term);
                assert_eq!(e.feed(&line, T0), vec![Inbound::Ack], "{id} {answer}");
                e.open_requests();
            }
            // After a link loss the same two are sent again, and a half line is dropped.
            e.encode(&Op::GetSignal).expect("signal");
            e.feed(b"I1", T0);
            let again = e.link_restored();
            assert_eq!(again.len(), 2, "{id}");
            assert_eq!(
                e.feed(b"I1", T0 + Duration::from_millis(10)),
                Vec::<Inbound>::new(),
                "{id}: the old half line is gone"
            );
        }
    }

    // Tests [@ANCHOR: radioctl_pcr_tuning_command]
    #[test]
    fn the_tuning_command_has_the_documented_layout() {
        assert_eq!(
            tuning_command(Hz(145_500_000), b'5', b'1').as_deref(),
            Some("K00145500000050100\n")
        );
        assert!(tuning_command(Hz(10_000_000_000), b'5', b'1').is_none());
    }

    // Tests [@ANCHOR: radioctl_pcr_dialect_terminator]
    // Tests [@ANCHOR: radioctl_pcr_engine]
    // Tests [@ANCHOR: radioctl_pcr_dialect_from_description]
    // Tests [@ANCHOR: radioctl_pcr_dialect_encode]
    // Tests [@ANCHOR: radioctl_pcr_dialect_status]
    // Tests [@ANCHOR: radioctl_pcr_dialect_decode]
    #[test]
    fn frequency_and_mode_share_one_command_and_are_remembered() {
        let mut e = engine(shipped("icom-ic-pcr1000")).expect("engine");
        assert_eq!(
            bytes(&mut e, Op::SetFrequency(Vfo::A, Hz(145_500_000))),
            b"K00145500000050200\n"
        );
        assert_eq!(e.feed(b"G000\r\n", T0), vec![Inbound::Ack]);
        assert_eq!(
            bytes(&mut e, Op::SetMode(Mode::Usb)),
            b"K00145500000010200\n"
        );
        assert_eq!(
            e.feed(b"G001\r\n", T0),
            vec![Inbound::Nak],
            "rejected: not remembered"
        );
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(b"H101\r\n", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Fm))]
        );
        e.encode(&Op::SetMode(Mode::Usb)).expect("set");
        e.feed(b"G000\r\n", T0);
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            e.feed(b"\nH101\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(145_500_000)
            ))],
            "the PCR1000 form is tolerated"
        );
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(b"H101\r\n", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Usb))]
        );
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(e.feed(b"H100\r\n", T0), vec![Inbound::Nak], "power off");
        e.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            e.feed(b"I14B\r\n", T0),
            vec![Inbound::Observation(Observation::Signal(0x4B))]
        );
        e.encode(&Op::GetSignal).expect("get");
        assert!(matches!(
            e.feed(b"I1ZZ\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(10_000_000_000)))
            .is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(engine(shipped("aor-ar8000")).is_err());
    }

    // Tests [@ANCHOR: radioctl_pcr_twin_new]
    // Tests [@ANCHOR: radioctl_pcr_twin_line]
    #[test]
    fn the_twin_rejects_a_malformed_tuning_command() {
        let desc = shipped("icom-ic-pcr100");
        let mut twin = PcrTwin::new(&desc).expect("twin");
        let mut state = crate::virtual_rig::VirtualRig::from_description(&desc)
            .expect("rig")
            .state()
            .clone();
        assert_eq!(twin.line(&mut state, "K00145500000050100"), b"G000\r\n");
        assert_eq!(state.freq_a, Hz(145_500_000));
        assert_eq!(twin.line(&mut state, "K0014550000005010"), b"G001\r\n");
        assert_eq!(
            twin.line(&mut state, "K00145500000090100"),
            b"G001\r\n",
            "no mode 9"
        );
        assert!(PcrTwin::new(&shipped("aor-ar8000")).is_err());
    }
}

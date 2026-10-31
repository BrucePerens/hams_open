// SPDX-License-Identifier: AGPL-3.0-or-later
//! A virtual FlexRadio: a sans-IO model of the radio's side of the SmartSDR TCP API, for tests.
//!
//! It speaks the subset the [`crate::flex::FlexEngine`] uses, written from the same published
//! pages (see that module's header): the `V` and `H` prologue, `C<seq>|command` lines with one
//! `R` reply each, `sub`/`unsub` of slice, tx and pan status, `ping`, `info`, `version`,
//! `slice t`, `slice s <n> mode=|audio_level=`, `filt`, `transmit set`, `xmit` and `cwx send`.
//! Anything else is answered with the response `0x50000013` (the page "TCPIP keepalive" lists it
//! as "Bad Command"). It is NOT a model of the real radio: its answers are our reading of the
//! wiki, so the engine and the virtual radio agreeing proves they agree with each other only.
//!
//! The virtual radio never transmits; keying only moves its interlock state to `TRANSMITTING`
//! so a test can see exactly which transmit commands arrived. Every command line received is
//! kept in [`VirtualFlex::received`], which is how the transmit-gate tests prove that nothing
//! that can key reached the wire while the gate was closed.
//!
//! [`VirtualFlex::order`] chooses whether a status line follows its reply or precedes it, since
//! the wiki does not fix the order.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::units::{Hz, Mode};

use crate::flex::{
    format_mhz, mode_from_name, mode_name, parse_mhz, parse_pairs, MAX_LINE, MAX_PERCENT,
};

/// Bad command response used for anything the virtual radio does not know.
pub const BAD_COMMAND: u32 = 0x5000_0013;
/// Response for an unrecognised mode ("TCPIP slice").
pub const BAD_MODE: u32 = 0x5000_0032;
/// Response for a parameter out of range ("TCPIP slice").
pub const OUT_OF_RANGE: u32 = 0x5000_0033;
/// Response for an unknown slice receiver ("TCPIP slice").
pub const INVALID_SLICE: u32 = 0x5000_000D;
/// Stream id of the one panadapter.
pub const PAN_STREAM: u32 = 0x4000_0000;

/// Whether a status line follows or precedes the reply that caused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// `R` first, then `S`.
    ReplyFirst,
    /// `S` first, then `R`.
    StatusFirst,
}

/// A change made at the radio itself (the GUI or the front panel), not by this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexPanel {
    /// The slice was tuned.
    Tune(Hz),
    /// The slice changed mode.
    SetMode(Mode),
    /// The operator pressed MOX.
    Mox(bool),
}

/// What the virtual radio holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlexRigState {
    /// Frequency of slice 0.
    pub frequency: Hz,
    /// Mode of slice 0.
    pub mode: Mode,
    /// Low filter edge in hertz.
    pub filter_lo_hz: i64,
    /// High filter edge in hertz.
    pub filter_hi_hz: i64,
    /// Slice audio level.
    pub audio_level: u16,
    /// Microphone level.
    pub mic_level: u16,
    /// Transmit power setting.
    pub rf_power: u16,
    /// True while "transmitting".
    pub keyed: bool,
    /// Texts of `cwx send` commands, spaces restored.
    pub cw_sent: Vec<String>,
    /// How many `cwx clear` commands arrived (each empties `cw_sent`).
    pub cw_stops: u32,
    /// Panadapter center.
    pub pan_center: Hz,
    /// Panadapter bandwidth.
    pub pan_bandwidth: Hz,
}

/// The radio.
pub struct VirtualFlex {
    handle: u32,
    model: String,
    scus: u8,
    state: FlexRigState,
    buffer: Vec<u8>,
    discarding: bool,
    received: Vec<String>,
    sub_slice: bool,
    sub_tx: bool,
    sub_pan: bool,
    /// Whether status precedes or follows its reply.
    pub order: Order,
}

// [@ANCHOR: radioctl_virtual_flex_signed_mhz]
/// Formats signed hertz as signed megahertz with six decimals (`-2700` is `-0.002700`).
pub fn format_signed_mhz(hz: i64) -> String {
    let sign = if hz < 0 { "-" } else { "" };
    format!("{sign}{}", format_mhz(Hz(hz.unsigned_abs())))
}

impl VirtualFlex {
    // [@ANCHOR: radioctl_virtual_flex_new]
    /// Builds the radio for a Flex description.
    pub fn from_description(desc: &RigDescription) -> Result<VirtualFlex, RigError> {
        let Some(flex) = desc.flex.as_ref().filter(|_| desc.family == Family::Flex) else {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a FlexRadio description",
            ));
        };
        Ok(VirtualFlex {
            handle: 0x6F4E_C23D,
            model: flex.model.clone(),
            scus: flex.scus,
            state: FlexRigState {
                frequency: desc.default_frequency,
                mode: desc.default_mode,
                filter_lo_hz: 100,
                filter_hi_hz: 2_800,
                audio_level: 50,
                mic_level: 40,
                rf_power: 10,
                keyed: false,
                cw_sent: Vec::new(),
                cw_stops: 0,
                pan_center: desc.default_frequency,
                pan_bandwidth: Hz(200_000),
            },
            buffer: Vec::new(),
            discarding: false,
            received: Vec::new(),
            sub_slice: false,
            sub_tx: false,
            sub_pan: false,
            order: Order::ReplyFirst,
        })
    }

    /// The prologue the radio sends on connect: `V` and `H` lines.
    pub fn greeting(&self) -> Vec<u8> {
        format!("V1.4.0.0\nH{:08X}\n", self.handle).into_bytes()
    }

    /// The radio's state.
    pub fn state(&self) -> &FlexRigState {
        &self.state
    }

    /// Every command line received so far, without the `C<seq>|` prefix.
    pub fn received(&self) -> &[String] {
        &self.received
    }

    fn slice_line(&self) -> String {
        let mode = mode_name(self.state.mode).unwrap_or("usb").to_uppercase();
        format!(
            "S{:08X}|slice 0 in_use=1 RF_frequency={} mode={mode} filter_lo={} filter_hi={} audio_level={}\n",
            self.handle,
            format_mhz(self.state.frequency),
            format_signed_mhz(self.state.filter_lo_hz),
            format_signed_mhz(self.state.filter_hi_hz),
            self.state.audio_level
        )
    }

    // [@ANCHOR: radioctl_virtual_flex_interlock_line]
    fn interlock_line(&self) -> String {
        let (state, extra) = if self.state.keyed {
            ("TRANSMITTING", " source=SW")
        } else {
            ("READY", "")
        };
        format!("S0|interlock state={state}{extra} tx_allowed=1\n")
    }

    fn transmit_line(&self) -> String {
        format!(
            "S{:08X}|transmit rfpower={} miclevel={}\n",
            self.handle, self.state.rf_power, self.state.mic_level
        )
    }

    fn pan_line(&self) -> String {
        format!(
            "S{:08X}|display pan 0x{PAN_STREAM:08X} center={} bandwidth={} x_pixels=800 y_pixels=400 fps=20\n",
            self.handle,
            format_mhz(self.state.pan_center),
            format_mhz(self.state.pan_bandwidth)
        )
    }

    fn reply(seq: &str, code: u32, message: &str) -> String {
        format!("R{seq}|{code:X}|{message}\n")
    }

    // [@ANCHOR: radioctl_virtual_flex_with_status]
    /// Joins a reply with the status it caused, in the configured order.
    fn with_status(&self, reply: String, status: String) -> String {
        match self.order {
            Order::ReplyFirst => reply + &status,
            Order::StatusFirst => status + &reply,
        }
    }

    // [@ANCHOR: radioctl_virtual_flex_slice_status_if_subscribed]
    fn slice_status_if_subscribed(&self) -> String {
        if self.sub_slice {
            self.slice_line()
        } else {
            String::new()
        }
    }

    // [@ANCHOR: radioctl_virtual_flex_command]
    /// Answers one command line.
    fn command(&mut self, seq: &str, text: &str) -> String {
        let mut words = text.split_whitespace();
        let verb = words.next().unwrap_or("");
        let args: Vec<&str> = words.collect();
        match (verb, args.as_slice()) {
            ("ping", []) => Self::reply(seq, 0, ""),
            ("version", []) => Self::reply(
                seq,
                0,
                "SmartSDR-MB=3.2.31.1#PSoC-MBTRX=1.3.1.6#FPGA-MB=0.6.0.0",
            ),
            ("info", []) => Self::reply(
                seq,
                0,
                &format!(
                    "model=\"{}\",name=\"Virtual\",callsign=\"N0CALL\",scu={},slice=4,software_ver=3.2.31",
                    self.model, self.scus
                ),
            ),
            ("sub", [object, _]) => {
                let (status, known) = match *object {
                    "slice" => {
                        self.sub_slice = true;
                        (self.slice_line(), true)
                    }
                    "tx" => {
                        self.sub_tx = true;
                        (self.interlock_line() + &self.transmit_line(), true)
                    }
                    "pan" => {
                        self.sub_pan = true;
                        (self.pan_line(), true)
                    }
                    _ => (String::new(), false),
                };
                if known {
                    self.with_status(Self::reply(seq, 0, ""), status)
                } else {
                    Self::reply(seq, 0x5000_00A3, "")
                }
            }
            ("unsub", [object, _]) => {
                match *object {
                    "slice" => self.sub_slice = false,
                    "tx" => self.sub_tx = false,
                    "pan" => self.sub_pan = false,
                    _ => {}
                }
                Self::reply(seq, 0, "")
            }
            ("slice", ["t", index, freq]) => {
                if *index != "0" {
                    return Self::reply(seq, INVALID_SLICE, "");
                }
                match parse_mhz(freq) {
                    Some(hz) => {
                        self.state.frequency = hz;
                        self.with_status(Self::reply(seq, 0, ""), self.slice_status_if_subscribed())
                    }
                    None => Self::reply(seq, 0x5000_0016, ""),
                }
            }
            ("slice", ["s", index, rest @ ..]) => {
                if *index != "0" {
                    return Self::reply(seq, INVALID_SLICE, "");
                }
                let joined = rest.join(" ");
                for (key, value) in parse_pairs(&joined) {
                    match key {
                        "mode" => match mode_from_name(value) {
                            Some(mode) => self.state.mode = mode,
                            None => return Self::reply(seq, BAD_MODE, ""),
                        },
                        "audio_level" => match value.parse::<u16>() {
                            Ok(v) if v <= MAX_PERCENT => self.state.audio_level = v,
                            _ => return Self::reply(seq, OUT_OF_RANGE, ""),
                        },
                        _ => return Self::reply(seq, 0x5000_002D, ""),
                    }
                }
                self.with_status(Self::reply(seq, 0, ""), self.slice_status_if_subscribed())
            }
            ("filt", [index, lo, hi]) => {
                if *index != "0" {
                    return Self::reply(seq, INVALID_SLICE, "");
                }
                match (lo.parse::<i64>(), hi.parse::<i64>()) {
                    (Ok(lo), Ok(hi)) if lo < hi => {
                        self.state.filter_lo_hz = lo;
                        self.state.filter_hi_hz = hi;
                        self.with_status(
                            Self::reply(seq, 0, ""),
                            self.slice_status_if_subscribed(),
                        )
                    }
                    _ => Self::reply(seq, OUT_OF_RANGE, ""),
                }
            }
            ("transmit", ["set", rest @ ..]) => {
                let joined = rest.join(" ");
                for (key, value) in parse_pairs(&joined) {
                    let Some(v) = value.parse::<u16>().ok().filter(|v| *v <= MAX_PERCENT) else {
                        return Self::reply(seq, OUT_OF_RANGE, "");
                    };
                    match key {
                        "miclevel" => self.state.mic_level = v,
                        "rfpower" => self.state.rf_power = v,
                        _ => return Self::reply(seq, 0x5000_002D, ""),
                    }
                }
                let status = if self.sub_tx {
                    self.transmit_line()
                } else {
                    String::new()
                };
                self.with_status(Self::reply(seq, 0, ""), status)
            }
            ("xmit", [state]) => {
                self.state.keyed = matches!(*state, "1" | "on" | "T" | "+");
                let status = if self.sub_tx {
                    self.interlock_line()
                } else {
                    String::new()
                };
                self.with_status(Self::reply(seq, 0, ""), status)
            }
            ("cwx", ["clear"]) => {
                self.state.cw_sent.clear();
                self.state.cw_stops = self.state.cw_stops.saturating_add(1);
                Self::reply(seq, 0, "")
            }
            ("cwx", ["send", ..]) => {
                let body = text.trim_start().strip_prefix("cwx send").unwrap_or("").trim();
                let sent = body.trim_matches('"').replace('\u{7f}', " ");
                self.state.cw_sent.push(sent);
                Self::reply(seq, 0, "0,0")
            }
            _ => Self::reply(seq, BAD_COMMAND, ""),
        }
    }

    // [@ANCHOR: radioctl_virtual_flex_feed]
    /// Consumes bytes from the client and returns what the radio sends back. Commands are
    /// `C[D]<seq>|text` lines ended by CR, LF or CRLF; a line that is not a command is ignored.
    pub fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8> {
        let mut out = String::new();
        for &byte in bytes {
            if byte == b'\r' || byte == b'\n' {
                if !self.discarding && !self.buffer.is_empty() {
                    let raw = std::mem::take(&mut self.buffer);
                    if let Ok(line) = String::from_utf8(raw) {
                        out.push_str(&self.line(&line));
                    }
                }
                self.discarding = false;
                self.buffer.clear();
            } else if self.buffer.len() >= MAX_LINE {
                self.buffer.clear();
                self.discarding = true;
            } else if !self.discarding {
                self.buffer.push(byte);
            }
        }
        out.into_bytes()
    }

    // [@ANCHOR: radioctl_virtual_flex_line]
    fn line(&mut self, line: &str) -> String {
        let Some(rest) = line.strip_prefix('C') else {
            return String::new();
        };
        let rest = rest.strip_prefix('D').unwrap_or(rest);
        let Some((seq, text)) = rest.split_once('|') else {
            return String::new();
        };
        if seq.parse::<u32>().is_err() {
            return String::new();
        }
        self.received.push(text.to_string());
        self.command(seq, text)
    }

    // [@ANCHOR: radioctl_virtual_flex_front_panel]
    /// A change made at the radio: returns the status lines sent to a subscribed client.
    pub fn front_panel(&mut self, change: FlexPanel) -> Vec<u8> {
        let mut out = String::new();
        match change {
            FlexPanel::Tune(hz) => {
                self.state.frequency = hz;
                out.push_str(&self.slice_status_if_subscribed());
            }
            FlexPanel::SetMode(mode) => {
                self.state.mode = mode;
                out.push_str(&self.slice_status_if_subscribed());
            }
            FlexPanel::Mox(on) => {
                self.state.keyed = on;
                if self.sub_tx {
                    out.push_str(&self.interlock_line());
                }
            }
        }
        out.into_bytes()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use radioctl_core::rigfile;

    fn radio() -> VirtualFlex {
        let desc = rigfile::parse(include_str!("../../../rigs/flex-6300.toml"))
            .expect("flex file")
            .description;
        VirtualFlex::from_description(&desc).expect("virtual flex")
    }

    fn talk(radio: &mut VirtualFlex, text: &str) -> String {
        String::from_utf8(radio.feed(text.as_bytes(), Duration::ZERO)).expect("ascii")
    }

    // Tests [@ANCHOR: radioctl_virtual_flex_new]
    #[test]
    fn only_a_flex_description_makes_a_virtual_flex() {
        assert!(VirtualFlex::from_description(&RigDescription::ic7300()).is_err());
        assert_eq!(radio().greeting(), b"V1.4.0.0\nH6F4EC23D\n");
    }

    // Tests [@ANCHOR: radioctl_virtual_flex_feed]
    // Tests [@ANCHOR: radioctl_virtual_flex_command]
    // Tests [@ANCHOR: radioctl_virtual_flex_line]
    #[test]
    fn commands_get_one_reply_each_and_junk_gets_none() {
        let mut r = radio();
        assert_eq!(talk(&mut r, "C1|ping\n"), "R1|0|\n");
        assert_eq!(talk(&mut r, "C2|nonsense\r\n"), "R2|50000013|\n");
        assert_eq!(talk(&mut r, "hello\nC|x\nCx|ping\n"), "");
        assert_eq!(talk(&mut r, "C3|sli"), "");
        assert_eq!(talk(&mut r, "ce t 0 7.1\n"), "R3|0|\n");
        assert_eq!(r.state().frequency, Hz(7_100_000));
        assert_eq!(talk(&mut r, "C4|slice t 9 7.1\n"), "R4|5000000D|\n");
        assert_eq!(talk(&mut r, "C5|slice s 0 mode=bogus\n"), "R5|50000032|\n");
        assert_eq!(
            talk(&mut r, "C6|slice s 0 audio_level=101\n"),
            "R6|50000033|\n"
        );
        assert_eq!(talk(&mut r, "C7|filt 0 500 100\n"), "R7|50000033|\n");
        assert!(r.received().iter().any(|l| l == "nonsense"));
        let long = "x".repeat(MAX_LINE + 5);
        assert_eq!(talk(&mut r, &format!("{long}\nC8|ping\n")), "R8|0|\n");
    }

    // Tests [@ANCHOR: radioctl_virtual_flex_command]
    // Tests [@ANCHOR: radioctl_virtual_flex_with_status]
    // Tests [@ANCHOR: radioctl_virtual_flex_slice_status_if_subscribed]
    // Tests [@ANCHOR: radioctl_virtual_flex_interlock_line]
    #[test]
    fn subscriptions_send_status_in_the_chosen_order() {
        let mut r = radio();
        let reply_first = talk(&mut r, "C1|sub slice all\n");
        assert!(reply_first
            .starts_with("R1|0|\nS6F4EC23D|slice 0 in_use=1 RF_frequency=14.074000 mode=USB"));
        r.order = Order::StatusFirst;
        let status_first = talk(&mut r, "C2|slice t 0 7.2\n");
        assert!(
            status_first.starts_with("S6F4EC23D|slice 0"),
            "{status_first}"
        );
        assert!(status_first.ends_with("R2|0|\n"));
        assert!(talk(&mut r, "C3|sub tx all\n").contains("interlock state=READY"));
        assert!(talk(&mut r, "C4|sub pan all\n").contains("display pan 0x40000000 center="));
        assert_eq!(talk(&mut r, "C5|sub bogus all\n"), "R5|500000A3|\n");
        assert_eq!(talk(&mut r, "C6|unsub slice all\n"), "R6|0|\n");
        assert_eq!(talk(&mut r, "C7|slice t 0 7.3\n"), "R7|0|\n");
    }

    // Tests [@ANCHOR: radioctl_virtual_flex_command]
    #[test]
    fn keying_is_recorded_and_cw_text_is_restored() {
        let mut r = radio();
        talk(&mut r, "C1|sub tx all\n");
        let keyed = talk(&mut r, "C2|xmit 1\n");
        assert!(keyed.contains("state=TRANSMITTING source=SW"), "{keyed}");
        assert!(r.state().keyed);
        assert!(talk(&mut r, "C3|xmit 0\n").contains("state=READY"));
        assert_eq!(talk(&mut r, "C4|cwx send \"CQ\u{7f}DX\"\n"), "R4|0|0,0\n");
        assert_eq!(r.state().cw_sent, vec!["CQ DX".to_string()]);
        assert_eq!(talk(&mut r, "C5|cwx clear\n"), "R5|0|\n");
        assert!(r.state().cw_sent.is_empty());
        assert_eq!(r.state().cw_stops, 1);
        assert!(talk(&mut r, "C5|transmit set rfpower=7\n").contains("rfpower=7"));
        assert_eq!(
            talk(&mut r, "C6|transmit set rfpower=900\n"),
            "R6|50000033|\n"
        );
    }

    // Tests [@ANCHOR: radioctl_virtual_flex_front_panel]
    // Tests [@ANCHOR: radioctl_virtual_flex_signed_mhz]
    #[test]
    fn front_panel_changes_are_pushed_only_to_subscribers() {
        let mut r = radio();
        assert!(r.front_panel(FlexPanel::Tune(Hz(7_000_000))).is_empty());
        assert!(r.front_panel(FlexPanel::Mox(true)).is_empty());
        talk(&mut r, "C1|sub slice all\nC2|sub tx all\n");
        let pushed = r.front_panel(FlexPanel::SetMode(Mode::Cw));
        assert!(String::from_utf8(pushed).expect("text").contains("mode=CW"));
        let pushed = r.front_panel(FlexPanel::Mox(false));
        assert!(String::from_utf8(pushed).expect("text").contains("READY"));
        assert_eq!(format_signed_mhz(-2_700), "-0.002700");
        assert_eq!(format_signed_mhz(300), "0.000300");
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! FlexRadio SmartSDR TCP/IP API (FLEX-6000 series): sans-IO client engine for the control plane.
//!
//! Written from FlexRadio's published API documentation only: the "SmartSDR API" wiki,
//! <https://github.com/flexradio/smartsdr-api-docs/wiki>, cloned from
//! `https://github.com/flexradio/smartsdr-api-docs.wiki.git` at commit
//! `8a397c11421a260247a730e50acadbecf65b4772` (6 October 2026). The repository carries no licence
//! or terms file and no page of the wiki states terms for copying; only facts (command spellings,
//! field names, value ranges) are used here, no text. No Hamlib source was read. Pages used:
//!
//! - "SmartSDR TCPIP API": the prologue (`V` version, `H` handle), `C[D]<seq>|command`, the
//!   `R<seq>|<hex>|message` reply (zero is success), `S<handle>|status` and `M<number>|text`
//!   lines, terminators CR, LF or CRLF, port 4992.
//! - "SmartSDR Status Responses": slice (`RF_frequency` and `filter_lo`/`filter_hi` in MHz, `mode`,
//!   `in_use`), panadapter (`center`, `bandwidth`, `x_pixels`/`x-pixels`, `y_pixels`, `fps`) and
//!   interlock (`state`, `source`, `tx_allowed`) status.
//! - "TCPIP slice" (`slice t`, `slice s <n> key=value`, `audio_level`, `mode`), "TCPIP filt"
//!   (`filt <slice> <lo> <hi>` in hertz), "TCPIP sub" (`sub slice|tx|pan all`), "TCPIP info"
//!   (the quoted `key=value` list), "TCPIP version" (`#`-separated `name=value`), "TCPIP ping",
//!   "TCPIP transmit" (`transmit set miclevel=` and `rfpower=`, `transmit tune`), "TCPIP xmit"
//!   (`xmit 1|0`, the interlock states), "TCPIP cwx" (`cwx send "text"`, spaces sent as 0x7f),
//!   "TCPIP keepalive", "Boolean State", "SmartSDR Supported Modes", "Discovery protocol".
//!
//! Excluded source: the wiki page "Known API Responses" embeds a C header (`messages.h`) whose
//! own comment says unauthorised use, duplication or distribution is prohibited. Its text and
//! constant names were not used; only the response codes printed in the per-command tables of
//! the pages above appear (`0x5000002C` wrong parameter count, `0x5000003D` transmit not
//! supported, `0x50000042` not ready, `0x50000043` no transmitter), and they are only logged.
//!
//! ## Shape of the engine
//!
//! SmartSDR has no "get" commands: a client subscribes (`sub slice all`, `sub tx all`,
//! `sub pan all`) and the radio pushes `S` status lines when an object changes. So the engine
//! keeps a shadow of the status it has seen. The first command sent on a link is prefixed with
//! the subscriptions and `info`; a read is a `ping` whose reply is answered from the shadow (the
//! docs do not fix the order of an `R` line and the `S` lines it causes, so a read whose value is
//! not in the shadow yet waits and is answered by the first status line that carries the field).
//! `GetFirmware` is the `version` command (the `SmartSDR-MB` component). Unsolicited status
//! lines are [`Inbound::Observation`]s, so the actor's cache follows a front-panel (GUI) change.
//!
//! The VFO A of the core vocabulary is one slice receiver (index 0 unless
//! [`FlexEngine::with_slice`] says otherwise); the engine never creates or removes slices, so a
//! radio with no slice yet answers reads with a timeout. Slice and panadapter state beyond the
//! core vocabulary is available through [`FlexEngine::slice_state`] and
//! [`FlexEngine::panadapter`].
//!
//! Not implemented: VITA-49 streams (see below), `slice create`, `display pan create`, memories,
//! the second VFO and split (a second slice is not modelled), RIT/XIT, `keepalive` (the engine
//! never enables it, so the radio does not time the link out), `client program` (the docs say not
//! to use it). Modes the core `Mode` cannot express (`sam`, `nfm`, `dfm`, `fdv`) are kept as text
//! in the slice state and produce no mode observation.
//!
//! ## What a stream interface needs (out of scope here)
//!
//! Meters, spectrum and audio travel as VITA-49 over UDP. A client sends `client udpport <p>`
//! with a free local UDP port; the radio sends packets to it (meters, panadapter, waterfall,
//! audio and DAX streams carry the stream ids from the `display pan create`, `stream create` and
//! `meter list` replies). Meter values arrive in extension packets whose layout is described by
//! periodic context packets ("Metering Protocol"), so S-meter, power and SWR need that decoder:
//! the Flex descriptions therefore do not claim `read-signal`, `read-power` or `read-swr`.
//! Discovery packets are VITA-49 extension packets too (stream id 0x800); see
//! [`parse_discovery`].
//!
//! ## Transmit
//!
//! Commands that can put RF on the air: `xmit 1`, `transmit tune on`, `cwx send`, `dvk` and
//! `atu start`. The engine can encode only the first and third, for [`Op::SetPtt`] and
//! [`Op::SendCw`], both of which are `Op::transmits()` and so refused by the actor and the handle
//! while the gate is closed. [`line_may_transmit`] classifies a wire line and a test checks that
//! every transmitting line comes from a transmitting op. `xmit 0` unkeys and is always allowed.
//! CW text containing a quote, `|`, a control character or anything outside printable ASCII is
//! refused, so a text cannot smuggle a second command.

use std::collections::BTreeMap;
use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Mode, Vfo};

/// TCP control port of a radio ("SmartSDR TCPIP API").
pub const CONTROL_PORT: u16 = 4992;
/// UDP port for the discovery broadcast: the page "Discovery protocol" names 4991 (the VITA-49
/// streaming port) and 4992 in the same paragraph; this crate listens on 4992 as the task says
/// and a bench run must settle which one the radio uses.
pub const DISCOVERY_PORT: u16 = 4992;
/// UDP port of VITA-49 streams ("TCPIP meter": "the standard VITA49 Port 4991").
pub const VITA_PORT: u16 = 4991;
/// Longest line kept; a `meter list` reply is a few kilobytes.
pub const MAX_LINE: usize = 16 * 1024;
/// Longest CW text sent in one `cwx send` (our limit; the docs give none).
pub const MAX_CW_CHARS: usize = 100;
/// Commands awaiting a reply before the oldest is forgotten.
pub const MAX_PENDING: usize = 64;
/// Largest value of a percentage level (`audio_level`, `miclevel`, `rfpower`).
pub const MAX_PERCENT: u16 = 100;

// [@ANCHOR: radioctl_flex_parse_mhz]
/// Parses a frequency in megahertz (`14.2012`) to hertz without floating point. The seventh
/// decimal digit rounds. `None` for anything else, or above 99999 MHz.
pub fn parse_mhz(text: &str) -> Option<Hz> {
    let (whole, frac) = match text.split_once('.') {
        Some((w, f)) => (w, f),
        None => (text, ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(whole) || !all_digits(frac) || whole.len() > 5 || frac.len() > 15 {
        return None;
    }
    let whole_mhz: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut hz = whole_mhz.checked_mul(1_000_000)?;
    let mut scale = 100_000u64;
    for (i, b) in frac.bytes().enumerate() {
        let digit = u64::from(b - b'0');
        if i < 6 {
            hz += digit * scale;
            scale /= 10;
        } else {
            if i == 6 && digit >= 5 {
                hz += 1;
            }
            break;
        }
    }
    Some(Hz(hz))
}

// [@ANCHOR: radioctl_flex_parse_mhz_signed]
/// Like [`parse_mhz`] with an optional sign, in signed hertz (filter edges are negative on the
/// lower sidebands).
pub fn parse_mhz_signed(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let hz = i64::try_from(parse_mhz(digits)?.0).ok()?;
    Some(if negative { -hz } else { hz })
}

// [@ANCHOR: radioctl_flex_format_mhz]
/// Formats hertz as megahertz with six decimals (`14201200` is `14.201200`).
pub fn format_mhz(hz: Hz) -> String {
    format!("{}.{:06}", hz.0 / 1_000_000, hz.0 % 1_000_000)
}

/// The SmartSDR spelling of each mode the core vocabulary can express ("SmartSDR Supported
/// Modes"). `digu` and `digl` are the Flex data modes on the upper and lower sideband.
pub const MODE_NAMES: [(Mode, &str); 7] = [
    (Mode::Lsb, "lsb"),
    (Mode::Usb, "usb"),
    (Mode::Cw, "cw"),
    (Mode::Am, "am"),
    (Mode::Fm, "fm"),
    (Mode::Data, "digu"),
    (Mode::DataR, "digl"),
];

// [@ANCHOR: radioctl_flex_mode_name]
/// The spelling of a mode in a `slice s ... mode=` command.
pub fn mode_name(mode: Mode) -> Option<&'static str> {
    if mode == Mode::Rtty {
        return Some("rtty");
    }
    MODE_NAMES
        .iter()
        .find(|(m, _)| *m == mode)
        .map(|(_, name)| *name)
}

// [@ANCHOR: radioctl_flex_mode_from_name]
/// The mode for a status spelling, any case (`USB`, `usb`).
pub fn mode_from_name(name: &str) -> Option<Mode> {
    let lower = name.to_ascii_lowercase();
    if lower == "rtty" {
        return Some(Mode::Rtty);
    }
    MODE_NAMES
        .iter()
        .find(|(_, n)| *n == lower)
        .map(|(mode, _)| *mode)
}

// [@ANCHOR: radioctl_flex_line_may_transmit]
/// True when a command line (without the `C<seq>|` prefix) can put RF on the air or play a
/// recording to the transmitter. Deliberately wide: any `dvk` or `cwx` command other than
/// `cwx clear`, any `xmit` or `transmit tune` that is not clearly "off", and `atu start`.
pub fn line_may_transmit(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let verb = words.next().unwrap_or("").to_ascii_lowercase();
    let first = words.next().unwrap_or("").to_ascii_lowercase();
    let second = words.next().unwrap_or("").to_ascii_lowercase();
    let is_off = |s: &str| matches!(s, "0" | "-" | "f" | "off");
    match verb.as_str() {
        "xmit" => !is_off(&first),
        "transmit" => first == "tune" && !is_off(&second),
        "cwx" => first != "clear",
        "dvk" => true,
        "atu" => first == "start",
        _ => false,
    }
}

// [@ANCHOR: radioctl_flex_cwx_text]
/// The `cwx send` argument for a text: spaces become 0x7f ("TCPIP cwx"), the whole is quoted.
/// Refused: empty text, more than [`MAX_CW_CHARS`] characters, and anything but printable ASCII
/// other than `"` and `|`.
pub fn cwx_text(text: &str) -> Result<String, RigError> {
    let bad = |what: &str| Err(RigError::new(ErrorKind::Rejected, what.to_string()));
    if text.is_empty() {
        return bad("empty CW text");
    }
    if text.chars().count() > MAX_CW_CHARS {
        return bad("CW text too long");
    }
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            ' ' => out.push('\u{7f}'),
            '"' | '|' => return bad("CW text may not contain a quote or a bar"),
            c if c.is_ascii_graphic() => out.push(c),
            _ => return bad("CW text must be printable ASCII"),
        }
    }
    out.push('"');
    Ok(out)
}

/// A decoded `R` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The sequence number of the command.
    pub seq: u32,
    /// The hexadecimal response; zero is success.
    pub code: u32,
    /// The message part (may be empty, may contain `|`).
    pub message: String,
}

// [@ANCHOR: radioctl_flex_parse_reply]
/// Decodes `R<seq>|<hex>|message[|debug]`. The sequence is decimal and the response hexadecimal,
/// with or without a `0x` prefix.
pub fn parse_reply(line: &str) -> Option<Reply> {
    let body = line.strip_prefix('R')?;
    let mut parts = body.splitn(3, '|');
    let seq = parts.next()?.parse().ok()?;
    let code_text = parts.next()?;
    let code_text = code_text
        .strip_prefix("0x")
        .or_else(|| code_text.strip_prefix("0X"))
        .unwrap_or(code_text);
    let code = u32::from_str_radix(code_text, 16).ok()?;
    let message = parts.next().unwrap_or("").to_string();
    Some(Reply { seq, code, message })
}

// [@ANCHOR: radioctl_flex_parse_pairs]
/// The `key=value` words of a status line, in order. Words without `=` are skipped.
pub fn parse_pairs(text: &str) -> Vec<(&str, &str)> {
    text.split_whitespace()
        .filter_map(|word| word.split_once('='))
        .collect()
}

/// What `info` told us about the radio ("TCPIP info").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RadioInfo {
    /// The model, e.g. `FLEX-6300`.
    pub model: Option<String>,
    /// Firmware version text.
    pub software_ver: Option<String>,
    /// Slice receivers the radio has.
    pub slices: Option<u8>,
    /// Spectrum capture units.
    pub scus: Option<u8>,
    /// The user's name for the radio.
    pub name: Option<String>,
    /// The owner's callsign.
    pub callsign: Option<String>,
}

// [@ANCHOR: radioctl_flex_parse_info]
/// Decodes the reply of `info`: `key=value,key="quoted value",...`. Commas inside quotes are
/// kept. Unknown keys are ignored.
pub fn parse_info(text: &str) -> RadioInfo {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let (mut key, mut value) = (String::new(), String::new());
    let (mut in_value, mut quoted) = (false, false);
    let flush = |key: &mut String, value: &mut String, pairs: &mut Vec<(String, String)>| {
        let k = key.trim().to_string();
        if !k.is_empty() {
            pairs.push((k, value.trim().to_string()));
        }
        key.clear();
        value.clear();
    };
    for ch in text.chars() {
        match ch {
            '"' if in_value => quoted = !quoted,
            ',' if !quoted => {
                flush(&mut key, &mut value, &mut pairs);
                in_value = false;
            }
            '=' if !in_value => in_value = true,
            '\n' | '\r' => {}
            c if in_value => value.push(c),
            c => key.push(c),
        }
    }
    flush(&mut key, &mut value, &mut pairs);
    let find = |k: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
    };
    RadioInfo {
        model: find("model"),
        software_ver: find("software_ver"),
        slices: find("slice").and_then(|v| v.parse().ok()),
        scus: find("scu").and_then(|v| v.parse().ok()),
        name: find("name").filter(|v| !v.is_empty()),
        callsign: find("callsign").filter(|v| !v.is_empty()),
    }
}

// [@ANCHOR: radioctl_flex_parse_firmware]
/// The major and minor of the `SmartSDR-MB` entry of a `version` reply
/// (`SmartSDR-MB=3.2.31.1#PSoC-MBTRX=...`).
pub fn parse_firmware(text: &str) -> Option<(u8, u8)> {
    let entry = text
        .split('#')
        .find_map(|item| item.strip_prefix("SmartSDR-MB="))?;
    let mut numbers = entry.split('.');
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers.next()?.parse().ok()?;
    Some((major, minor))
}

/// A radio announced by the discovery broadcast.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovery {
    /// Model name.
    pub model: String,
    /// Serial number.
    pub serial: String,
    /// Firmware version.
    pub version: String,
    /// Name, with the underscores of the wire turned back into spaces.
    pub name: String,
    /// Owner callsign.
    pub callsign: String,
    /// Address to connect to.
    pub ip: String,
    /// TCP control port.
    pub port: u16,
}

/// VITA-49 stream id of a discovery packet ("Discovery protocol").
pub const DISCOVERY_STREAM_ID: u32 = 0x800;
/// VITA-49 class id of a discovery packet.
pub const DISCOVERY_CLASS_ID: u32 = 0x534C_FFFF;

// [@ANCHOR: radioctl_flex_parse_discovery]
/// Decodes a discovery datagram: a 28-byte VITA-49 extension header (header, stream id, two
/// class id words, three timestamp words) with stream id 0x800 and class id 0x534CFFFF in either
/// class word, then `model=... serial=... version=... name=... callsign=... ip=... port=...` as
/// text ended by NUL or the end of the packet.
pub fn parse_discovery(packet: &[u8]) -> Option<Discovery> {
    let word = |i: usize| -> Option<u32> {
        let at = i.checked_mul(4)?;
        let bytes: [u8; 4] = packet.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    };
    if word(1)? != DISCOVERY_STREAM_ID
        || (word(2)? != DISCOVERY_CLASS_ID && word(3)? != DISCOVERY_CLASS_ID)
    {
        return None;
    }
    let payload = packet.get(28..)?;
    let end = payload
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(payload.len());
    let text = std::str::from_utf8(payload.get(..end)?).ok()?;
    let mut out = Discovery::default();
    let mut seen_port = false;
    for (key, value) in parse_pairs(text) {
        match key {
            "model" => out.model = value.to_string(),
            "serial" => out.serial = value.to_string(),
            "version" => out.version = value.to_string(),
            "name" => out.name = value.replace('_', " "),
            "callsign" => out.callsign = value.replace('_', " "),
            "ip" => out.ip = value.to_string(),
            "port" => {
                out.port = value.parse().ok()?;
                seen_port = true;
            }
            _ => {}
        }
    }
    (seen_port && !out.model.is_empty() && !out.ip.is_empty()).then_some(out)
}

/// What the engine knows about one slice receiver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SliceState {
    /// `in_use`.
    pub in_use: Option<bool>,
    /// `active`.
    pub active: Option<bool>,
    /// `RF_frequency`.
    pub frequency: Option<Hz>,
    /// `mode` as the radio spelled it.
    pub mode_text: Option<String>,
    /// The mode, when the core vocabulary has it.
    pub mode: Option<Mode>,
    /// `filter_lo` in hertz.
    pub filter_lo_hz: Option<i64>,
    /// `filter_hi` in hertz.
    pub filter_hi_hz: Option<i64>,
    /// `audio_level`, 0 to 100.
    pub audio_level: Option<u16>,
    /// `tx`: this slice is the transmit slice.
    pub tx: Option<bool>,
    /// `antenna_id`.
    pub antenna: Option<String>,
}

/// What the engine knows about one panadapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanadapterState {
    /// `center`.
    pub center: Option<Hz>,
    /// `bandwidth`.
    pub bandwidth: Option<Hz>,
    /// `x_pixels` (the page spells it both `x-pixels` and `x_pixels`).
    pub x_pixels: Option<u32>,
    /// `y_pixels`.
    pub y_pixels: Option<u32>,
    /// `fps`.
    pub fps: Option<u32>,
}

/// What the engine knows about the transmitter and its interlock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransmitState {
    /// `interlock state=`.
    pub interlock: Option<String>,
    /// `tx_allowed`.
    pub tx_allowed: Option<bool>,
    /// `rfpower`, 0 to 100 (key assumed to match the `transmit set` parameter).
    pub rf_power: Option<u16>,
    /// `miclevel`, 0 to 100 (same assumption).
    pub mic_level: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A subscription: the reply is not shown to the actor.
    Quiet,
    /// `info`: the reply is parsed into [`RadioInfo`].
    Info,
    /// A set: the reply is the acknowledgement.
    Set,
    /// A read answered from the shadow.
    Get(Field),
    /// `version`.
    Version,
}

struct Pending {
    seq: u32,
    kind: Kind,
}

/// The FlexRadio SmartSDR protocol engine.
pub struct FlexEngine {
    desc: RigDescription,
    slice: u8,
    seq: u32,
    line: Vec<u8>,
    line_at: Duration,
    discarding: bool,
    pending: Vec<Pending>,
    awaiting: Vec<Field>,
    subscribed: bool,
    protocol_version: Option<String>,
    handle: Option<u32>,
    slices: BTreeMap<u8, SliceState>,
    pans: BTreeMap<u32, PanadapterState>,
    tx: TransmitState,
    info: RadioInfo,
    last_message: Option<(u32, String)>,
    refused: u32,
    last_refusal: Option<u32>,
}

impl FlexEngine {
    // [@ANCHOR: radioctl_flex_engine_new]
    /// Builds an engine for a Flex description; VFO A is slice 0.
    pub fn new(desc: RigDescription) -> Result<FlexEngine, RigError> {
        if desc.family != Family::Flex || desc.flex.is_none() {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a FlexRadio description",
            ));
        }
        let slice = desc.flex.as_ref().map_or(0, |f| f.slice);
        Ok(FlexEngine {
            desc,
            slice,
            seq: 0,
            line: Vec::new(),
            line_at: Duration::ZERO,
            discarding: false,
            pending: Vec::new(),
            awaiting: Vec::new(),
            subscribed: false,
            protocol_version: None,
            handle: None,
            slices: BTreeMap::new(),
            pans: BTreeMap::new(),
            tx: TransmitState::default(),
            info: RadioInfo::default(),
            last_message: None,
            refused: 0,
            last_refusal: None,
        })
    }

    /// Uses another slice receiver as VFO A.
    pub fn with_slice(mut self, index: u8) -> FlexEngine {
        self.slice = index;
        self
    }

    /// The protocol version line (`1.4.0.0`) and the client handle the radio sent on connect.
    pub fn prologue(&self) -> (Option<&str>, Option<u32>) {
        (self.protocol_version.as_deref(), self.handle)
    }

    /// The state of a slice receiver, if any status for it has been seen.
    pub fn slice_state(&self, index: u8) -> Option<&SliceState> {
        self.slices.get(&index)
    }

    /// The state of a panadapter by stream id.
    pub fn panadapter(&self, stream_id: u32) -> Option<&PanadapterState> {
        self.pans.get(&stream_id)
    }

    /// Transmitter and interlock state.
    pub fn transmit_state(&self) -> &TransmitState {
        &self.tx
    }

    /// What `info` said.
    pub fn radio_info(&self) -> &RadioInfo {
        &self.info
    }

    /// The last `M` line: its number and text.
    pub fn last_message(&self) -> Option<&(u32, String)> {
        self.last_message.as_ref()
    }

    /// How many commands the radio refused that the actor did not ask about (subscriptions,
    /// `info`), and the last such response code.
    pub fn refusals(&self) -> (u32, Option<u32>) {
        (self.refused, self.last_refusal)
    }

    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
    }

    fn next_seq(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    // [@ANCHOR: radioctl_flex_subscription_lines]
    fn subscription_lines(on: bool) -> [String; 3] {
        let verb = if on { "sub" } else { "unsub" };
        [
            format!("{verb} slice all"),
            format!("{verb} tx all"),
            format!("{verb} pan all"),
        ]
    }

    fn slice_value(&self) -> Option<&SliceState> {
        self.slices.get(&self.slice)
    }

    // [@ANCHOR: radioctl_flex_shadow_value]
    /// The observation the shadow holds for a field.
    fn shadow_value(&self, field: Field) -> Option<Observation> {
        let slice = self.slice_value();
        match field {
            Field::Frequency(Vfo::A) => Some(Observation::Frequency(Vfo::A, slice?.frequency?)),
            Field::Mode => Some(Observation::Mode(slice?.mode?)),
            Field::FilterWidth => {
                let s = slice?;
                let width = s.filter_hi_hz?.checked_sub(s.filter_lo_hz?)?;
                Some(Observation::FilterWidth(Hz(u64::try_from(width).ok()?)))
            }
            Field::Level(Level::AfGain) => {
                Some(Observation::Level(Level::AfGain, slice?.audio_level?))
            }
            Field::Level(Level::MicGain) => {
                Some(Observation::Level(Level::MicGain, self.tx.mic_level?))
            }
            Field::Level(Level::RfPower) => {
                Some(Observation::Level(Level::RfPower, self.tx.rf_power?))
            }
            Field::Ptt => Some(Observation::Ptt(interlock_keyed(
                self.tx.interlock.as_deref()?,
            ))),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_flex_engine_commands]
    /// The command lines for an op and how to treat each reply; the last one is the op's own.
    fn commands(&self, op: &Op) -> Result<Vec<(String, Kind)>, RigError> {
        let n = self.slice;
        let percent = |value: u16| {
            if value > MAX_PERCENT {
                Err(RigError::new(
                    ErrorKind::Rejected,
                    format!("level {value} is above {MAX_PERCENT}"),
                ))
            } else {
                Ok(value)
            }
        };
        let get = |field| Ok(vec![("ping".to_string(), Kind::Get(field))]);
        match op {
            Op::GetFrequency(Vfo::A) => get(Field::Frequency(Vfo::A)),
            Op::GetMode => get(Field::Mode),
            Op::GetPtt => get(Field::Ptt),
            Op::GetFilterWidth => get(Field::FilterWidth),
            Op::GetLevel(level @ (Level::AfGain | Level::MicGain | Level::RfPower)) => {
                get(Field::Level(*level))
            }
            Op::GetFirmware => Ok(vec![("version".to_string(), Kind::Version)]),
            Op::SetFrequency(Vfo::A, hz) => Ok(vec![(
                format!("slice t {n} {}", format_mhz(*hz)),
                Kind::Set,
            )]),
            Op::SetMode(mode) => {
                let name = mode_name(*mode)
                    .ok_or_else(|| Self::unsupported("no SmartSDR spelling for this mode"))?;
                Ok(vec![(format!("slice s {n} mode={name}"), Kind::Set)])
            }
            Op::SetFilterWidth(width) => {
                let lo = self
                    .slice_value()
                    .and_then(|s| s.filter_lo_hz)
                    .ok_or_else(|| {
                        RigError::new(
                            ErrorKind::Rejected,
                            "the filter's low edge has not been reported yet",
                        )
                    })?;
                let width = i64::try_from(width.0)
                    .ok()
                    .filter(|w| (1..=24_000).contains(w))
                    .ok_or_else(|| {
                        RigError::new(ErrorKind::Rejected, "filter width out of range")
                    })?;
                Ok(vec![(format!("filt {n} {lo} {}", lo + width), Kind::Set)])
            }
            Op::SetLevel(Level::AfGain, v) => Ok(vec![(
                format!("slice s {n} audio_level={}", percent(*v)?),
                Kind::Set,
            )]),
            Op::SetLevel(Level::MicGain, v) => Ok(vec![(
                format!("transmit set miclevel={}", percent(*v)?),
                Kind::Set,
            )]),
            Op::SetLevel(Level::RfPower, v) => Ok(vec![(
                format!("transmit set rfpower={}", percent(*v)?),
                Kind::Set,
            )]),
            Op::SetPtt(on) => Ok(vec![(format!("xmit {}", u8::from(*on)), Kind::Set)]),
            Op::SendCw(text) => Ok(vec![(format!("cwx send {}", cwx_text(text)?), Kind::Set)]),
            // "TCPIP cwx": `cwx clear` empties the CWX buffer (classed non-transmitting above).
            Op::StopCw => Ok(vec![("cwx clear".to_string(), Kind::Set)]),
            Op::SetTransceive(on) => {
                let mut lines: Vec<(String, Kind)> = Self::subscription_lines(*on)
                    .into_iter()
                    .map(|line| (line, Kind::Quiet))
                    .collect();
                if let Some(last) = lines.last_mut() {
                    last.1 = Kind::Set;
                }
                Ok(lines)
            }
            _ => Err(Self::unsupported("not available over the SmartSDR API")),
        }
    }

    // [@ANCHOR: radioctl_flex_apply_slice]
    /// Applies one `key=value` list of a slice status line; returns the observations.
    fn apply_slice(&mut self, index: u8, pairs: &[(&str, &str)]) -> Vec<Observation> {
        let mut out = Vec::new();
        let is_ours = index == self.slice;
        let state = self.slices.entry(index).or_default();
        let flag = |v: &str| matches!(v, "1" | "T" | "t" | "on" | "ON" | "+");
        let (mut freq_seen, mut filter_seen, mut mode_seen, mut level_seen) =
            (false, false, false, false);
        for (key, value) in pairs {
            match *key {
                "in_use" => state.in_use = Some(flag(value)),
                "active" => state.active = Some(flag(value)),
                "tx" => state.tx = Some(flag(value)),
                "antenna_id" | "rxant" => state.antenna = Some((*value).to_string()),
                "RF_frequency" => {
                    if let Some(hz) = parse_mhz(value) {
                        state.frequency = Some(hz);
                        freq_seen = true;
                    }
                }
                "mode" => {
                    state.mode_text = Some((*value).to_string());
                    state.mode = mode_from_name(value);
                    mode_seen = state.mode.is_some();
                }
                "filter_lo" => {
                    if let Some(hz) = parse_mhz_signed(value) {
                        state.filter_lo_hz = Some(hz);
                        filter_seen = true;
                    }
                }
                "filter_hi" => {
                    if let Some(hz) = parse_mhz_signed(value) {
                        state.filter_hi_hz = Some(hz);
                        filter_seen = true;
                    }
                }
                "audio_level" => {
                    if let Some(v) = value.parse::<u16>().ok().filter(|v| *v <= MAX_PERCENT) {
                        state.audio_level = Some(v);
                        level_seen = true;
                    }
                }
                _ => {}
            }
        }
        if !is_ours {
            return out;
        }
        if freq_seen {
            if let Some(hz) = state.frequency {
                out.push(Observation::Frequency(Vfo::A, hz));
            }
        }
        if mode_seen {
            if let Some(mode) = state.mode {
                out.push(Observation::Mode(mode));
            }
        }
        if filter_seen {
            if let (Some(lo), Some(hi)) = (state.filter_lo_hz, state.filter_hi_hz) {
                if let Some(width) = hi.checked_sub(lo).and_then(|w| u64::try_from(w).ok()) {
                    out.push(Observation::FilterWidth(Hz(width)));
                }
            }
        }
        if level_seen {
            if let Some(v) = state.audio_level {
                out.push(Observation::Level(Level::AfGain, v));
            }
        }
        out
    }

    // [@ANCHOR: radioctl_flex_apply_pan]
    fn apply_pan(&mut self, id: u32, pairs: &[(&str, &str)]) {
        let pan = self.pans.entry(id).or_default();
        for (key, value) in pairs {
            match *key {
                "center" => pan.center = parse_mhz(value).or(pan.center),
                "bandwidth" => pan.bandwidth = parse_mhz(value).or(pan.bandwidth),
                "x_pixels" | "x-pixels" => pan.x_pixels = value.parse().ok().or(pan.x_pixels),
                "y_pixels" | "y-pixels" => pan.y_pixels = value.parse().ok().or(pan.y_pixels),
                "fps" => pan.fps = value.parse().ok().or(pan.fps),
                _ => {}
            }
        }
    }

    // [@ANCHOR: radioctl_flex_apply_interlock]
    fn apply_interlock(&mut self, pairs: &[(&str, &str)]) -> Vec<Observation> {
        let mut out = Vec::new();
        for (key, value) in pairs {
            match *key {
                "state" => {
                    self.tx.interlock = Some((*value).to_string());
                    out.push(Observation::Ptt(interlock_keyed(value)));
                }
                "tx_allowed" => self.tx.tx_allowed = Some(*value == "1"),
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_flex_apply_transmit]
    fn apply_transmit(&mut self, pairs: &[(&str, &str)]) -> Vec<Observation> {
        let mut out = Vec::new();
        for (key, value) in pairs {
            let percent = value.parse::<u16>().ok().filter(|v| *v <= MAX_PERCENT);
            match (*key, percent) {
                ("rfpower", Some(v)) => {
                    self.tx.rf_power = Some(v);
                    out.push(Observation::Level(Level::RfPower, v));
                }
                ("miclevel", Some(v)) => {
                    self.tx.mic_level = Some(v);
                    out.push(Observation::Level(Level::MicGain, v));
                }
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_flex_engine_status]
    /// Decodes `S<handle>|object ...` and returns the observations it carries.
    fn status(&mut self, rest: &str) -> Vec<Observation> {
        let Some((_handle, body)) = rest.split_once('|') else {
            return Vec::new();
        };
        let mut words = body.splitn(2, ' ');
        let object = words.next().unwrap_or("");
        let tail = words.next().unwrap_or("");
        match object {
            "slice" => {
                let mut parts = tail.splitn(2, ' ');
                let Some(index) = parts.next().and_then(|i| i.parse::<u8>().ok()) else {
                    return Vec::new();
                };
                let pairs = parse_pairs(parts.next().unwrap_or(""));
                self.apply_slice(index, &pairs)
            }
            "display" => {
                let mut parts = tail.splitn(3, ' ');
                if parts.next() == Some("pan") {
                    let id = parts.next().and_then(|id| {
                        let hex = id.strip_prefix("0x").unwrap_or(id);
                        u32::from_str_radix(hex, 16).ok()
                    });
                    if let Some(id) = id {
                        let pairs = parse_pairs(parts.next().unwrap_or(""));
                        self.apply_pan(id, &pairs);
                    }
                }
                Vec::new()
            }
            "interlock" => self.apply_interlock(&parse_pairs(tail)),
            "transmit" => self.apply_transmit(&parse_pairs(tail)),
            _ => Vec::new(),
        }
    }

    // [@ANCHOR: radioctl_flex_answer_reply]
    fn answer_reply(&mut self, reply: Reply) -> Vec<Inbound> {
        let Some(at) = self.pending.iter().position(|p| p.seq == reply.seq) else {
            return vec![Inbound::Unrecognised(
                format!("R{}|{:X}", reply.seq, reply.code).into_bytes(),
            )];
        };
        let kind = self.pending.remove(at).kind;
        let ok = reply.code == 0;
        match kind {
            Kind::Quiet => {
                if !ok {
                    self.refused += 1;
                    self.last_refusal = Some(reply.code);
                }
                Vec::new()
            }
            Kind::Info => {
                if ok {
                    self.info = parse_info(&reply.message);
                } else {
                    self.refused += 1;
                    self.last_refusal = Some(reply.code);
                }
                Vec::new()
            }
            Kind::Set => vec![if ok { Inbound::Ack } else { Inbound::Nak }],
            Kind::Version => match parse_firmware(&reply.message).filter(|_| ok) {
                Some((major, minor)) => {
                    vec![Inbound::Observation(Observation::Firmware(major, minor))]
                }
                None => vec![Inbound::Nak],
            },
            Kind::Get(field) => {
                if !ok {
                    return vec![Inbound::Nak];
                }
                match self.shadow_value(field) {
                    Some(value) => vec![Inbound::Observation(value)],
                    None => {
                        if !self.awaiting.contains(&field) {
                            self.awaiting.push(field);
                        }
                        Vec::new()
                    }
                }
            }
        }
    }

    // [@ANCHOR: radioctl_flex_engine_line]
    fn handle_line(&mut self, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let Some(first) = line.chars().next() else {
            return Vec::new();
        };
        let rest = line.get(first.len_utf8()..).unwrap_or("");
        match first {
            'V' => {
                self.protocol_version = Some(rest.to_string());
                Vec::new()
            }
            'H' => match u32::from_str_radix(rest.trim_start_matches("0x"), 16) {
                Ok(handle) => {
                    self.handle = Some(handle);
                    Vec::new()
                }
                Err(_) => unknown(),
            },
            'R' => match parse_reply(line) {
                Some(reply) => self.answer_reply(reply),
                None => unknown(),
            },
            'S' => {
                let observations = self.status(rest);
                self.awaiting
                    .retain(|field| !observations.iter().any(|o| o.field() == *field));
                observations.into_iter().map(Inbound::Observation).collect()
            }
            'M' => {
                if let Some((number, text)) = rest.split_once('|') {
                    let number = u32::from_str_radix(number.trim_start_matches("0x"), 16);
                    if let Ok(number) = number {
                        self.last_message = Some((number, text.to_string()));
                        return Vec::new();
                    }
                }
                unknown()
            }
            _ => unknown(),
        }
    }
}

// [@ANCHOR: radioctl_flex_interlock_keyed]
/// True when an interlock state means RF is, or may still be, on the air: `PTT_REQUESTED`,
/// `TRANSMITTING` and `UNKEY_REQUESTED` (the transmitter is engaged until the unkey completes).
pub fn interlock_keyed(state: &str) -> bool {
    matches!(state, "PTT_REQUESTED" | "TRANSMITTING" | "UNKEY_REQUESTED")
}

impl ProtocolEngine for FlexEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_flex_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let mut lines = self.commands(op)?;
        // Every line that can transmit must come from an op the actor gates.
        if !op.transmits() && lines.iter().any(|(line, _)| line_may_transmit(line)) {
            return Err(RigError::new(
                ErrorKind::Rejected,
                "internal: a non-transmit op produced a transmit command",
            ));
        }
        if !self.subscribed && !matches!(op, Op::SetTransceive(_)) {
            let mut prefix: Vec<(String, Kind)> = Self::subscription_lines(true)
                .into_iter()
                .map(|line| (line, Kind::Quiet))
                .collect();
            prefix.push(("info".to_string(), Kind::Info));
            prefix.append(&mut lines);
            lines = prefix;
            self.subscribed = true;
        }
        if let Op::SetTransceive(on) = op {
            self.subscribed = *on;
        }
        let reply = match lines.last().map(|(_, kind)| *kind) {
            Some(Kind::Get(field)) => ReplyKind::Observation(field),
            Some(Kind::Version) => ReplyKind::Observation(Field::Firmware),
            _ => ReplyKind::Ack,
        };
        let mut bytes = Vec::new();
        for (line, kind) in lines {
            let seq = self.next_seq();
            if self.pending.len() >= MAX_PENDING {
                self.pending.remove(0);
            }
            self.pending.push(Pending { seq, kind });
            bytes.extend_from_slice(format!("C{seq}|{line}\n").as_bytes());
        }
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_flex_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        if !self.line.is_empty() && now.saturating_sub(self.line_at) > PARTIAL_FRAME_TIMEOUT {
            self.line.clear();
            self.discarding = false;
        }
        let mut out = Vec::new();
        for &byte in bytes {
            if byte == b'\r' || byte == b'\n' {
                if self.discarding {
                    out.push(Inbound::Unrecognised(b"line too long".to_vec()));
                    self.discarding = false;
                } else if !self.line.is_empty() {
                    let raw = std::mem::take(&mut self.line);
                    match String::from_utf8(raw) {
                        Ok(text) => out.extend(self.handle_line(&text)),
                        Err(error) => out.push(Inbound::Unrecognised(error.into_bytes())),
                    }
                }
                continue;
            }
            if self.discarding {
                continue;
            }
            if self.line.len() >= MAX_LINE {
                self.line.clear();
                self.discarding = true;
                continue;
            }
            if self.line.is_empty() {
                self.line_at = now;
            }
            self.line.push(byte);
        }
        out
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use radioctl_core::rigfile;

    const T0: Duration = Duration::from_secs(1);

    fn flex() -> RigDescription {
        rigfile::parse(include_str!("../../../rigs/flex-6300.toml"))
            .expect("flex file")
            .description
    }

    fn engine() -> FlexEngine {
        FlexEngine::new(flex()).expect("engine")
    }

    fn text(request: &Request) -> String {
        String::from_utf8(request.bytes.clone()).expect("ascii")
    }

    // Tests [@ANCHOR: radioctl_flex_parse_mhz]
    // Tests [@ANCHOR: radioctl_flex_parse_mhz_signed]
    // Tests [@ANCHOR: radioctl_flex_format_mhz]
    #[test]
    fn megahertz_text_round_trips_exactly() {
        assert_eq!(parse_mhz("14.2012"), Some(Hz(14_201_200)));
        assert_eq!(parse_mhz("0.0003"), Some(Hz(300)));
        assert_eq!(parse_mhz("7"), Some(Hz(7_000_000)));
        assert_eq!(parse_mhz("14.0742345"), Some(Hz(14_074_235)));
        assert_eq!(parse_mhz("14.123456789012345"), Some(Hz(14_123_457)));
        assert_eq!(parse_mhz(""), None);
        assert_eq!(parse_mhz("1e6"), None);
        assert_eq!(parse_mhz("-1.0"), None);
        assert_eq!(parse_mhz("123456.0"), None);
        assert_eq!(parse_mhz_signed("-0.0027"), Some(-2_700));
        assert_eq!(parse_mhz_signed("+0.0024"), Some(2_400));
        assert_eq!(format_mhz(Hz(14_201_200)), "14.201200");
        assert_eq!(format_mhz(Hz(300)), "0.000300");
        for hz in [1, 999_999, 1_000_000, 14_074_000, 54_000_000, 77_777_777] {
            assert_eq!(parse_mhz(&format_mhz(Hz(hz))), Some(Hz(hz)));
        }
    }

    // Tests [@ANCHOR: radioctl_flex_mode_name]
    // Tests [@ANCHOR: radioctl_flex_mode_from_name]
    #[test]
    fn modes_use_the_smartsdr_spellings() {
        assert_eq!(mode_name(Mode::Data), Some("digu"));
        assert_eq!(mode_name(Mode::CwR), None);
        assert_eq!(mode_from_name("USB"), Some(Mode::Usb));
        assert_eq!(mode_from_name("DIGL"), Some(Mode::DataR));
        assert_eq!(mode_from_name("RTTY"), Some(Mode::Rtty));
        assert_eq!(mode_from_name("sam"), None);
        for (mode, name) in MODE_NAMES {
            assert_eq!(mode_from_name(name), Some(mode));
        }
    }

    // Tests [@ANCHOR: radioctl_flex_parse_reply]
    #[test]
    fn replies_split_into_sequence_code_and_message() {
        let reply = parse_reply("R21|0|2|OK Slice receiver 2").expect("reply");
        assert_eq!((reply.seq, reply.code), (21, 0));
        assert_eq!(reply.message, "2|OK Slice receiver 2");
        assert_eq!(parse_reply("R5|5000002C|").expect("r").code, 0x5000_002C);
        assert_eq!(parse_reply("R5|0x50000016|x").expect("r").code, 0x5000_0016);
        assert_eq!(parse_reply("R|0|"), None);
        assert_eq!(parse_reply("R5|zz|"), None);
        assert_eq!(parse_reply("R5"), None);
    }

    // Tests [@ANCHOR: radioctl_flex_parse_pairs]
    // Tests [@ANCHOR: radioctl_flex_parse_info]
    // Tests [@ANCHOR: radioctl_flex_parse_firmware]
    #[test]
    fn info_version_and_pairs_decode() {
        assert_eq!(
            parse_pairs("in_use=1 junk RF_frequency=1.44"),
            vec![("in_use", "1"), ("RF_frequency", "1.44")]
        );
        let info = parse_info(
            "model=\"FLEX-6300\",chassis_serial=\"1234\",name=\"Shack, main\",callsign=\"K6BP\",\
             scu=1,slice=2,software_ver=3.2.31",
        );
        assert_eq!(info.model.as_deref(), Some("FLEX-6300"));
        assert_eq!(info.name.as_deref(), Some("Shack, main"));
        assert_eq!((info.scus, info.slices), (Some(1), Some(2)));
        assert_eq!(info.software_ver.as_deref(), Some("3.2.31"));
        assert_eq!(
            parse_firmware("SmartSDR-MB=0.6.0.2#PSoC-MBTRX=1.3.1.6#FPGA-MB=0.6.0.0"),
            Some((0, 6))
        );
        assert_eq!(parse_firmware("PSoC-MBTRX=1.3.1.6"), None);
    }

    // Tests [@ANCHOR: radioctl_flex_parse_discovery]
    #[test]
    fn a_discovery_packet_decodes() {
        let mut packet = Vec::new();
        for word in [
            0x5800_0010u32,
            DISCOVERY_STREAM_ID,
            0x0000_1C2D,
            DISCOVERY_CLASS_ID,
            0,
            0,
            0,
        ] {
            packet.extend_from_slice(&word.to_be_bytes());
        }
        packet.extend_from_slice(
            b"model=FLEX-6300 serial=1234-5678 version=3.2.31 name=Shack_Radio callsign=K6BP \
              ip=192.168.1.20 port=4992\0\0\0\0",
        );
        let found = parse_discovery(&packet).expect("discovery");
        assert_eq!(found.name, "Shack Radio");
        assert_eq!((found.ip.as_str(), found.port), ("192.168.1.20", 4992));
        assert_eq!(found.model, "FLEX-6300");
        let mut wrong = packet.clone();
        wrong[7] = 0x01;
        assert_eq!(parse_discovery(&wrong), None);
        assert_eq!(parse_discovery(&packet[..20]), None);
        assert_eq!(parse_discovery(b""), None);
    }

    // Tests [@ANCHOR: radioctl_flex_cwx_text]
    #[test]
    fn cw_text_is_quoted_and_cannot_carry_a_command() {
        assert_eq!(cwx_text("CQ DX").expect("ok"), "\"CQ\u{7f}DX\"");
        for bad in ["", "a\"b", "a|b", "a\nb", "caf\u{e9}", &"x".repeat(101)] {
            assert!(cwx_text(bad).is_err(), "{bad:?}");
        }
    }

    // Tests [@ANCHOR: radioctl_flex_line_may_transmit]
    #[test]
    fn the_transmit_classifier_is_wide() {
        for line in [
            "xmit 1",
            "xmit on",
            "XMIT T",
            "xmit",
            "transmit tune on",
            "transmit tune",
            "cwx send \"x\"",
            "cwx macro send 1",
            "dvk play 1",
            "atu start",
        ] {
            assert!(line_may_transmit(line), "{line}");
        }
        for line in [
            "xmit 0",
            "xmit off",
            "transmit tune off",
            "transmit set rfpower=10",
            "cwx clear",
            "slice t 0 14.2",
            "slice s 0 tx=1",
            "sub tx all",
            "ping",
        ] {
            assert!(!line_may_transmit(line), "{line}");
        }
    }

    // Tests [@ANCHOR: radioctl_flex_engine_new]
    #[test]
    fn only_a_flex_description_builds_an_engine() {
        assert!(FlexEngine::new(RigDescription::ic7300()).is_err());
        assert!(FlexEngine::new(flex()).is_ok());
    }

    // Tests [@ANCHOR: radioctl_flex_engine_encode]
    // Tests [@ANCHOR: radioctl_flex_engine_commands]
    // Tests [@ANCHOR: radioctl_flex_subscription_lines]
    #[test]
    fn the_first_command_carries_the_subscriptions_and_later_ones_do_not() {
        let mut e = engine();
        let first = e.encode(&Op::GetFrequency(Vfo::A)).expect("encode");
        assert_eq!(
            text(&first),
            "C1|sub slice all\nC2|sub tx all\nC3|sub pan all\nC4|info\nC5|ping\n"
        );
        assert_eq!(
            first.reply,
            ReplyKind::Observation(Field::Frequency(Vfo::A))
        );
        let next = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(14_074_000)))
            .expect("encode");
        assert_eq!(text(&next), "C6|slice t 0 14.074000\n");
        assert_eq!(next.reply, ReplyKind::Ack);
        let mut other = |op: Op| text(&e.encode(&op).expect("encode"));
        assert_eq!(other(Op::SetMode(Mode::Data)), "C7|slice s 0 mode=digu\n");
        assert_eq!(
            other(Op::SetLevel(Level::AfGain, 40)),
            "C8|slice s 0 audio_level=40\n"
        );
        assert_eq!(
            other(Op::SetLevel(Level::MicGain, 30)),
            "C9|transmit set miclevel=30\n"
        );
        assert_eq!(
            other(Op::SetLevel(Level::RfPower, 5)),
            "C10|transmit set rfpower=5\n"
        );
        assert_eq!(other(Op::SetPtt(false)), "C11|xmit 0\n");
        assert_eq!(other(Op::GetFirmware), "C12|version\n");
        let slice1 = FlexEngine::new(flex()).expect("engine").with_slice(1);
        let mut slice1 = slice1;
        let first = text(&slice1.encode(&Op::SetMode(Mode::Cw)).expect("encode"));
        assert!(first.ends_with("slice s 1 mode=cw\n"), "{first}");
    }

    // Tests [@ANCHOR: radioctl_flex_engine_encode]
    #[test]
    fn unsupported_and_out_of_range_ops_are_refused_before_the_wire() {
        let mut e = engine();
        assert_eq!(
            e.encode(&Op::GetSwr).expect_err("no swr").kind,
            ErrorKind::Unsupported
        );
        assert!(e.encode(&Op::SetMode(Mode::CwR)).is_err());
        assert!(e.encode(&Op::SetLevel(Level::AfGain, 101)).is_err());
        assert!(e.encode(&Op::SetLevel(Level::KeyerSpeed, 20)).is_err());
        assert!(e.encode(&Op::SendCw("a|b".into())).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::B, Hz(1))).is_err());
        assert!(e.encode(&Op::SetFilterWidth(Hz(2400))).is_err());
        assert_eq!(e.seq, 0, "nothing refused used a sequence number");
        // The stop is a plain set (after the engine's first-command prologue).
        let stop = e.encode(&Op::StopCw).expect("stop");
        assert!(stop.bytes.ends_with(b"|cwx clear\n"), "{:?}", stop.bytes);
        assert_eq!(stop.reply, ReplyKind::Ack);
    }

    // Tests [@ANCHOR: radioctl_flex_engine_feed]
    // Tests [@ANCHOR: radioctl_flex_engine_line]
    // Tests [@ANCHOR: radioctl_flex_engine_status]
    // Tests [@ANCHOR: radioctl_flex_shadow_value]
    // Tests [@ANCHOR: radioctl_flex_apply_slice]
    // Tests [@ANCHOR: radioctl_flex_answer_reply]
    #[test]
    fn a_read_is_answered_from_status_in_either_order() {
        // The R line first, then the status line it caused.
        let mut e = engine();
        e.feed(b"V1.4.0.0\nH6F4EC23D\n", T0);
        assert_eq!(e.prologue(), (Some("1.4.0.0"), Some(0x6F4E_C23D)));
        e.encode(&Op::GetFrequency(Vfo::A)).expect("encode");
        assert!(e
            .feed(
                b"R1|0|\nR2|0|\nR3|0|\nR4|0|model=\"FLEX-6300\"\nR5|0|\n",
                T0
            )
            .is_empty());
        let got = e.feed(
            b"SA4E3D1C9|slice 0 in_use=1 RF_frequency=14.2012 mode=USB\n",
            T0,
        );
        assert_eq!(
            got,
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(14_201_200))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
            ]
        );
        // The status line first, then the R line.
        e.encode(&Op::GetMode).expect("encode");
        let got = e.feed(b"R6|0|\n", T0);
        assert_eq!(
            got,
            vec![Inbound::Observation(Observation::Mode(Mode::Usb))]
        );
        assert_eq!(e.radio_info().model.as_deref(), Some("FLEX-6300"));
        assert_eq!(e.slice_state(0).and_then(|s| s.in_use), Some(true));
    }

    // Tests [@ANCHOR: radioctl_flex_engine_feed]
    #[test]
    fn sets_are_acknowledged_or_refused_and_other_slices_are_ignored() {
        let mut e = engine();
        e.encode(&Op::GetMode).expect("encode");
        e.feed(b"R1|0|\nR2|0|\nR3|0|\nR4|0|\n", T0);
        e.encode(&Op::SetFrequency(Vfo::A, Hz(7_100_000)))
            .expect("encode");
        assert_eq!(e.feed(b"R6|0|\n", T0), vec![Inbound::Ack]);
        e.encode(&Op::SetMode(Mode::Lsb)).expect("encode");
        assert_eq!(e.feed(b"R7|50000032|\r\n", T0), vec![Inbound::Nak]);
        assert!(e
            .feed(b"S1|slice 3 RF_frequency=3.5 mode=LSB\n", T0)
            .is_empty());
        assert_eq!(
            e.slice_state(3).and_then(|s| s.frequency),
            Some(Hz(3_500_000))
        );
        let unknown = e.feed(b"R99|0|\n", T0);
        assert!(matches!(unknown.as_slice(), [Inbound::Unrecognised(_)]));
    }

    // Tests [@ANCHOR: radioctl_flex_engine_feed]
    // Tests [@ANCHOR: radioctl_flex_apply_pan]
    #[test]
    fn partial_stale_oversize_and_odd_lines_are_survived() {
        let mut e = engine();
        assert!(e.feed(b"S0|slice 0 RF_freq", T0).is_empty());
        let rest = e.feed(b"uency=10.0\n", T0 + Duration::from_millis(100));
        assert_eq!(rest.len(), 1);
        assert!(e.feed(b"S0|slice 0 RF_freq", T0).is_empty());
        let stale = e.feed(b"uency=10.0\n", T0 + Duration::from_secs(2));
        assert!(matches!(stale.as_slice(), [Inbound::Unrecognised(_)]));
        let mut long = vec![b'x'; MAX_LINE + 10];
        long.push(b'\n');
        assert!(matches!(
            e.feed(&long, T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            e.feed(&[0xff, 0xfe, b'\n'], T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            e.feed(b"Zjunk\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(e.feed(b"M10000001|A message\n", T0).is_empty());
        assert_eq!(
            e.last_message(),
            Some(&(0x1000_0001, "A message".to_string()))
        );
        assert!(e
            .feed(
                b"S0|display pan 0x40000000 center=14.1 bandwidth=0.2 x_pixels=500 fps=20\n",
                T0
            )
            .is_empty());
        let pan = e.panadapter(0x4000_0000).expect("pan");
        assert_eq!(pan.center, Some(Hz(14_100_000)));
        assert_eq!((pan.x_pixels, pan.fps), (Some(500), Some(20)));
    }

    // Tests [@ANCHOR: radioctl_flex_interlock_keyed]
    // Tests [@ANCHOR: radioctl_flex_apply_interlock]
    #[test]
    fn interlock_states_map_to_the_transmit_flag() {
        let mut e = engine();
        let got = e.feed(
            b"S0|interlock state=TRANSMITTING source=SW tx_allowed=1\n",
            T0,
        );
        assert_eq!(got, vec![Inbound::Observation(Observation::Ptt(true))]);
        let got = e.feed(b"S0|interlock state=READY tx_allowed=1\n", T0);
        assert_eq!(got, vec![Inbound::Observation(Observation::Ptt(false))]);
        assert!(interlock_keyed("PTT_REQUESTED"));
        assert!(interlock_keyed("UNKEY_REQUESTED"));
        assert!(!interlock_keyed("RECEIVE"));
        assert!(!interlock_keyed("TX_FAULT"));
    }

    // Tests [@ANCHOR: radioctl_flex_engine_encode]
    // Tests [@ANCHOR: radioctl_flex_apply_transmit]
    #[test]
    fn filter_width_and_levels_come_from_status_and_set_from_the_low_edge() {
        let mut e = engine();
        e.feed(
            b"S0|slice 0 filter_lo=0.0003 filter_hi=0.0027 audio_level=50\nS0|transmit rfpower=20 miclevel=40\n",
            T0,
        );
        let got = e.shadow_value(Field::FilterWidth);
        assert_eq!(got, Some(Observation::FilterWidth(Hz(2_400))));
        assert_eq!(
            e.shadow_value(Field::Level(Level::RfPower)),
            Some(Observation::Level(Level::RfPower, 20))
        );
        assert_eq!(
            text(&e.encode(&Op::SetFilterWidth(Hz(500))).expect("encode"))
                .lines()
                .last(),
            Some("C5|filt 0 300 800")
        );
    }

    // Tests [@ANCHOR: radioctl_flex_line_may_transmit]
    // Tests [@ANCHOR: radioctl_flex_engine_encode]
    #[test]
    fn only_transmitting_ops_produce_transmitting_lines() {
        let ops = [
            Op::GetFrequency(Vfo::A),
            Op::SetFrequency(Vfo::A, Hz(14_000_000)),
            Op::GetMode,
            Op::SetMode(Mode::Usb),
            Op::GetPtt,
            Op::SetPtt(true),
            Op::SetPtt(false),
            Op::SendCw("CQ".into()),
            Op::SetTransceive(true),
            Op::SetTransceive(false),
            Op::GetFilterWidth,
            Op::SetLevel(Level::RfPower, 10),
            Op::SetLevel(Level::MicGain, 10),
            Op::GetFirmware,
        ];
        for op in ops {
            let mut e = engine();
            let request = e.encode(&op).expect("encode");
            let sent_transmit = text(&request)
                .lines()
                .filter_map(|l| l.split_once('|'))
                .any(|(_, command)| line_may_transmit(command));
            assert_eq!(sent_transmit, op.transmits(), "{op:?}");
        }
    }
}

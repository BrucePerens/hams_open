// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `rigctld` text protocol, request side: bounded line assembly and the parser. Sans-IO: bytes
//! and strings in, typed commands out; no sockets, no runtime, no rig.
//!
//! A request is one line: an optional `+` (extended response format), then a one-letter command or
//! a `\long_name`, then blank-separated arguments. Every VFO-taking command accepts an optional
//! VFO token first (`VFOA`, `VFOB`, `currVFO`, ...) in either VFO mode. Missing, surplus or
//! malformed arguments are an error (`RPRT -1`), never a wait for more input and never a guess:
//! the parser is the attack surface of a network server.

use radioctl_core::units::Mode;

use crate::names::{mode_from_hamlib, parse_vfo, VfoSel};
use crate::reply::{EINVAL, ENIMPL};

/// Longest request line accepted by default (bytes, without the newline).
pub const DEFAULT_MAX_LINE: usize = 1024;
/// Most arguments a command may carry.
const MAX_ARGS: usize = 8;
/// Longest Morse text accepted for `send_morse`.
pub const MAX_MORSE_CHARS: usize = 64;
/// Largest frequency accepted, in hertz.
pub const MAX_HZ: u64 = 99_999_999_999;

/// A parsed command, independent of the VFO token and of the response format.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `f`
    GetFreq,
    /// `F freq`
    SetFreq(u64),
    /// `m`
    GetMode,
    /// `M mode passband`
    SetMode {
        /// The mode.
        mode: Mode,
        /// Passband in hertz; 0 and -1 leave the rig's own width.
        passband: i64,
    },
    /// `v`
    GetVfo,
    /// `V vfo`
    SetVfo(VfoSel),
    /// `t`
    GetPtt,
    /// `T 0|1|2|3`
    SetPtt(u8),
    /// `s`
    GetSplitVfo,
    /// `S split txvfo`
    SetSplitVfo {
        /// Split on or off.
        on: bool,
        /// The transmit VFO.
        tx: VfoSel,
    },
    /// `i`
    GetSplitFreq,
    /// `I freq`
    SetSplitFreq(u64),
    /// `x`
    GetSplitMode,
    /// `X mode passband`
    SetSplitMode {
        /// The mode.
        mode: Mode,
        /// Passband in hertz.
        passband: i64,
    },
    /// `\get_split_freq_mode`
    GetSplitFreqMode,
    /// `\set_split_freq_mode freq mode passband`
    SetSplitFreqMode {
        /// Transmit frequency in hertz.
        freq: u64,
        /// The mode.
        mode: Mode,
        /// Passband in hertz.
        passband: i64,
    },
    /// `j`
    GetRit,
    /// `J offset`
    SetRit(i64),
    /// `z`
    GetXit,
    /// `Z offset`
    SetXit(i64),
    /// `l level`
    GetLevel(String),
    /// `L level value`
    SetLevel(String, f64),
    /// `u func`
    GetFunc(String),
    /// `U func 0|1`
    SetFunc(String, bool),
    /// `1` or `\dump_caps`
    DumpCaps,
    /// `\dump_state`
    DumpState,
    /// `\chk_vfo`
    ChkVfo,
    /// `\get_powerstat`
    GetPowerStat,
    /// `\set_powerstat n`
    SetPowerStat(i64),
    /// `\get_lock_mode`
    GetLockMode,
    /// `\set_lock_mode 0|1`
    SetLockMode(bool),
    /// `\get_info`
    GetInfo,
    /// `b text` / `\send_morse text`
    SendMorse(String),
    /// `\stop_morse`
    StopMorse,
    /// `\get_vfo_info`
    GetVfoInfo,
    /// `\get_vfo_list`
    GetVfoList,
    /// `w text` / `\send_cmd text`: the raw escape hatch, never served.
    SendCmd(String),
    /// `q` / `Q`
    Quit,
}

/// What a line turned into.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    /// The client asked for the extended (`+`) response format.
    pub extended: bool,
    /// The long name of the command, for the response header; empty when it was not recognised.
    pub name: &'static str,
    /// The VFO token the client sent, as sent, with what it names.
    pub vfo: Option<(String, VfoSel)>,
    /// The remaining arguments, blank-normalised, for the response header.
    pub args: String,
    /// The command, or the `RPRT` code to answer with.
    pub command: Result<Command, i32>,
}

/// A line from a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A text line without its newline (and without a trailing carriage return).
    Text(String),
    /// A line that is not text: invalid UTF-8 or a control character.
    Invalid,
}

/// A line grew past the limit before its newline arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineTooLong;

/// Assembles lines from a byte stream with a hard bound on the length of a line.
#[derive(Debug)]
pub struct LineBuffer {
    pending: Vec<u8>,
    max: usize,
}

impl LineBuffer {
    // [@ANCHOR: radioctld_command_linebuffer_new]
    /// A buffer that accepts lines of at most `max` bytes.
    pub fn new(max: usize) -> LineBuffer {
        LineBuffer {
            pending: Vec::new(),
            max: max.max(1),
        }
    }

    // [@ANCHOR: radioctld_command_linebuffer_has_partial]
    /// True while the start of a line has arrived but its newline has not.
    pub fn has_partial(&self) -> bool {
        !self.pending.is_empty()
    }

    // [@ANCHOR: radioctld_command_linebuffer_push]
    /// Adds received bytes and returns the lines they completed. A line longer than the limit is
    /// an error and the caller drops the connection; nothing beyond the limit is ever buffered.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Line>, LineTooLong> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.pending);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                lines.push(into_line(line));
            } else {
                if self.pending.len() >= self.max {
                    return Err(LineTooLong);
                }
                self.pending.push(byte);
            }
        }
        Ok(lines)
    }
}

// [@ANCHOR: radioctld_command_into_line]
/// Text if the bytes are UTF-8 without control characters (a tab is fine), else invalid.
fn into_line(bytes: Vec<u8>) -> Line {
    match String::from_utf8(bytes) {
        Ok(text) if text.chars().all(|c| !c.is_control() || c == '\t') => Line::Text(text),
        _ => Line::Invalid,
    }
}

/// One-letter commands and the long name each stands for.
const SHORTS: [(char, &str); 24] = [
    ('f', "get_freq"),
    ('F', "set_freq"),
    ('m', "get_mode"),
    ('M', "set_mode"),
    ('v', "get_vfo"),
    ('V', "set_vfo"),
    ('t', "get_ptt"),
    ('T', "set_ptt"),
    ('s', "get_split_vfo"),
    ('S', "set_split_vfo"),
    ('i', "get_split_freq"),
    ('I', "set_split_freq"),
    ('x', "get_split_mode"),
    ('X', "set_split_mode"),
    ('j', "get_rit"),
    ('J', "set_rit"),
    ('z', "get_xit"),
    ('Z', "set_xit"),
    ('l', "get_level"),
    ('L', "set_level"),
    ('u', "get_func"),
    ('U', "set_func"),
    ('b', "send_morse"),
    ('w', "send_cmd"),
];

/// Long names served (the one-letter forms are in [`SHORTS`]; `1`, `q` and `Q` are special).
const SERVED: [&str; 39] = [
    "get_freq",
    "set_freq",
    "get_mode",
    "set_mode",
    "get_vfo",
    "set_vfo",
    "get_ptt",
    "set_ptt",
    "get_split_vfo",
    "set_split_vfo",
    "get_split_freq",
    "set_split_freq",
    "get_split_mode",
    "set_split_mode",
    "get_split_freq_mode",
    "set_split_freq_mode",
    "get_rit",
    "set_rit",
    "get_xit",
    "set_xit",
    "get_level",
    "set_level",
    "get_func",
    "set_func",
    "dump_caps",
    "dump_state",
    "chk_vfo",
    "get_powerstat",
    "set_powerstat",
    "get_info",
    "send_morse",
    "stop_morse",
    "get_vfo_info",
    "get_vfo_list",
    "send_cmd",
    "quit",
    "get_rig_info",
    "get_lock_mode",
    "set_lock_mode",
];

/// Hamlib commands radioctld knows by name and does not implement: `RPRT -4`.
const UNIMPLEMENTED: [&str; 30] = [
    "get_dcd",
    "get_rptr_shift",
    "set_rptr_shift",
    "get_rptr_offs",
    "set_rptr_offs",
    "get_ctcss_tone",
    "set_ctcss_tone",
    "get_dcs_code",
    "set_dcs_code",
    "get_ctcss_sql",
    "set_ctcss_sql",
    "get_dcs_sql",
    "set_dcs_sql",
    "get_mem",
    "set_mem",
    "get_channel",
    "set_channel",
    "get_ant",
    "set_ant",
    "get_ts",
    "set_ts",
    "get_parm",
    "set_parm",
    "vfo_op",
    "scan",
    "get_trn",
    "set_trn",
    "send_dtmf",
    "recv_dtmf",
    "reset",
];

/// One-letter forms of [`UNIMPLEMENTED`] commands and a few more Hamlib letters.
const UNIMPLEMENTED_SHORTS: &str = "aAcCdDeEgGhHkKnNoOpPrRyY2345678@";

// [@ANCHOR: radioctld_command_resolve]
/// The canonical long name of a command word, and whether radioctld implements it. `None` when
/// the word is not a command at all.
fn resolve(word: &str, long_form: bool) -> Option<(&'static str, bool)> {
    if long_form {
        if let Some(name) = SERVED.iter().find(|n| **n == word) {
            return Some((name, true));
        }
        return UNIMPLEMENTED
            .iter()
            .find(|n| **n == word)
            .map(|name| (*name, false));
    }
    let mut chars = word.chars();
    let (first, rest) = (chars.next()?, chars.next());
    if rest.is_some() {
        return None;
    }
    if let Some((_, name)) = SHORTS.iter().find(|(c, _)| *c == first) {
        return Some((name, true));
    }
    match first {
        '1' => Some(("dump_caps", true)),
        'q' | 'Q' => Some(("quit", true)),
        c if UNIMPLEMENTED_SHORTS.contains(c) => Some(("unimplemented", false)),
        _ => None,
    }
}

// [@ANCHOR: radioctld_command_takes_vfo]
/// True for commands that accept an optional leading VFO token.
fn takes_vfo(name: &str) -> bool {
    !matches!(
        name,
        "dump_caps"
            | "dump_state"
            | "chk_vfo"
            | "get_info"
            | "get_rig_info"
            | "get_vfo_list"
            | "quit"
            | "set_vfo"
            | "set_powerstat"
            | "get_lock_mode"
            | "set_lock_mode"
    )
}

// [@ANCHOR: radioctld_command_parse_hz]
/// A frequency in hertz from decimal text such as `14074000` or `14074000.000000`; a fraction of
/// a hertz is dropped. No sign, no exponent, at most [`MAX_HZ`].
pub fn parse_hz(text: &str) -> Option<u64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || whole.len() > 12
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 12
    {
        return None;
    }
    let hz: u64 = whole.parse().ok()?;
    (hz <= MAX_HZ).then_some(hz)
}

// [@ANCHOR: radioctld_command_parse_int]
/// A signed integer of at most 12 characters.
pub fn parse_int(text: &str) -> Option<i64> {
    if text.len() > 12 {
        return None;
    }
    text.parse().ok()
}

// [@ANCHOR: radioctld_command_parse_float]
/// A finite floating-point number of at most 32 characters.
pub fn parse_float(text: &str) -> Option<f64> {
    if text.len() > 32 {
        return None;
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

// [@ANCHOR: radioctld_command_sanitise]
/// Text safe to echo in a response header: control characters become `?`.
fn sanitise(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

// [@ANCHOR: radioctld_command_morse_text]
/// Morse text for `send_morse`: 1 to [`MAX_MORSE_CHARS`] printable ASCII characters.
fn morse_text(text: &str) -> Result<String, i32> {
    let ok = !text.is_empty()
        && text.len() <= MAX_MORSE_CHARS
        && text.bytes().all(|b| (0x20..0x7f).contains(&b));
    if ok {
        Ok(text.to_string())
    } else {
        Err(EINVAL)
    }
}

// [@ANCHOR: radioctld_command_mode_and_passband]
/// `mode passband` arguments.
fn mode_and_passband(args: &[&str]) -> Result<(Mode, i64), i32> {
    match args {
        [mode, passband] => {
            let mode = mode_from_hamlib(mode).ok_or(EINVAL)?;
            let passband = parse_int(passband).ok_or(EINVAL)?;
            Ok((mode, passband))
        }
        _ => Err(EINVAL),
    }
}

// [@ANCHOR: radioctld_command_flag]
/// A 0/1 flag; any nonzero integer is on.
fn flag(text: &str) -> Result<bool, i32> {
    parse_int(text).map(|v| v != 0).ok_or(EINVAL)
}

// [@ANCHOR: radioctld_command_build]
/// Builds the command `name` from its arguments (the VFO token already taken off). `rest` is the
/// argument text with its internal spacing, for the commands that carry free text.
fn build(name: &str, args: &[&str], rest: &str) -> Result<Command, i32> {
    let no_args = |command: Command| {
        if args.is_empty() {
            Ok(command)
        } else {
            Err(EINVAL)
        }
    };
    match (name, args) {
        ("get_freq", _) => no_args(Command::GetFreq),
        ("set_freq", [f]) => parse_hz(f).map(Command::SetFreq).ok_or(EINVAL),
        ("get_mode", _) => no_args(Command::GetMode),
        ("set_mode", _) => {
            let (mode, passband) = mode_and_passband(args)?;
            Ok(Command::SetMode { mode, passband })
        }
        ("get_vfo", _) => no_args(Command::GetVfo),
        ("set_vfo", [v]) => parse_vfo(v).map(Command::SetVfo).ok_or(EINVAL),
        ("get_ptt", _) => no_args(Command::GetPtt),
        ("set_ptt", [p]) => match parse_int(p) {
            Some(v @ 0..=3) => Ok(Command::SetPtt(v as u8)),
            _ => Err(EINVAL),
        },
        ("get_split_vfo", _) => no_args(Command::GetSplitVfo),
        ("set_split_vfo", [on, tx]) => Ok(Command::SetSplitVfo {
            on: flag(on)?,
            tx: parse_vfo(tx).ok_or(EINVAL)?,
        }),
        ("get_split_freq", _) => no_args(Command::GetSplitFreq),
        ("set_split_freq", [f]) => parse_hz(f).map(Command::SetSplitFreq).ok_or(EINVAL),
        ("get_split_mode", _) => no_args(Command::GetSplitMode),
        ("set_split_mode", _) => {
            let (mode, passband) = mode_and_passband(args)?;
            Ok(Command::SetSplitMode { mode, passband })
        }
        ("get_split_freq_mode", _) => no_args(Command::GetSplitFreqMode),
        ("set_split_freq_mode", [f, m, p]) => {
            let (mode, passband) = mode_and_passband(&[m, p])?;
            Ok(Command::SetSplitFreqMode {
                freq: parse_hz(f).ok_or(EINVAL)?,
                mode,
                passband,
            })
        }
        ("get_rit", _) => no_args(Command::GetRit),
        ("set_rit", [v]) => parse_int(v).map(Command::SetRit).ok_or(EINVAL),
        ("get_xit", _) => no_args(Command::GetXit),
        ("set_xit", [v]) => parse_int(v).map(Command::SetXit).ok_or(EINVAL),
        ("get_level", [l]) => Ok(Command::GetLevel((*l).to_string())),
        ("set_level", [l, v]) => Ok(Command::SetLevel(
            (*l).to_string(),
            parse_float(v).ok_or(EINVAL)?,
        )),
        ("get_func", [f]) => Ok(Command::GetFunc((*f).to_string())),
        ("set_func", [f, v]) => Ok(Command::SetFunc((*f).to_string(), flag(v)?)),
        ("dump_caps", _) => no_args(Command::DumpCaps),
        ("dump_state", _) => no_args(Command::DumpState),
        ("chk_vfo", _) => no_args(Command::ChkVfo),
        ("get_powerstat", _) => no_args(Command::GetPowerStat),
        ("set_powerstat", [v]) => parse_int(v).map(Command::SetPowerStat).ok_or(EINVAL),
        ("get_lock_mode", _) => no_args(Command::GetLockMode),
        ("set_lock_mode", [v]) => flag(v).map(Command::SetLockMode),
        ("get_info" | "get_rig_info", _) => no_args(Command::GetInfo),
        ("send_morse", _) => morse_text(rest).map(Command::SendMorse),
        ("stop_morse", _) => no_args(Command::StopMorse),
        ("get_vfo_info", _) => no_args(Command::GetVfoInfo),
        ("get_vfo_list", _) => no_args(Command::GetVfoList),
        ("send_cmd", _) => Ok(Command::SendCmd(rest.to_string())),
        ("quit", _) => no_args(Command::Quit),
        _ => Err(EINVAL),
    }
}

// [@ANCHOR: radioctld_command_take_vfo]
/// Splits an optional leading VFO token off `rest`. For `send_morse` only the unambiguous
/// `VFOA`, `VFOB` and `currVFO` are taken, and only when text follows, so Morse text such as
/// "TX test" is never mistaken for a VFO.
fn take_vfo<'a>(name: &str, rest: &'a str) -> (Option<(String, VfoSel)>, &'a str) {
    let (first, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let selected = match parse_vfo(first) {
        Some(sel) if name != "send_morse" => Some(sel),
        Some(sel) if matches!(first, "VFOA" | "VFOB" | "currVFO") && !after.trim().is_empty() => {
            Some(sel)
        }
        _ => None,
    };
    match selected {
        Some(sel) => (Some((first.to_string(), sel)), after.trim_start()),
        None => (None, rest),
    }
}

// [@ANCHOR: radioctld_command_parse_line]
/// Parses one request line. `None` for a blank line (nothing to answer).
pub fn parse_line(line: &str) -> Option<Parsed> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (extended, line) = match line.strip_prefix('+') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, line),
    };
    let failure = |code: i32| Parsed {
        extended,
        name: "",
        vfo: None,
        args: String::new(),
        command: Err(code),
    };
    let (word, long_form, rest) = if let Some(long) = line.strip_prefix('\\') {
        let (word, rest) = long.split_once(char::is_whitespace).unwrap_or((long, ""));
        (word, true, rest.trim_start())
    } else {
        let mut chars = line.chars();
        let Some(first) = chars.next() else {
            return Some(failure(EINVAL));
        };
        let end = first.len_utf8();
        (
            line.get(..end).unwrap_or(""),
            false,
            line.get(end..).unwrap_or("").trim_start(),
        )
    };
    let Some((name, implemented)) = resolve(word, long_form) else {
        return Some(failure(EINVAL));
    };
    if !implemented {
        return Some(Parsed {
            extended,
            name: if name == "unimplemented" { "" } else { name },
            vfo: None,
            args: sanitise(rest),
            command: Err(ENIMPL),
        });
    }
    let (vfo, rest) = if takes_vfo(name) {
        take_vfo(name, rest)
    } else {
        (None, rest)
    };
    let tokens: Vec<&str> = rest.split_whitespace().take(MAX_ARGS + 1).collect();
    let command = if tokens.len() > MAX_ARGS {
        Err(EINVAL)
    } else {
        build(name, &tokens, rest.trim_end())
    };
    Some(Parsed {
        extended,
        name,
        vfo,
        args: sanitise(&tokens.join(" ")),
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &str) -> Line {
        Line::Text(line.to_string())
    }

    // Tests [@ANCHOR: radioctld_command_linebuffer_new]
    // Tests [@ANCHOR: radioctld_command_linebuffer_push]
    // Tests [@ANCHOR: radioctld_command_linebuffer_has_partial]
    // Tests [@ANCHOR: radioctld_command_into_line]
    #[test]
    fn lines_are_assembled_across_reads_and_crlf_is_trimmed() {
        let mut buffer = LineBuffer::new(32);
        assert_eq!(buffer.push(b"f\nF 140"), Ok(vec![text("f")]));
        assert!(buffer.has_partial());
        assert_eq!(
            buffer.push(b"74000\r\n\n"),
            Ok(vec![text("F 14074000"), text("")])
        );
        assert!(!buffer.has_partial());
        assert_eq!(
            buffer.push(b"a\x00b\n\xff\n"),
            Ok(vec![Line::Invalid, Line::Invalid])
        );
    }

    // Tests [@ANCHOR: radioctld_command_linebuffer_push]
    #[test]
    fn a_line_over_the_limit_is_an_error_and_is_never_buffered() {
        let mut buffer = LineBuffer::new(8);
        assert_eq!(buffer.push(b"12345678\n"), Ok(vec![text("12345678")]));
        assert_eq!(buffer.push(b"123456789"), Err(LineTooLong));
        let mut flood = LineBuffer::new(8);
        assert_eq!(flood.push(&[b'x'; 100_000]), Err(LineTooLong));
        assert!(flood.pending.len() <= 8);
    }

    // Tests [@ANCHOR: radioctld_command_parse_hz]
    #[test]
    fn frequencies_are_decimal_hertz_with_an_optional_dropped_fraction() {
        assert_eq!(parse_hz("14074000"), Some(14_074_000));
        assert_eq!(parse_hz("14074000.000000"), Some(14_074_000));
        assert_eq!(parse_hz("14074000.9"), Some(14_074_000));
        assert_eq!(parse_hz("-1"), None);
        assert_eq!(parse_hz("1e6"), None);
        assert_eq!(parse_hz(".5"), None);
        assert_eq!(parse_hz(""), None);
        assert_eq!(parse_hz("100000000000"), None);
        assert_eq!(parse_hz("99999999999"), Some(MAX_HZ));
        assert_eq!(parse_hz("0x10"), None);
    }

    // Tests [@ANCHOR: radioctld_command_parse_int]
    // Tests [@ANCHOR: radioctld_command_parse_float]
    #[test]
    fn integers_and_floats_are_bounded_and_finite() {
        assert_eq!(parse_int("-120"), Some(-120));
        assert_eq!(parse_int("1.5"), None);
        assert_eq!(parse_int("1234567890123"), None);
        assert_eq!(parse_float("0.5"), Some(0.5));
        assert_eq!(parse_float("nan"), None);
        assert_eq!(parse_float("inf"), None);
        assert_eq!(parse_float("1e999"), None);
        assert_eq!(parse_float(&"1".repeat(40)), None);
    }

    // Tests [@ANCHOR: radioctld_command_parse_line]
    // Tests [@ANCHOR: radioctld_command_resolve]
    // Tests [@ANCHOR: radioctld_command_build]
    #[test]
    fn the_common_commands_parse_with_and_without_a_vfo_token() {
        let cmd = |line: &str| parse_line(line).expect("a command").command;
        assert_eq!(cmd("f"), Ok(Command::GetFreq));
        assert_eq!(cmd("f VFOA"), Ok(Command::GetFreq));
        assert_eq!(cmd("F 14074000"), Ok(Command::SetFreq(14_074_000)));
        assert_eq!(cmd("F VFOB 7074000.5"), Ok(Command::SetFreq(7_074_000)));
        assert_eq!(cmd("\\set_freq currVFO 1000"), Ok(Command::SetFreq(1000)));
        assert_eq!(
            cmd("M USB 2400"),
            Ok(Command::SetMode {
                mode: Mode::Usb,
                passband: 2400
            })
        );
        assert_eq!(
            cmd("M VFOA PKTUSB 3000"),
            Ok(Command::SetMode {
                mode: Mode::Data,
                passband: 3000
            })
        );
        assert_eq!(cmd("T 1"), Ok(Command::SetPtt(1)));
        assert_eq!(cmd("t"), Ok(Command::GetPtt));
        assert_eq!(
            cmd("S 1 VFOB"),
            Ok(Command::SetSplitVfo {
                on: true,
                tx: VfoSel::B
            })
        );
        assert_eq!(
            cmd("S VFOA 0 VFOA"),
            Ok(Command::SetSplitVfo {
                on: false,
                tx: VfoSel::A
            })
        );
        assert_eq!(cmd("l KEYSPD"), Ok(Command::GetLevel("KEYSPD".into())));
        assert_eq!(cmd("L AF 0.5"), Ok(Command::SetLevel("AF".into(), 0.5)));
        assert_eq!(cmd("U RIT 1"), Ok(Command::SetFunc("RIT".into(), true)));
        assert_eq!(cmd("1"), Ok(Command::DumpCaps));
        assert_eq!(cmd("\\dump_state"), Ok(Command::DumpState));
        assert_eq!(cmd("\\chk_vfo"), Ok(Command::ChkVfo));
        assert_eq!(cmd("q"), Ok(Command::Quit));
        assert_eq!(cmd("Q"), Ok(Command::Quit));
        assert_eq!(cmd("v"), Ok(Command::GetVfo));
        assert_eq!(cmd("V VFOB"), Ok(Command::SetVfo(VfoSel::B)));
        assert_eq!(cmd("J -120"), Ok(Command::SetRit(-120)));
        assert_eq!(cmd("\\get_lock_mode"), Ok(Command::GetLockMode));
        assert_eq!(cmd("\\set_lock_mode 1"), Ok(Command::SetLockMode(true)));
    }

    // Tests [@ANCHOR: radioctld_command_parse_line]
    // Tests [@ANCHOR: radioctld_command_sanitise]
    #[test]
    fn the_extended_prefix_the_echo_and_blank_lines() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   \t "), None);
        let p = parse_line("+F   VFOA  14074000").expect("parsed");
        assert!(p.extended);
        assert_eq!(p.name, "set_freq");
        assert_eq!(p.vfo, Some(("VFOA".to_string(), VfoSel::A)));
        assert_eq!(p.args, "14074000");
        let p = parse_line("+\\chk_vfo").expect("parsed");
        assert_eq!((p.extended, p.name), (true, "chk_vfo"));
        assert_eq!(sanitise("a\u{7f}b"), "a?b");
    }

    // Tests [@ANCHOR: radioctld_command_parse_line]
    // Tests [@ANCHOR: radioctld_command_mode_and_passband]
    // Tests [@ANCHOR: radioctld_command_flag]
    #[test]
    fn bad_arguments_are_einval_and_never_wait_for_more() {
        let code = |line: &str| {
            parse_line(line)
                .expect("a command")
                .command
                .expect_err("an error")
        };
        for line in [
            "F",
            "F abc",
            "F 1 2",
            "F -5",
            "M",
            "M USB",
            "M FOO 1",
            "M USB x",
            "T",
            "T 4",
            "T x",
            "T 1 2",
            "S 1",
            "S x VFOB",
            "S 1 SOMEWHERE",
            "L AF",
            "L AF nan",
            "U NB",
            "U NB x",
            "f extra",
            "J x",
            "V",
            "V 12",
            "\\get_freq 1 2 3 4 5 6 7 8 9",
            "I x",
        ] {
            assert_eq!(code(line), EINVAL, "{line}");
        }
        assert_eq!(code("zzz-no-such"), EINVAL);
        assert_eq!(code("é"), EINVAL);
        assert_eq!(code("\\no_such_command"), EINVAL);
        assert_eq!(code("\\"), EINVAL);
        assert_eq!(code("+"), EINVAL);
    }

    // Tests [@ANCHOR: radioctld_command_parse_line]
    #[test]
    fn known_but_unimplemented_commands_are_enimpl() {
        for line in [
            "\\get_dcd",
            "\\get_ant 0",
            "c",
            "\\vfo_op CPY",
            "r",
            "+\\reset",
        ] {
            let p = parse_line(line).expect("a command");
            assert_eq!(p.command, Err(ENIMPL), "{line}");
        }
    }

    // Tests [@ANCHOR: radioctld_command_takes_vfo]
    // Tests [@ANCHOR: radioctld_command_take_vfo]
    // Tests [@ANCHOR: radioctld_command_morse_text]
    #[test]
    fn morse_text_is_bounded_and_not_mistaken_for_a_vfo() {
        let cmd = |line: &str| parse_line(line).expect("a command").command;
        assert_eq!(cmd("b TX test"), Ok(Command::SendMorse("TX test".into())));
        assert_eq!(
            cmd("b VFOA CQ  CQ"),
            Ok(Command::SendMorse("CQ  CQ".into()))
        );
        assert_eq!(cmd("b"), Err(EINVAL));
        assert_eq!(cmd("b VFOA"), Ok(Command::SendMorse("VFOA".into())));
        assert_eq!(cmd(&format!("b {}", "A".repeat(65))), Err(EINVAL));
        assert!(cmd(&format!("b {}", "A".repeat(64))).is_ok());
        assert!(!takes_vfo("dump_state"));
        assert!(takes_vfo("get_freq"));
    }

    // Tests [@ANCHOR: radioctld_command_parse_line]
    #[test]
    fn raw_send_cmd_is_parsed_so_that_it_can_be_refused() {
        let p = parse_line("w FA;").expect("a command");
        assert_eq!(p.command, Ok(Command::SendCmd("FA;".into())));
    }
}

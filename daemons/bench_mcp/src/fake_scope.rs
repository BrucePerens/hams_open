// SPDX-License-Identifier: AGPL-3.0-or-later
//! A fake Siglent SDS1000X-E SCPI server for tests: a TCP server that keeps the settings the programming guide documents (EN02E, the
//! SDS1000X-E column) and answers like the guide shows (`<header> <value>`, E-notation with a unit, `#9` waveform blocks). It is the
//! test double for every scope tool that has no real scope to talk to; it never touches 192.168.10.51. What the real unit answers
//! differently from the guide is listed in the to-do `bench-mcp-scope-remaining-functions` ("lab confirms").

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How a setting is stored and validated.
#[derive(Clone, Copy)]
enum Kind {
    /// One of these words (compared in upper case); anything else is ignored, as the scope does.
    Enum(&'static [&'static str]),
    /// A number with an optional SI prefix; stored and answered in E-notation followed by this unit.
    Num(&'static str),
    /// `KEY,value,KEY,value` pairs; any subset may be set, the answer lists all keys in this order.
    Pairs,
    /// Stored as sent.
    Raw,
}

const ON_OFF: Kind = Kind::Enum(&["ON", "OFF"]);

/// Setting, kind, power-on value. A key with a channel or bus prefix (`C1:VDIV`, `MATH:INVS`) is looked up by what follows the colon.
const TABLE: &[(&str, Kind, &str)] = &[
    (
        "ACQW",
        Kind::Enum(&["SAMPLING", "PEAK_DETECT", "AVERAGE", "ERES"]),
        "SAMPLING",
    ),
    ("AVGA", Kind::Raw, "16"),
    (
        "MSIZ",
        Kind::Enum(&["7K", "70K", "700K", "7M", "14K", "140K", "1.4M", "14M"]),
        "14K",
    ),
    (
        "TRIG_MODE",
        Kind::Enum(&["AUTO", "NORM", "SINGLE", "STOP"]),
        "AUTO",
    ),
    ("TDIV", Kind::Num("S"), "1.00E-03S"),
    ("VDIV", Kind::Num("V"), "5.00E-01V"),
    ("OFST", Kind::Num("V"), "0.00E+00V"),
    (
        "CPL",
        Kind::Enum(&["A1M", "A50", "D1M", "D50", "GND"]),
        "D1M",
    ),
    ("ATTN", Kind::Raw, "1"),
    ("INVS", ON_OFF, "OFF"),
    ("TRA", ON_OFF, "ON"),
    ("BWL", Kind::Pairs, "C1,OFF,C2,OFF"),
    (
        "SCSV",
        Kind::Enum(&["OFF", "1MIN", "5MIN", "10MIN", "30MIN", "60MIN"]),
        "30MIN",
    ),
    ("GRDS", Kind::Enum(&["FULL", "HALF", "OFF"]), "FULL"),
    ("INTS", Kind::Pairs, "TRACE,50,GRID,50"),
    (
        "PESU",
        Kind::Enum(&["OFF", "INFINITE", "1", "5", "10", "30"]),
        "OFF",
    ),
    ("DTJN", ON_OFF, "OFF"),
    ("MENU", ON_OFF, "ON"),
    ("XYDS", ON_OFF, "OFF"),
    ("HSMD", ON_OFF, "OFF"),
    ("HSLST", ON_OFF, "OFF"),
    ("MTVD", Kind::Num("V"), "1.00E+00V"),
    ("MTVP", Kind::Raw, "0"),
    (
        "FFTW",
        Kind::Enum(&["RECT", "BLAC", "HANN", "HAMM", "FLATTOP"]),
        "HANN",
    ),
    ("FFTC", Kind::Num("Hz"), "5.00E+08Hz"),
    ("FFTF", Kind::Enum(&["OFF", "ON", "EXCLU"]), "OFF"),
    ("FFTP", Kind::Num("V"), "0.00E+00V"),
    ("FFTS", Kind::Num(""), "1.00E+01"),
    ("FFTT", Kind::Raw, "100.00MHz"),
    ("FFTU", Kind::Enum(&["VRMS", "DBM", "DBVRMS"]), "DBVRMS"),
    ("CRMS", Kind::Enum(&["OFF", "MANUAL", "TRACK"]), "OFF"),
    ("CRTY", Kind::Enum(&["X", "Y", "X-Y"]), "X"),
    ("MEGS", ON_OFF, "OFF"),
    ("MEGA", Kind::Num("S"), "0.00E+00S"),
    ("MEGB", Kind::Num("S"), "0.00E+00S"),
    ("DCST", ON_OFF, "OFF"),
    ("PFEN", ON_OFF, "OFF"),
    ("PFDS", ON_OFF, "OFF"),
    ("PFFS", ON_OFF, "OFF"),
    ("PFBF", ON_OFF, "OFF"),
    ("PFSC", Kind::Enum(&["C1", "C2"]), "C1"),
    ("PFST", Kind::Pairs, "XMASK,0.04,YMASK,0.04"),
    (
        "REFLA",
        Kind::Enum(&["REFA", "REFB", "REFC", "REFD"]),
        "REFA",
    ),
    ("REFSR", Kind::Enum(&["C1", "C2", "MATH"]), "C1"),
    ("REFDS", ON_OFF, "OFF"),
    ("REFPO", Kind::Num("V"), "0.00E+00V"),
    ("REFSC", Kind::Num("V"), "5.00E-01V"),
    ("TRSE", Kind::Raw, "EDGE,SR,C1,HT,OFF"),
    ("TRPA", Kind::Raw, "C1,X,C2,X,STATE,AND"),
];

/// Commands under these prefixes (the serial trigger and the serial decode) are stored and answered as sent.
const BUS_PREFIXES: &[&str] = &[
    "TRIIC:", "TRSPI:", "TRUART:", "TRCAN:", "TRLIN:", "B1:DC", "B2:DC",
];

/// What the fake does besides answering.
#[derive(Clone, Default)]
pub struct Opts {
    /// New connections made before this long has passed are reset on their first line, as the real scope did while it held a dead session.
    pub stale_for: Duration,
    /// Answer `PAVA? STATn` in one line instead of the guide's two (the real unit's framing is unconfirmed).
    pub stat_one_line: bool,
}

#[derive(Clone)]
struct State {
    vals: HashMap<String, String>,
    nums: HashMap<String, f64>,
    custom: Vec<(String, String)>,
    frame: u32,
    pf: (bool, u32, u32),
    mask: bool,
    ref_saved: Vec<String>,
}

/// A running fake scope: the port it listens on and every line received, in order.
pub struct FakeScope {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<String>>>,
}

/// The number part and the unit of `1.5MHz`, `-3us`, `500mV` or `0.2`, as (value, unit text). Lower-case `m` is milli, upper-case `M` mega.
fn parse_num(s: &str) -> Option<(f64, String)> {
    let s = s.trim();
    let b = s.as_bytes();
    let mut end = 0;
    while end < b.len() {
        let c = b[end];
        let exp = matches!(c, b'e' | b'E')
            && b.get(end + 1)
                .is_some_and(|n| n.is_ascii_digit() || matches!(n, b'+' | b'-'));
        if !(c.is_ascii_digit()
            || c == b'.'
            || exp
            || (matches!(c, b'+' | b'-') && (end == 0 || matches!(b[end - 1], b'e' | b'E'))))
        {
            break;
        }
        end += 1;
    }
    let v: f64 = s[..end].parse().ok()?;
    let rest = &s[end..];
    let (mult, unit) = match rest.chars().next() {
        Some('p') => (1e-12, &rest[1..]),
        Some('n') => (1e-9, &rest[1..]),
        Some('u') => (1e-6, &rest[1..]),
        Some('m') => (1e-3, &rest[1..]),
        Some('k' | 'K') => (1e3, &rest[1..]),
        Some('M') => (1e6, &rest[1..]),
        Some('G') => (1e9, &rest[1..]),
        _ => (1.0, rest),
    };
    Some((v * mult, unit.to_string()))
}

/// `1.00E-03` style: two decimals, sign and at least two exponent digits, then the unit.
fn e(v: f64, unit: &str) -> String {
    let s = format!("{v:.2E}");
    let (m, x) = s.split_once('E').unwrap();
    let x: i32 = x.parse().unwrap();
    format!("{m}E{}{:02}{unit}", if x < 0 { '-' } else { '+' }, x.abs())
}

/// Voltage of channel `ch` at fractional sample `x` (0 to 1000): a 1 kHz sine of an amplitude that falls frame by frame in history mode;
/// channel 2 is the same sine a quarter period later.
fn volts(st: &State, ch: u8, x: f64) -> f64 {
    let amp_code = (100.0 - f64::from(st.frame)).max(10.0);
    let vdiv = parse_num(
        st.vals
            .get(&format!("C{ch}:VDIV"))
            .map_or("5.00E-01V", |v| v),
    )
    .unwrap()
    .0;
    let phase = if ch == 2 {
        std::f64::consts::FRAC_PI_2
    } else {
        0.0
    };
    amp_code * vdiv / 25.0 * (2.0 * std::f64::consts::PI * x / 100.0 + phase).sin()
}

fn block(header: &str, data: &[u8]) -> Vec<u8> {
    let mut r = format!("{header},#9{:09}", data.len()).into_bytes();
    r.extend(data);
    r.extend(b"\n\n");
    r
}

/// The value of a measurement item on a channel for the model signal.
fn pava(st: &State, ch: u8, item: &str) -> Option<String> {
    let a = (0..1000)
        .map(|k| volts(st, ch, f64::from(k)).abs())
        .fold(0.0, f64::max);
    let (v, u) = match item {
        "PKPK" | "AMPL" => (2.0 * a, "V"),
        "MAX" | "TOP" => (a, "V"),
        "MIN" | "BASE" => (-a, "V"),
        "MEAN" | "CMEAN" | "LEVELX" => (0.0, "V"),
        "STDEV" | "VSTD" | "RMS" | "CRMS" => (a / 2f64.sqrt(), "V"),
        "OVSN" | "FPRE" | "OVSP" | "RPRE" => (0.0, "%"),
        "DUTY" | "NDUTY" => (50.0, "%"),
        "PER" | "WID" => (1e-3, "S"),
        "PWID" | "NWID" => (5e-4, "S"),
        "RISE" | "FALL" | "DELAY" | "TIMEL" => (1e-6, "S"),
        "FREQ" => (1e3, "Hz"),
        _ => return None,
    };
    Some(e(v, u))
}

/// A stored cursor position (seconds or volts), 0 until set.
fn num(st: &State, k: &str) -> f64 {
    st.nums.get(k).copied().unwrap_or(0.0)
}

impl State {
    fn new() -> State {
        let mut vals = HashMap::new();
        for ch in [1, 2] {
            for (k, _, d) in TABLE
                .iter()
                .filter(|t| matches!(t.0, "VDIV" | "OFST" | "CPL" | "ATTN" | "INVS" | "TRA"))
            {
                vals.insert(format!("C{ch}:{k}"), (*d).to_string());
            }
        }
        for (k, _, d) in TABLE {
            vals.insert((*k).to_string(), (*d).to_string());
        }
        State {
            vals,
            nums: HashMap::new(),
            custom: Vec::new(),
            frame: 0,
            pf: (false, 0, 0),
            mask: false,
            ref_saved: Vec::new(),
        }
    }
}

fn spec(key: &str) -> Option<(Kind, &'static str)> {
    let rest = key.split_once(':').map_or(key, |(_, r)| r);
    TABLE.iter().find(|t| t.0 == rest).map(|t| (t.1, t.2))
}

/// Applies a set command to the stored value; a value the scope would not take leaves the old one.
fn set(st: &mut State, key: &str, arg: &str) {
    if BUS_PREFIXES.iter().any(|p| key.starts_with(p)) {
        st.vals.insert(key.to_string(), arg.to_string());
        return;
    }
    let Some((kind, default)) = spec(key) else {
        return;
    };
    let cur = st
        .vals
        .get(key)
        .cloned()
        .unwrap_or_else(|| default.to_string());
    let new = match kind {
        Kind::Enum(words) => words
            .iter()
            .find(|w| w.eq_ignore_ascii_case(arg.trim()))
            .map(|w| (*w).to_string()),
        Kind::Num(unit) => parse_num(arg).map(|(v, _)| e(v, unit)),
        Kind::Raw => Some(arg.trim().to_string()),
        Kind::Pairs => {
            let mut map: Vec<(String, String)> = cur
                .split(',')
                .collect::<Vec<_>>()
                .chunks(2)
                .map(|c| (c[0].to_string(), c[1].to_string()))
                .collect();
            for c in arg
                .split(',')
                .collect::<Vec<_>>()
                .chunks(2)
                .filter(|c| c.len() == 2)
            {
                if let Some(slot) = map.iter_mut().find(|m| m.0 == c[0].trim().to_uppercase()) {
                    slot.1 = c[1].trim().to_string();
                }
            }
            Some(
                map.iter()
                    .map(|m| format!("{},{}", m.0, m.1))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        }
    };
    st.vals.insert(key.to_string(), new.unwrap_or(cur));
}

/// The reply to one command line (empty for a command, which the scope does not answer).
fn answer(st: &mut State, line: &str, o: &Opts) -> Vec<u8> {
    let line = line.trim();
    let (name, arg) = line
        .split_once(' ')
        .map_or((line, ""), |(n, a)| (n, a.trim()));
    let query = name.ends_with('?');
    let key = name.trim_end_matches('?').to_string();
    let rest = key
        .split_once(':')
        .map_or(key.as_str(), |(_, r)| r)
        .to_string();
    let ch: u8 = key
        .strip_prefix('C')
        .and_then(|r| r.chars().next())
        .and_then(|c| c.to_digit(10))
        .map_or(1, |d| d as u8);
    let text = |s: String| format!("{s}\n").into_bytes();
    match (name, rest.as_str()) {
        ("*IDN?", _) => return text("*IDN SIGLENT,SDS1202X-E,SDS1FAKE000000,1.3.27".into()),
        ("*OPC?", _) => return text("*OPC 1".into()),
        ("*SAV", _) | ("*RCL", _) => {
            let n: u32 = arg.parse().unwrap_or(99);
            let slot = format!("slot{n}");
            if name == "*SAV" && (1..=20).contains(&n) {
                st.vals.insert(slot, serialise(st));
            } else if name == "*RCL" && n == 0 {
                st.vals = State::new().vals;
            } else if name == "*RCL" && n <= 20 {
                if let Some(s) = st.vals.get(&slot).cloned() {
                    restore(st, &s);
                }
            }
            return vec![];
        }
        ("PNSU?", _) => {
            let s = serialise(st);
            let mut r = b"PNSU ".to_vec();
            r.extend(&block("", s.as_bytes())[1..]);
            return r;
        }
        ("PNSU", _) => {
            if let Some(s) = arg.get(11..) {
                restore(st, s);
            }
            return vec![];
        }
        ("SARA?", _) => {
            let depth = match st.vals["MSIZ"].as_str() {
                "7K" | "14K" => 7e3,
                "70K" | "140K" => 7e4,
                "700K" | "1.4M" => 7e5,
                _ => 7e6,
            };
            let tdiv = parse_num(&st.vals["TDIV"]).unwrap().0;
            return text(format!(
                "SARA {}",
                e((depth / (14.0 * tdiv)).min(1e9), "Sa/s")
            ));
        }
        ("SCDP", _) => {
            // A small bitmap: the header the scope sends and 200 bytes of pixels.
            let mut bmp =
                b"BM\x1e\0\0\0\0\0\0\0\x1a\0\0\0\x0c\0\0\0\x01\0\x01\0\x01\0\x18\0".to_vec();
            bmp.extend((0..200u8).map(|i| i.wrapping_mul(7)));
            return bmp;
        }
        ("DEFINE", _) => {
            st.vals.insert("DEFINE".into(), arg.to_string());
            return vec![];
        }
        ("DEFINE?", _) => {
            return text(format!(
                "DEFINE {}",
                st.vals.get("DEFINE").map_or("EQN,'C1+C2'", |v| v)
            ))
        }
        ("FRAM", _) => {
            st.frame = arg.parse::<u32>().unwrap_or(0).min(100);
            return vec![];
        }
        ("FRAM?", _) => return text(format!("FRAM {}", st.frame)),
        ("FTIM?", _) => {
            return text(format!(
                "FTIM 00: 05: {:02}. {:06}",
                st.frame % 60,
                650_814 + st.frame * 1000
            ))
        }
        ("CYMT?", _) | ("CYMT_HW?", _) => {
            return text(if name == "CYMT?" {
                format!("CYMT {}", e(1e3, "Hz"))
            } else {
                "CYMT 1.00000kHz".into()
            });
        }
        ("PACU", _) => {
            if let Some((p, c)) = arg.split_once(',') {
                if st.custom.len() < 5 && p != "ALL" {
                    st.custom.push((c.trim().to_string(), p.trim().to_string()));
                }
            }
            return vec![];
        }
        ("MEACL", _) => {
            st.custom.clear();
            return vec![];
        }
        ("PASTAT", _) => {
            let v = arg.to_uppercase();
            if v == "ON" || v == "OFF" {
                st.vals.insert("PASTAT".into(), v);
            }
            return vec![];
        }
        ("PASTAT?", _) => {
            return text(format!(
                "PASTAT {}",
                st.vals.get("PASTAT").map_or("OFF", |v| v)
            ))
        }
        ("MEAD", _) => return vec![],
        ("PFOP", _) => {
            if st.vals["PFEN"] == "ON" {
                st.pf.0 = arg.eq_ignore_ascii_case("ON");
                st.vals.insert("PFOP".into(), arg.to_uppercase());
            }
            return vec![];
        }
        ("PFOP?", _) => return text(format!("PFOP {}", if st.pf.0 { "ON" } else { "OFF" })),
        ("PFCM", _) => {
            st.mask = st.vals["PFEN"] == "ON" && !st.pf.0;
            return vec![];
        }
        ("PACL", _) => {
            st.pf = (st.pf.0, 0, 0);
            return vec![];
        }
        ("PFDD?", _) => {
            if st.pf.0 && st.mask {
                st.pf.1 += 10;
                st.pf.2 += 1;
            }
            return text(format!(
                "PFDD FAIL,{},PASS,{},TOTAL,{}",
                st.pf.2,
                st.pf.1 - st.pf.2,
                st.pf.1
            ));
        }
        ("REFSA", _) => {
            st.ref_saved.push(st.vals["REFLA"].clone());
            st.vals.insert("REFDS".into(), "ON".into());
            return vec![];
        }
        ("REFCL", _) => {
            st.ref_saved.clear();
            st.vals.insert("REFDS".into(), "OFF".into());
            return vec![];
        }
        ("REFDS", _) if !st.ref_saved.contains(&st.vals["REFLA"]) => return vec![],
        _ => {}
    }
    // Per-channel and math traces.
    if rest == "WF" {
        let n = 1000;
        let data: Vec<u8> = if key.starts_with("MATH") {
            if st.vals.get("DEFINE").is_some_and(|d| d.contains("FFT")) {
                // Observed on firmware 1.3.27 (2026-10-28), not in the guide: 1500 points for the FFT trace.
                (0..1500)
                    .map(|i| if i == 750 { 100u8 } else { 156u8 })
                    .collect()
            } else {
                let eq = st
                    .vals
                    .get("DEFINE")
                    .cloned()
                    .unwrap_or_default()
                    .to_uppercase();
                let (vd, vp) = (
                    parse_num(&st.vals["MTVD"]).unwrap().0,
                    st.vals["MTVP"].parse::<f64>().unwrap_or(0.0) / 50.0
                        * parse_num(&st.vals["MTVD"]).unwrap().0,
                );
                (0..1400)
                    .map(|k| {
                        let x = f64::from(k) * 1000.0 / 1400.0;
                        let (a, b) = (volts(st, 1, x), volts(st, 2, x));
                        let v = if eq.contains('+') {
                            a + b
                        } else if eq.contains('-') {
                            a - b
                        } else if eq.contains('*') {
                            a * b
                        } else {
                            a
                        };
                        ((v + vp) / (vd / 25.0)).round() as i8 as u8
                    })
                    .collect()
            }
        } else {
            (0..n)
                .map(|k| {
                    let vdiv = parse_num(&st.vals[&format!("C{ch}:VDIV")]).unwrap().0;
                    (volts(st, ch, f64::from(k)) / (vdiv / 25.0)).round() as i8 as u8
                })
                .collect()
        };
        return block(&format!("{key} DAT2"), &data);
    }
    if rest == "CRST" {
        if query {
            let keys: Vec<&str> = arg.split(',').collect();
            let body = keys
                .iter()
                .map(|k| {
                    format!(
                        "{k},{}",
                        e(num(st, k), if k.starts_with('V') { "V" } else { "S" })
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            return text(format!("{key} {body}"));
        }
        for c in arg
            .split(',')
            .collect::<Vec<_>>()
            .chunks(2)
            .filter(|c| c.len() == 2)
        {
            if let Some((v, _)) = parse_num(c[1]) {
                st.nums.insert(c[0].trim().to_uppercase(), v);
            }
        }
        return vec![];
    }
    if rest == "CRVA" && query {
        let track = st.vals["CRMS"] == "TRACK";
        if st.vals["CRMS"] == "OFF" {
            return vec![];
        }
        return text(if arg == "VREL" {
            format!(
                "{key} VREL,{},{},{}",
                e(num(st, "VDIF") - num(st, "VREF"), "V"),
                e(num(st, "VREF"), "V"),
                e(num(st, "VDIF"), "V")
            )
        } else {
            let (a, b) = if track {
                ("HREF", "HDIF")
            } else {
                ("TREF", "TDIF")
            };
            let dt = num(st, b) - num(st, a);
            format!(
                "{key} HREL,{},{},{},{}",
                e(dt, "S"),
                e(if dt == 0.0 { 0.0 } else { 1.0 / dt.abs() }, "Hz"),
                e(num(st, a), "S"),
                e(num(st, b), "S")
            )
        });
    }
    if rest == "MEAD" && query {
        return text(format!(
            "{key} {},{}",
            arg,
            if arg == "PHA" {
                e(-89.46, "degree")
            } else {
                e(1e-6, "S")
            }
        ));
    }
    if rest == "PAVA" && query {
        if arg.starts_with("CUST") {
            let list: Vec<String> = (0..5)
                .map(|i| match st.custom.get(i) {
                    Some((c, p)) => format!(
                        "CUST{}:{c},{p},{}",
                        i + 1,
                        pava(st, c[1..].parse().unwrap_or(1), p).unwrap_or_default()
                    ),
                    None => format!("CUST{}:OFF", i + 1),
                })
                .collect();
            let n = arg.strip_prefix("CUST").unwrap();
            return text(if n == "ALL" {
                format!("PAVA {}", list.join(";"))
            } else {
                format!(
                    "PAVA {}",
                    list.get(n.parse::<usize>().unwrap_or(1) - 1)
                        .cloned()
                        .unwrap_or_default()
                )
            });
        }
        if let Some(n) = arg.strip_prefix("STAT") {
            let Some((c, p)) = st.custom.get(n.parse::<usize>().unwrap_or(1) - 1).cloned() else {
                return vec![];
            };
            if st.vals.get("PASTAT").is_none_or(|v| v != "ON") {
                return vec![];
            }
            let v = pava(st, c[1..].parse().unwrap_or(1), &p).unwrap_or_default();
            let split = if o.stat_one_line { " " } else { "\n" };
            return text(format!("PAVA STAT{n} {c}{split}{p}:cur,{v},mean,{v},min,{v},max,{v},std-dev,0.00E+00V,count,171"));
        }
        if arg == "ALL" {
            let items = [
                "MAX", "MIN", "PKPK", "TOP", "BASE", "AMPL", "MEAN", "CMEAN", "STDEV", "VSTD",
                "RMS", "CRMS", "OVSN", "FPRE", "OVSP", "RPRE", "LEVELX", "PER", "FREQ", "PWID",
                "NWID", "RISE", "FALL", "WID", "DUTY", "NDUTY", "DELAY", "TIMEL",
            ];
            return text(format!(
                "{key} {}",
                items
                    .iter()
                    .map(|i| format!("{i},{}", pava(st, ch, i).unwrap()))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        return pava(st, ch, arg).map_or(vec![], |v| text(format!("{key} {arg},{v}")));
    }
    // Plain settings.
    let known = spec(&key).is_some() || BUS_PREFIXES.iter().any(|p| key.starts_with(p));
    if !known {
        return vec![];
    }
    if query {
        let default = spec(&key).map_or("", |s| s.1);
        let v = st
            .vals
            .get(&key)
            .cloned()
            .unwrap_or_else(|| default.to_string());
        return text(format!("{key} {v}"));
    }
    set(st, &key, arg);
    vec![]
}

/// The settings as one line of text, which `PNSU?` returns as its setup block and `restore` reads back.
fn serialise(st: &State) -> String {
    let mut v: Vec<_> = st
        .vals
        .iter()
        .filter(|(k, _)| !k.starts_with("slot"))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    v.sort();
    format!("<SETUP>{}</SETUP>", v.join(";"))
}

fn restore(st: &mut State, s: &str) {
    for kv in s
        .trim_start_matches("<SETUP>")
        .trim_end_matches("</SETUP>")
        .split(';')
    {
        if let Some((k, v)) = kv.split_once('=') {
            st.vals.insert(k.to_string(), v.to_string());
        }
    }
}

/// Reads one command: a line, except that a `PNSU #9<n>` setup block is read by its length because its data may hold newlines.
fn read_command(s: &mut TcpStream) -> Option<String> {
    let mut b = Vec::new();
    let mut one = [0u8; 1];
    loop {
        if s.read_exact(&mut one).is_err() {
            return None;
        }
        if one[0] == b'\n' {
            break;
        }
        b.push(one[0]);
        if b.len() == 16 && b.starts_with(b"PNSU #9") {
            let n: usize = String::from_utf8_lossy(&b[7..16]).parse().ok()?;
            let mut data = vec![0u8; n];
            s.read_exact(&mut data).ok()?;
            b.extend(data);
            return Some(String::from_utf8_lossy(&b).to_string());
        }
    }
    Some(
        String::from_utf8_lossy(&b)
            .trim_end_matches('\r')
            .to_string(),
    )
}

impl FakeScope {
    pub fn start() -> FakeScope {
        FakeScope::start_with(Opts::default())
    }

    pub fn start_with(o: Opts) -> FakeScope {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let born = Instant::now();
        std::thread::spawn(move || {
            let mut st = State::new();
            // One client at a time, like the real unit.
            for conn in listener.incoming() {
                let Ok(mut sock) = conn else { break };
                while let Some(l) = read_command(&mut sock) {
                    if l.trim().is_empty() {
                        continue;
                    }
                    if born.elapsed() < o.stale_for {
                        break;
                    }
                    log.lock().unwrap().push(l.clone());
                    let r = answer(&mut st, &l, &o);
                    if !r.is_empty() && sock.write_all(&r).is_err() {
                        break;
                    }
                }
            }
        });
        FakeScope { port, seen }
    }

    /// The commands received that start with `prefix`.
    pub fn got(&self, prefix: &str) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with(prefix))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Link;

    #[test]
    fn it_answers_like_the_guide_for_identity_settings_and_units() {
        let f = FakeScope::start();
        let mut l = Link::tcp("127.0.0.1", f.port).unwrap();
        assert!(l
            .query("*IDN?")
            .unwrap()
            .starts_with("*IDN SIGLENT,SDS1202X-E,"));
        l.send("C1:VDIV 200mV").unwrap();
        assert_eq!(l.query("C1:VDIV?").unwrap(), "C1:VDIV 2.00E-01V");
        l.send("TDIV 2us").unwrap();
        assert_eq!(l.query("TDIV?").unwrap(), "TDIV 2.00E-06S");
        l.send("FFTC 58MHz").unwrap();
        assert_eq!(l.query("FFTC?").unwrap(), "FFTC 5.80E+07Hz");
        // A word the scope does not know leaves the setting as it was.
        l.send("ACQW BOGUS").unwrap();
        assert_eq!(l.query("ACQW?").unwrap(), "ACQW SAMPLING");
        // Key-value settings change only the keys sent and answer all of them.
        l.send("INTS GRID,75").unwrap();
        assert_eq!(l.query("INTS?").unwrap(), "INTS TRACE,50,GRID,75");
        l.send("BWL C1,ON").unwrap();
        assert_eq!(l.query("BWL?").unwrap(), "BWL C1,ON,C2,OFF");
        // A command with no query of its own stays silent: the next reply belongs to the next query.
        l.send("SET50").unwrap();
        assert_eq!(l.query("*OPC?").unwrap(), "*OPC 1");
        assert_eq!(f.got("INTS"), ["INTS GRID,75", "INTS?"]);
    }

    #[test]
    fn it_serves_waveform_blocks_that_decode_with_the_guides_formula() {
        let f = FakeScope::start();
        let mut l = Link::tcp("127.0.0.1", f.port).unwrap();
        let d = l.block("C1:WF? DAT2").unwrap();
        assert_eq!(d.len(), 1000);
        let peak = d.iter().map(|&b| i32::from(b as i8)).max().unwrap();
        assert_eq!(peak, 100, "100 codes at 0.5 V per division is 2 V");
        // The history frame number changes the amplitude, the way a recorded frame differs from the live one.
        l.send("FRAM 40").unwrap();
        let d = l.block("C1:WF? DAT2").unwrap();
        assert_eq!(d.iter().map(|&b| i32::from(b as i8)).max().unwrap(), 60);
        // Channel 2 is a quarter period later: it starts at its peak.
        assert_eq!(i32::from(l.block("C2:WF? DAT2").unwrap()[0] as i8), 60);
        // Math add of the two, scaled by MTVD and MTVP as the guide's formula undoes it.
        l.send("DEFINE EQN,'C1+C2'").unwrap();
        l.send("MTVD 1V").unwrap();
        assert_eq!(l.block("MATH:WF? DAT2").unwrap().len(), 1400);
    }

    #[test]
    fn it_keeps_measurements_setups_and_history_the_way_the_guide_describes() {
        let f = FakeScope::start();
        let mut l = Link::tcp("127.0.0.1", f.port).unwrap();
        assert_eq!(l.query("C1:PAVA? FREQ").unwrap(), "C1:PAVA FREQ,1.00E+03Hz");
        l.send("PACU PKPK,C1").unwrap();
        assert!(l
            .query("PAVA? CUSTALL")
            .unwrap()
            .starts_with("PAVA CUST1:C1,PKPK,"));
        // Setups: save to a slot, change, recall; slot 0 is the default.
        l.send("TDIV 5us").unwrap();
        l.send("*SAV 3").unwrap();
        l.send("TDIV 1ms").unwrap();
        l.send("*RCL 3").unwrap();
        assert_eq!(l.query("TDIV?").unwrap(), "TDIV 5.00E-06S");
        l.send("*RCL 0").unwrap();
        assert_eq!(l.query("TDIV?").unwrap(), "TDIV 1.00E-03S");
        // History frames are bounded and time-stamped.
        l.send("FRAM 500").unwrap();
        assert_eq!(l.query("FRAM?").unwrap(), "FRAM 100");
        assert!(l.query("FTIM?").unwrap().starts_with("FTIM 00: 05: "));
        // Pass/fail counts only with a mask, running.
        l.send("PFEN ON").unwrap();
        l.send("PFCM").unwrap();
        l.send("PFOP ON").unwrap();
        assert_eq!(l.query("PFDD?").unwrap(), "PFDD FAIL,1,PASS,9,TOTAL,10");
    }

    #[test]
    fn a_new_connection_is_reset_while_it_pretends_to_hold_a_dead_session() {
        let f = FakeScope::start_with(Opts {
            stale_for: Duration::from_millis(600),
            ..Opts::default()
        });
        let mut l = Link::tcp("127.0.0.1", f.port).unwrap();
        assert!(
            l.query("*IDN?").is_err(),
            "the first line of a connection made too early gets no answer"
        );
        std::thread::sleep(Duration::from_millis(700));
        let mut l = Link::tcp("127.0.0.1", f.port).unwrap();
        assert!(l.query("*IDN?").is_ok());
        assert!(
            f.got("*IDN").len() == 1,
            "the reset connection's line was not served"
        );
    }
}

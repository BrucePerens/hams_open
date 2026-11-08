// SPDX-License-Identifier: AGPL-3.0-or-later
//! Parsers for what the `glasgow` command line prints, and for the capture files it writes. Pure functions over text, tested against
//! recordings in `tests/glasgow_fixtures/` (the voltage table and the list output were recorded from a revC3 on 2026-11-08; the
//! applet log lines are the formats in the pinned source and are marked as such in the fixtures).

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// One row of `glasgow voltage`: Port, Vsupply, Vlimit, Isupply, Ialert, Vsense, Vsense(alert).
#[derive(Clone, Debug, PartialEq)]
pub struct PortRow {
    pub port: String,
    pub vsupply_v: f64,
    pub vlimit_v: f64,
    /// None on a revision without current measurement ("n/a"), or a short ("(SHORT)", see `short`).
    pub isupply_a: Option<f64>,
    pub short: bool,
    pub ialert_a: Option<f64>,
    pub vsense_v: f64,
    pub fault: bool,
}

fn num(field: &str, what: &str) -> Result<f64, String> {
    match field.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(format!("{what}: {field:?} is not a finite number")),
    }
}

/// The table printed by `glasgow voltage` (and, with fewer columns, `voltage-limit`: Port, Vsupply, Vlimit only). A row that does
/// not parse is an error, never a guess: a voltage this server cannot read is a voltage it must treat as unknown.
pub fn voltage_table(text: &str) -> Result<Vec<PortRow>, String> {
    // [@ANCHOR: glasgow_parse_voltage_table]
    let mut rows = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.first()
            .is_some_and(|p| matches!(*p, "A" | "B" | "C" | "D"))
        {
            if f.len() < 3 {
                return Err(format!("voltage table row {line:?} has too few columns"));
            }
            let (isupply, short) = match f.get(3).map(|s| s.trim()) {
                Some("(SHORT)") => (None, true),
                Some("n/a") | None | Some("") => (None, false),
                Some(v) => (Some(num(v, "Isupply")?), false),
            };
            rows.push(PortRow {
                port: f[0].to_string(),
                vsupply_v: num(f[1], "Vsupply")?,
                vlimit_v: num(f[2], "Vlimit")?,
                isupply_a: isupply,
                short,
                ialert_a: match f.get(4).map(|s| s.trim()) {
                    Some("n/a") | None | Some("") => None,
                    Some(v) => Some(num(v, "Ialert")?),
                },
                vsense_v: match f.get(5) {
                    Some(v) => num(v, "Vsense")?,
                    None => f64::NAN,
                },
                fault: line.contains("(FAULT)"),
            });
        }
    }
    if rows.is_empty() {
        return Err(format!(
            "no port rows in the voltage output: {:?}",
            text.chars().take(200).collect::<String>()
        ));
    }
    Ok(rows)
}

/// The serial numbers `glasgow list` prints (one per line on stdout; log lines start with `X: ` and are not serials).
pub fn device_list(stdout: &str) -> Vec<String> {
    // [@ANCHOR: glasgow_parse_device_list]
    stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| serial_ok(l))
        .map(String::from)
        .collect()
}

/// A Glasgow serial: revision letter and digit, a dash, and a UTC build stamp (`C3-20240904T183337Z`). Glasgow itself uses this
/// pattern for its `--serial` argument.
pub fn serial_ok(s: &str) -> bool {
    // [@ANCHOR: glasgow_serial_ok]
    let b = s.as_bytes();
    b.len() == 19
        && (b'A'..=b'D').contains(&b[0])
        && b[1].is_ascii_digit()
        && b[2] == b'-'
        && b[3..11].iter().all(u8::is_ascii_digit)
        && b[11] == b'T'
        && b[12..18].iter().all(u8::is_ascii_digit)
        && b[18] == b'Z'
}

/// The warnings and errors in glasgow's stderr (`W: ` and `E: ` lines), for the caller to see.
pub fn log_problems(stderr: &str) -> Vec<String> {
    // [@ANCHOR: glasgow_parse_log_problems]
    stderr
        .lines()
        .filter(|l| l.starts_with("W: ") || l.starts_with("E: "))
        .map(|l| l.to_string())
        .collect()
}

/// The hardware revision named by a serial (`C3` of `C3-2024...`), and the port voltage range it allows: revA-C 1.65 to 5.0 V
/// (the voltage-limit register 1.8 to 5.0), revD 0.9 to 5.5 V; from `glasgow voltage --help` of the pinned install.
pub fn revision_limits(serial: &str) -> Option<(String, f64, f64)> {
    // [@ANCHOR: glasgow_revision_limits]
    if !serial_ok(serial) {
        return None;
    }
    let rev = &serial[..2];
    let (lo, hi) = if rev.starts_with('D') {
        (0.9, 5.5)
    } else {
        (1.65, 5.0)
    };
    Some((rev.to_string(), lo, hi))
}

/// The Glasgow as the kernel lists it (sysfs `idVendor` 20b7, `idProduct` 9db1), with the link speed. `root` is `/sys/bus/usb/devices`.
pub fn usb_device(root: &Path) -> Option<Value> {
    // [@ANCHOR: glasgow_usb_device]
    let read = |d: &Path, f: &str| {
        std::fs::read_to_string(d.join(f))
            .ok()
            .map(|s| s.trim().to_string())
    };
    let mut names: Vec<_> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    names.sort();
    for d in names {
        if read(&d, "idVendor").as_deref() == Some("20b7")
            && read(&d, "idProduct").as_deref() == Some("9db1")
        {
            let speed = read(&d, "speed").and_then(|s| s.parse::<f64>().ok());
            return Some(json!({
                "sysfs": d.file_name().and_then(|n| n.to_str()),
                "usb_id": "20b7:9db1",
                "product": read(&d, "product"),
                "manufacturer": read(&d, "manufacturer"),
                "serial": read(&d, "serial"),
                "bus": read(&d, "busnum").and_then(|s| s.parse::<u32>().ok()),
                "device": read(&d, "devnum").and_then(|s| s.parse::<u32>().ok()),
                "link_speed_mbps": speed,
                "high_speed": speed.map(|s| s >= 480.0),
            }));
        }
    }
    None
}

/// Pins from a spec such as `A0`, `A0:3` or `A0:3,B1`: a list of (port, number). At most 16 pins; ports A and B only.
pub fn pins(spec: &str) -> Result<Vec<(char, u8)>, String> {
    // [@ANCHOR: glasgow_parse_pins]
    let mut out = Vec::new();
    for part in spec.split(',') {
        let b = part.trim().as_bytes();
        let ok = |i: usize| b.get(i).is_some_and(|c| (b'0'..=b'7').contains(c));
        if b.len() < 2 || !matches!(b[0], b'A' | b'B') || !ok(1) {
            return Err(format!("pin spec {part:?} must look like A0, A0:3 or B7"));
        }
        let port = b[0] as char;
        let first = b[1] - b'0';
        let last = match b.len() {
            2 => first,
            4 if b[2] == b':' && ok(3) => b[3] - b'0',
            _ => return Err(format!("pin spec {part:?} must look like A0, A0:3 or B7")),
        };
        if last < first {
            return Err(format!("pin spec {part:?}: the range runs backwards"));
        }
        for n in first..=last {
            if out.contains(&(port, n)) {
                return Err(format!("pin {port}{n} is listed twice in {spec:?}"));
            }
            out.push((port, n));
        }
    }
    if out.is_empty() || out.len() > 16 {
        return Err(format!("{spec:?} must name 1 to 16 pins"));
    }
    Ok(out)
}

/// The TAPs a JTAG scan reported: `TAP #0: IR[5] IDCODE=0x4ba00477` (a TAP with no IDCODE says `BYPASS`).
pub fn jtag_taps(stderr: &str) -> Vec<Value> {
    // [@ANCHOR: glasgow_parse_jtag_taps]
    let mut taps = Vec::new();
    for l in stderr.lines() {
        let Some(rest) = l.split("TAP #").nth(1) else {
            continue;
        };
        let index: Option<u32> = rest.split(':').next().and_then(|n| n.trim().parse().ok());
        let ir = rest
            .split("IR[")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .map(String::from);
        let idcode = rest
            .split("IDCODE=0x")
            .nth(1)
            .and_then(|s| u32::from_str_radix(s.split_whitespace().next().unwrap_or(""), 16).ok());
        taps.push(json!({"tap": index, "ir": ir, "idcode": idcode.map(|i| format!("0x{i:08x}")),
            "bypass": rest.contains("BYPASS"),
            "manufacturer_id": idcode.map(|i| (i >> 1) & 0x7ff), "part": idcode.map(|i| (i >> 12) & 0xffff), "version": idcode.map(|i| i >> 28)}));
    }
    taps
}

/// The single result line a helper script prints: `BENCH_GLASGOW_RESULT {json}`. No such line is an error (the script did not finish).
pub fn result_line(stdout: &str) -> Result<Value, String> {
    // [@ANCHOR: glasgow_parse_result_line]
    let line = stdout
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix("BENCH_GLASGOW_RESULT "))
        .ok_or("the helper script printed no result line (it did not finish)")?;
    serde_json::from_str(line)
        .map_err(|e| format!("the helper script's result line is not JSON: {e}"))
}

/// Bytes from hex text (`48 65`, `4865`, `0x48,0x65`); an odd digit count or a non-hex character is an error.
pub fn hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    // [@ANCHOR: glasgow_parse_hex_bytes]
    let clean: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect::<String>()
        .replace("0x", "");
    if !clean.len().is_multiple_of(2) || !clean.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "{s:?} is not hex bytes (an even number of hex digits)"
        ));
    }
    Ok((0..clean.len() / 2)
        .map(|i| u8::from_str_radix(&clean[2 * i..2 * i + 2], 16).unwrap_or(0))
        .collect())
}

/// A summary of a VCD file written by `glasgow run analyzer`: the duration, and for each pin its transitions, the fraction of time
/// high, the shortest and longest pulse, the first and last level, and (when it toggled at least 3 times) the frequency from the
/// mean rising-edge period. Never a judgement; the numbers are what was captured. `x` values (the overrun marker) are counted.
pub fn vcd_summary(text: &str) -> Result<Value, String> {
    // [@ANCHOR: glasgow_vcd_summary]
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut now: u64 = 0;
    let mut start: Option<u64> = None;
    let mut state: BTreeMap<String, (char, u64)> = BTreeMap::new();
    struct Acc {
        transitions: u64,
        high_ns: u64,
        min_ns: Option<u64>,
        max_ns: u64,
        rises: Vec<u64>,
        first: Option<char>,
        last: char,
        x: u64,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    let mut in_header = true;
    for line in text.lines() {
        let l = line.trim();
        if in_header {
            if l.starts_with("$var") {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() >= 5 {
                    names.insert(f[3].to_string(), f[4].to_string());
                }
            }
            if l.starts_with("$enddefinitions") {
                in_header = false;
            }
            continue;
        }
        if let Some(t) = l.strip_prefix('#') {
            now = t.parse().map_err(|_| format!("bad timestamp {l:?}"))?;
            start.get_or_insert(now);
            continue;
        }
        let Some(c) = l.chars().next() else { continue };
        if !matches!(c, '0' | '1' | 'x' | 'X' | 'z' | 'Z') || l.starts_with("$") {
            continue;
        }
        let id = l[1..].trim().to_string();
        let Some(name) = names.get(&id) else { continue };
        let c = c.to_ascii_lowercase();
        let a = acc.entry(name.clone()).or_insert(Acc {
            transitions: 0,
            high_ns: 0,
            min_ns: None,
            max_ns: 0,
            rises: vec![],
            first: None,
            last: c,
            x: 0,
        });
        if c == 'x' {
            a.x += 1;
        }
        if let Some((prev, since)) = state.get(name).copied() {
            let width = now.saturating_sub(since);
            if prev == '1' {
                a.high_ns += width;
            }
            if prev != c {
                a.transitions += 1;
                if prev == '0' || prev == '1' {
                    a.min_ns = Some(a.min_ns.map_or(width, |m| m.min(width)));
                    a.max_ns = a.max_ns.max(width);
                }
                if c == '1' {
                    a.rises.push(now);
                }
            }
        } else {
            a.first = Some(c);
        }
        a.last = c;
        state.insert(name.clone(), (c, now));
    }
    let start = start.ok_or("the VCD file has no timestamps (an empty capture)")?;
    let duration_ns = now.saturating_sub(start);
    let mut pins = serde_json::Map::new();
    for (name, a) in acc {
        let (last_c, since) = state.get(&name).copied().unwrap_or(('0', now));
        let high = a.high_ns
            + if last_c == '1' {
                now.saturating_sub(since)
            } else {
                0
            };
        let freq = if a.rises.len() >= 3 {
            let span = (a.rises[a.rises.len() - 1] - a.rises[0]) as f64;
            (span > 0.0).then(|| 1e9 * (a.rises.len() - 1) as f64 / span)
        } else {
            None
        };
        pins.insert(
            name,
            json!({"transitions": a.transitions, "high_fraction": if duration_ns > 0 { Some(high as f64 / duration_ns as f64) } else { None },
                "min_pulse_ns": a.min_ns, "max_pulse_ns": if a.min_ns.is_some() { Some(a.max_ns) } else { None },
                "first_level": a.first.map(|c| c.to_string()), "last_level": a.last.to_string(), "rising_edge_frequency_hz": freq, "unknown_samples": a.x}),
        );
    }
    Ok(json!({"duration_ns": duration_ns, "pins": pins}))
}

/// A summary of the CSV `glasgow run uart-analyzer` writes: lines `rx,<hex>` / `tx,<hex>` and `rx,#F` (frame error) or `#P` (parity
/// error). Per channel: the byte count, the bytes as hex (at most `keep` of them) and as text with escapes, and the error counts.
pub fn uart_csv_summary(text: &str, keep: usize) -> Result<Value, String> {
    // [@ANCHOR: glasgow_uart_csv_summary]
    let mut ch: BTreeMap<String, (Vec<u8>, u64, u64)> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (c, rest) = line
            .split_once(',')
            .ok_or_else(|| format!("bad uart-analyzer line {line:?}"))?;
        if c != "rx" && c != "tx" {
            return Err(format!("bad uart-analyzer channel in {line:?}"));
        }
        let e = ch.entry(c.to_string()).or_default();
        match rest.trim() {
            "#F" => e.1 += 1,
            "#P" => e.2 += 1,
            hex => e.0.extend(hex_bytes(hex)?),
        }
    }
    let mut out = serde_json::Map::new();
    for (c, (bytes, frame, parity)) in ch {
        let kept = &bytes[..bytes.len().min(keep)];
        out.insert(c, json!({"bytes": bytes.len(), "truncated": bytes.len() > keep, "hex": kept.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "text": kept.iter().map(|&b| match b { 0x20..=0x7e => (b as char).to_string(), b'\n' => "\\n".into(), b'\r' => "\\r".into(), b'\t' => "\\t".into(), _ => format!("\\x{b:02x}") }).collect::<String>(),
            "frame_errors": frame, "parity_errors": parity}));
    }
    Ok(Value::Object(out))
}

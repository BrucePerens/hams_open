// SPDX-License-Identifier: AGPL-3.0-or-later
//! NanoVNA-H (and the NanoVNA family with the same ChibiOS shell) as a second driver behind the `bench_vna_*` tools. The instrument is
//! a text shell on a USB CDC serial port: the host sends `command\r`, the shell echoes it and ends every reply with the prompt `ch> `.
//! Facts used (public NanoVNA documentation): `version`, `info`, `help` (lists the commands), `sweep [start stop [points]]`,
//! `frequencies`, `data 0` (S11) and `data 1` (S21) as `re im` lines, `scan start stop points mask` on newer firmware, `pause`/`resume`.
//! A shell command the firmware does not have is answered `name ?`. Nothing here writes the unit's calibration slots, flash or
//! settings: the sweep range it changes (RAM only) is read first and put back afterwards.

use crate::link::{Port, Serial};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use vnwa_open::audio::Complex;
use vnwa_open::sweep::Point;

/// The end of every shell reply.
pub const PROMPT: &[u8] = b"ch> ";

/// The failures of this driver, each with its own wording prefix so a caller can tell them apart.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The instrument cannot do this, or this driver does not do it.
    NotSupported(String),
    /// The serial port is missing or went away.
    NotPresent(String),
    /// No prompt within the time allowed.
    Timeout(String),
    /// The reply is not what the shell protocol says.
    Protocol(String),
    /// The shell answered `name ?`: the firmware lacks the command.
    UnknownCommand(String),
    /// A bad argument of a tool call.
    Args(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // [@ANCHOR: bench_nanovna_error_display]
        match self {
            Error::NotSupported(s) => write!(f, "not supported by this instrument (nanovna): {s}"),
            Error::NotPresent(s) => write!(f, "device not present: {s}"),
            Error::Timeout(s) => write!(f, "nanovna timeout: {s}"),
            Error::Protocol(s) => write!(f, "nanovna protocol error: {s}"),
            Error::UnknownCommand(s) => write!(f, "nanovna firmware has no command {s:?}"),
            Error::Args(s) => write!(f, "{s}"),
            Error::Io(s) => write!(f, "nanovna i/o error: {s}"),
        }
    }
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

/// The `nanovna` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The serial device (use the /dev/serial/by-id name).
    pub serial: Option<String>,
    /// Overrides of the limits otherwise taken from the board name in `info`.
    pub min_hz: Option<f64>,
    pub max_hz: Option<f64>,
    pub max_points: Option<usize>,
    /// Wait per point after a sweep is set, before the data is read (the unit sweeps continuously).
    pub settle_ms: f64,
    /// Use `scan` when `help` lists it (default); false always uses `sweep` and `data`.
    pub use_scan: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            serial: None,
            min_hz: None,
            max_hz: None,
            max_points: None,
            settle_ms: 12.0,
            use_scan: true,
        }
    }
}

/// Reads `serial`, `min_hz`, `max_hz`, `max_points`, `settle_ms`, `use_scan` from an instrument entry.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: bench_nanovna_parse_config]
    let mut v = Config {
        serial: c.get("serial").and_then(|x| x.as_str()).map(String::from),
        ..Config::default()
    };
    if v.serial.is_none() {
        return Err("a nanovna needs \"serial\" (the /dev/serial/by-id path)".into());
    }
    let num = |k: &str| -> Result<Option<f64>, String> {
        match c.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(x) => x
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.0)
                .map(Some)
                .ok_or(format!("{k} must be a positive number")),
        }
    };
    v.min_hz = num("min_hz")?;
    v.max_hz = num("max_hz")?;
    if let Some(p) = num("max_points")? {
        if p.fract() != 0.0 || p > 100_000.0 {
            return Err("max_points must be a whole number".into());
        }
        v.max_points = Some(p as usize);
    }
    if let Some(s) = c.get("settle_ms") {
        v.settle_ms = s
            .as_f64()
            .filter(|n| (0.0..=1000.0).contains(n))
            .ok_or("settle_ms must be 0 to 1000")?;
    }
    if let Some(s) = c.get("use_scan") {
        v.use_scan = s.as_bool().ok_or("use_scan must be true or false")?;
    }
    Ok(v)
}

/// The text lines of one reply: the echo of the command and the prompt are removed. Non-text bytes are garbage; `name ?` is an unknown command.
pub fn parse_reply(cmd: &str, raw: &[u8]) -> Result<Vec<String>, Error> {
    // [@ANCHOR: bench_nanovna_parse_reply]
    let body = raw.strip_suffix(PROMPT).unwrap_or(raw);
    if let Some(b) = body
        .iter()
        .find(|b| !(matches!(**b, b'\r' | b'\n' | b'\t') || (0x20..0x7f).contains(*b)))
    {
        return Err(Error::Protocol(format!(
            "non-text byte 0x{b:02x} in the reply to {cmd:?} (garbage on the line, or a binary reply)"
        )));
    }
    let text = String::from_utf8_lossy(body);
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    if lines.first().is_some_and(|l| l.trim() == cmd.trim()) {
        lines.remove(0);
    }
    let word = cmd.split_whitespace().next().unwrap_or("");
    if lines
        .first()
        .is_some_and(|l| l.trim() == format!("{word} ?"))
    {
        return Err(Error::UnknownCommand(word.to_string()));
    }
    Ok(lines)
}

/// A conversation over a byte port with the shell.
pub struct Shell<P: Port> {
    port: P,
    /// Default time for one reply.
    pub timeout: Duration,
}

impl<P: Port> Shell<P> {
    pub fn new(port: P) -> Shell<P> {
        Shell {
            port,
            timeout: Duration::from_secs(3),
        }
    }

    /// Sends `cmd` and returns the reply lines, waiting up to `timeout` for the prompt. Replies may arrive in any number of pieces.
    pub fn command_within(&mut self, cmd: &str, timeout: Duration) -> Result<Vec<String>, Error> {
        // [@ANCHOR: bench_nanovna_command]
        // Whatever is left from an earlier timeout or garbage must not be taken for this reply.
        self.port.set_timeout(Duration::from_millis(1));
        let mut scratch = [0u8; 4096];
        loop {
            match self.port.read(&mut scratch) {
                Ok(0) => return Err(Error::NotPresent("the serial port was closed".into())),
                Ok(_) => {}
                Err(e) if is_wait(&e) => break,
                Err(e) => return Err(Error::NotPresent(e.to_string())),
            }
        }
        let mut data = format!("{cmd}\r").into_bytes();
        let give_up = Instant::now() + Duration::from_secs(3);
        while !data.is_empty() {
            match self.port.write(&data) {
                Ok(0) => return Err(Error::NotPresent("the serial port accepts no data".into())),
                Ok(n) => {
                    data.drain(..n);
                }
                Err(e) if is_wait(&e) && Instant::now() < give_up => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) if is_wait(&e) => {
                    return Err(Error::Timeout(format!(
                        "the port did not take {cmd:?} (the unit's USB serial is not draining)"
                    )))
                }
                Err(e) => return Err(Error::NotPresent(e.to_string())),
            }
        }
        let deadline = Instant::now() + timeout;
        let mut got: Vec<u8> = Vec::new();
        while !got.ends_with(PROMPT) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                let seen: String = String::from_utf8_lossy(&got).chars().take(60).collect();
                return Err(Error::Timeout(format!(
                    "no prompt after {cmd:?} within {} ms (received {} bytes: {seen:?})",
                    timeout.as_millis(),
                    got.len()
                )));
            }
            self.port.set_timeout(left.min(Duration::from_millis(50)));
            match self.port.read(&mut scratch) {
                Ok(0) => return Err(Error::NotPresent("the serial port was closed".into())),
                Ok(n) => got.extend_from_slice(&scratch[..n]),
                Err(e) if is_wait(&e) => std::thread::sleep(Duration::from_millis(1)),
                Err(e) => return Err(Error::NotPresent(e.to_string())),
            }
        }
        parse_reply(cmd, &got)
    }

    /// `command_within` with the shell's default time.
    pub fn command(&mut self, cmd: &str) -> Result<Vec<String>, Error> {
        let t = self.timeout;
        self.command_within(cmd, t)
    }
}

fn is_wait(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::Interrupted
    )
}

/// What `version`, `info` and `help` said.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub version: String,
    pub board: Option<String>,
    pub info: Vec<String>,
    pub commands: Vec<String>,
}

/// The range this unit can sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    pub min_hz: f64,
    pub max_hz: f64,
    pub max_points: usize,
}

impl Identity {
    /// Whether `help` lists the command.
    pub fn has(&self, cmd: &str) -> bool {
        self.commands.iter().any(|c| c == cmd)
    }

    /// The limits from the board name (public documentation: the NanoVNA-H sweeps 50 kHz to 900 MHz with 101 points; the H4 has 401 points
    /// and a higher top frequency), unless the configuration overrides them.
    pub fn limits(&self, cfg: &Config) -> Limits {
        // [@ANCHOR: bench_nanovna_limits]
        let h4 = self.board.as_deref().is_some_and(|b| b.contains("H4"));
        Limits {
            min_hz: cfg.min_hz.unwrap_or(50e3),
            max_hz: cfg.max_hz.unwrap_or(if h4 { 1.5e9 } else { 900e6 }),
            max_points: cfg.max_points.unwrap_or(if h4 { 401 } else { 101 }),
        }
    }
}

/// Reads `version`, `info` and `help`. A firmware without `help` or `info` is still identified by its version.
pub fn identify<P: Port>(sh: &mut Shell<P>) -> Result<Identity, Error> {
    // [@ANCHOR: bench_nanovna_identify]
    let version = sh
        .command("version")?
        .into_iter()
        .find(|l| !l.trim().is_empty())
        .ok_or(Error::Protocol("version gave no text".into()))?
        .trim()
        .to_string();
    let optional = |sh: &mut Shell<P>, c: &str| match sh.command(c) {
        Ok(l) => Ok(l),
        Err(Error::UnknownCommand(_)) => Ok(Vec::new()),
        Err(e) => Err(e),
    };
    let info = optional(sh, "info")?;
    let board = info.iter().find_map(|l| {
        l.strip_prefix("Board:")
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
    });
    let help = optional(sh, "help")?;
    let commands = help
        .iter()
        .flat_map(|l| l.strip_prefix("Commands:").unwrap_or(l).split([' ', ',']))
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    Ok(Identity {
        version,
        board,
        info,
        commands,
    })
}

fn parse_num(s: &str, what: &str) -> Result<f64, Error> {
    s.parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
        .ok_or(Error::Protocol(format!("{what}: {s:?} is not a number")))
}

/// `start stop points` as `sweep` without arguments prints it.
pub fn parse_sweep_query(lines: &[String]) -> Result<(f64, f64, usize), Error> {
    // [@ANCHOR: bench_nanovna_parse_sweep_query]
    let t: Vec<&str> = lines
        .first()
        .map(|l| l.split_whitespace().collect())
        .unwrap_or_default();
    if t.len() != 3 {
        return Err(Error::Protocol(format!(
            "sweep query should be 'start stop points', got {lines:?}"
        )));
    }
    let n = parse_num(t[2], "sweep points")?;
    Ok((
        parse_num(t[0], "sweep start")?,
        parse_num(t[1], "sweep stop")?,
        n as usize,
    ))
}

/// The numbers of each line, `per_line` of them, as parsed floats.
pub fn parse_columns(
    lines: &[String],
    per_line: usize,
    what: &str,
) -> Result<Vec<Vec<f64>>, Error> {
    // [@ANCHOR: bench_nanovna_parse_columns]
    lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            if v.len() != per_line {
                return Err(Error::Protocol(format!(
                    "{what}: expected {per_line} numbers per line, got {l:?}"
                )));
            }
            v.iter().map(|s| parse_num(s, what)).collect()
        })
        .collect()
}

fn complexes(lines: &[String], what: &str) -> Result<Vec<Complex>, Error> {
    Ok(parse_columns(lines, 2, what)?
        .iter()
        .map(|v| Complex { re: v[0], im: v[1] })
        .collect())
}

fn point(freq_hz: f64, s11: Complex, s21: Complex) -> Point {
    Point {
        freq_hz,
        s11,
        s21,
        if_error_hz: 0.0,
        level_change: 0.0,
    }
}

/// The check of the sweep arguments against the unit's limits.
pub fn check_range(start: f64, stop: f64, points: usize, lim: &Limits) -> Result<(), Error> {
    // [@ANCHOR: bench_nanovna_check_range]
    for (n, f) in [("start_hz", start), ("stop_hz", stop)] {
        if !(lim.min_hz..=lim.max_hz).contains(&f) {
            return Err(Error::Args(format!(
                "{n} {f} is outside this unit's {} to {} Hz",
                lim.min_hz, lim.max_hz
            )));
        }
    }
    if stop <= start {
        return Err(Error::Args(
            "stop_hz must be above start_hz (the NanoVNA sweeps a band)".into(),
        ));
    }
    if !(2..=lim.max_points).contains(&points) {
        return Err(Error::Args(format!(
            "points {points} must be a whole number from 2 to {} (this unit's limit)",
            lim.max_points
        )));
    }
    Ok(())
}

/// Sweeps `start..stop` and returns S11 and S21 as the unit reports them (with whatever calibration the unit itself has active). With
/// `scan` in the firmware one command returns frequency, S11 and S21; otherwise the unit's sweep range is set, read back through
/// `frequencies` and `data 0/1` while paused, and put back afterwards. The unit's calibration slots are never written.
pub fn measure<P: Port>(
    sh: &mut Shell<P>,
    id: &Identity,
    cfg: &Config,
    start: f64,
    stop: f64,
    points: usize,
) -> Result<Vec<Point>, Error> {
    // [@ANCHOR: bench_nanovna_measure]
    check_range(start, stop, points, &id.limits(cfg))?;
    let (a, b, n) = (start.round() as u64, stop.round() as u64, points);
    if cfg.use_scan && id.has("scan") {
        let wait = Duration::from_secs(10) + Duration::from_millis(60 * n as u64);
        match sh.command_within(&format!("scan {a} {b} {n} 7"), wait) {
            Ok(lines) => return scan_points(&lines, n),
            // A shell that lists scan but answers badly falls back to the plain sweep.
            Err(Error::UnknownCommand(_)) | Err(Error::Protocol(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let before = parse_sweep_query(&sh.command("sweep")?)?;
    let result = sweep_and_read(sh, cfg, (a, b, n));
    // Put the range back whatever happened (best effort: the first error is the one reported).
    let restore = sh
        .command(&format!(
            "sweep {} {} {}",
            before.0.round() as u64,
            before.1.round() as u64,
            before.2
        ))
        .and_then(|_| sh.command("resume"));
    match (result, restore) {
        (Ok(p), Ok(_)) => Ok(p),
        (Ok(_), Err(e)) => Err(Error::Io(format!(
            "measured, but restoring the sweep range failed: {e}"
        ))),
        (Err(e), _) => Err(e),
    }
}

fn scan_points(lines: &[String], n: usize) -> Result<Vec<Point>, Error> {
    // [@ANCHOR: bench_nanovna_scan_points]
    let rows = parse_columns(lines, 5, "scan")?;
    if rows.len() != n {
        return Err(Error::Protocol(format!(
            "scan returned {} points, {n} asked",
            rows.len()
        )));
    }
    Ok(rows
        .iter()
        .map(|v| {
            point(
                v[0],
                Complex { re: v[1], im: v[2] },
                Complex { re: v[3], im: v[4] },
            )
        })
        .collect())
}

fn sweep_and_read<P: Port>(
    sh: &mut Shell<P>,
    cfg: &Config,
    (a, b, n): (u64, u64, usize),
) -> Result<Vec<Point>, Error> {
    // [@ANCHOR: bench_nanovna_sweep_and_read]
    sh.command(&format!("sweep {a} {b} {n}"))?;
    sh.command("resume")?;
    std::thread::sleep(Duration::from_millis(
        300 + (cfg.settle_ms * n as f64) as u64,
    ));
    sh.command("pause")?;
    let f = parse_columns(&sh.command("frequencies")?, 1, "frequencies")?;
    let s11 = complexes(&sh.command("data 0")?, "data 0")?;
    let s21 = complexes(&sh.command("data 1")?, "data 1")?;
    if f.len() != n || s11.len() != n || s21.len() != n {
        return Err(Error::Protocol(format!(
            "asked for {n} points, got {} frequencies, {} S11 and {} S21",
            f.len(),
            s11.len(),
            s21.len()
        )));
    }
    Ok((0..n).map(|i| point(f[i][0], s11[i], s21[i])).collect())
}

fn open_shell(cfg: &Config) -> Result<Shell<Serial>, Error> {
    // [@ANCHOR: bench_nanovna_open_shell]
    let path = cfg.serial.as_deref().unwrap_or_default();
    if !std::path::Path::new(path).exists() {
        return Err(Error::NotPresent(format!(
            "{path} does not exist (unplugged?)"
        )));
    }
    // USB CDC ignores the rate; 115200 is the conventional setting.
    Ok(Shell::new(Serial::open(path, 115200).map_err(Error::Io)?))
}

const NO_GENERATOR: &str = "the NanoVNA's shell has no controllable CW generator, modulation, frequency meter or spectrum analyser mode that this driver uses; use the VNWA for those";

/// Every tool of the `bench_vna_*` set for a `nanovna` instrument.
pub fn call(
    inst: &crate::config::Instrument,
    name: &str,
    args: &Value,
    out_dir: &std::path::Path,
) -> Result<Value, String> {
    // [@ANCHOR: bench_nanovna_call]
    let cfg = &inst.nanovna;
    match name {
        "bench_identify" | "bench_status" => {
            let mut sh = open_shell(cfg)?;
            let id = identify(&mut sh)?;
            let lim = id.limits(cfg);
            Ok(json!({
                "idn": format!("NanoVNA {} version {}", id.board.clone().unwrap_or_else(|| "(board unknown)".into()), id.version),
                "driver": "nanovna", "serial": cfg.serial, "version": id.version, "board": id.board, "info": id.info,
                "commands": id.commands, "scan": id.has("scan"),
                "min_hz": lim.min_hz, "max_hz": lim.max_hz, "max_points": lim.max_points,
                "hub": inst.vna.hub.as_ref().map(|h| format!("{} port {}", h.hub, h.port)),
                "calibrated": "unknown: S11/S21 are as the unit reports them, with its own calibration if one is active",
            }))
        }
        "bench_vna_sweep" => sweep_tool(cfg, args, out_dir),
        "bench_vna_calibrate_measure" => calibrate_measure(cfg, args, out_dir),
        "bench_vna_calibrate_build" => crate::vna_tools::calibrate_build(args, out_dir),
        "bench_vna_power_cycle" => power_cycle(inst),
        "bench_vna_generator_on"
        | "bench_vna_generator_off"
        | "bench_vna_generator_level"
        | "bench_vna_generator_am"
        | "bench_vna_generator_fm"
        | "bench_vna_meter"
        | "bench_vna_spectrum" => {
            Err(Error::NotSupported(format!("{name}: {NO_GENERATOR}")).into())
        }
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

/// The sweep arguments of a NanoVNA call: `start_hz`, optional `stop_hz` (default start), `points` (default 101 or the limit).
/// The VNWA's `if_hz`, `point_ms` and `rf_level` are refused as not supported.
pub fn sweep_args(args: &Value, id: &Identity, cfg: &Config) -> Result<(f64, f64, usize), Error> {
    // [@ANCHOR: bench_nanovna_sweep_args]
    for k in ["if_hz", "point_ms", "rf_level"] {
        if args.get(k).is_some_and(|v| !v.is_null()) {
            return Err(Error::NotSupported(format!(
                "{k} is a VNWA setting; the NanoVNA sets its own IF and level"
            )));
        }
    }
    let lim = id.limits(cfg);
    let num = |k: &str| match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or(Error::Args(format!("{k} must be a number"))),
    };
    let start = num("start_hz")?.ok_or(Error::Args("start_hz is required".into()))?;
    let stop = num("stop_hz")?.unwrap_or(start);
    let points = match num("points")? {
        None => lim.max_points.min(101),
        Some(p) if p.fract() == 0.0 && p >= 0.0 => p as usize,
        Some(p) => return Err(Error::Args(format!("points {p} must be a whole number"))),
    };
    check_range(start, stop, points, &lim)?;
    Ok((start, stop, points))
}

fn sweep_tool(cfg: &Config, args: &Value, out_dir: &std::path::Path) -> Result<Value, String> {
    // [@ANCHOR: bench_nanovna_sweep_tool]
    let mut sh = open_shell(cfg)?;
    let id = identify(&mut sh)?;
    let (start, stop, points) = sweep_args(args, &id, cfg)?;
    let pts = measure(&mut sh, &id, cfg, start, stop, points)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = out_dir.join(format!("vna_sweep_{stamp}.csv"));
    std::fs::write(&file, crate::vna::csv(&pts)).map_err(|e| format!("{}: {e}", file.display()))?;
    let setup = vnwa_open::cal::Setup {
        cable: args
            .get("cable")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .into(),
        ..Default::default()
    };
    let calibrated = match args.get("calibration").and_then(|c| c.as_str()) {
        None => None,
        Some(f) => {
            args.get("cable").and_then(|c| c.as_str()).ok_or("cable is required with calibration (the text given when the standards were measured)")?;
            let (_, corrected) = crate::vna_tools::apply_calibration_setup(f, &setup, &pts)?;
            let cf = out_dir.join(format!("vna_sweep_{stamp}_calibrated.csv"));
            std::fs::write(&cf, crate::vna::calibrated_csv(&pts, &corrected))
                .map_err(|e| format!("{}: {e}", cf.display()))?;
            Some(cf)
        }
    };
    let db = |c: Complex| 20.0 * c.abs().log10();
    let rows: Vec<Value> = if pts.len() <= 51 {
        pts.iter().map(|p| json!({"freq_hz": p.freq_hz, "s21_db": db(p.s21), "s21_deg": p.s21.phase_deg(), "s11_db": db(p.s11), "s11_deg": p.s11.phase_deg()})).collect()
    } else {
        Vec::new()
    };
    Ok(json!({
        "points": pts.len(), "start_hz": start, "stop_hz": stop, "driver": "nanovna",
        "calibrated": calibrated.is_some(), "calibrated_file": calibrated.map(|c| c.display().to_string()),
        "note": "S11 and S21 as the NanoVNA reports them (its own calibration applies if one is active on the unit); the unit's sweep range was put back after the read",
        "file": file.display().to_string(), "rows": rows,
    }))
}

fn calibrate_measure(cfg: &Config, args: &Value, out: &std::path::Path) -> Result<Value, String> {
    // [@ANCHOR: bench_nanovna_calibrate_measure]
    let (session, standard, cable) = crate::vna_tools::measure_names(args)?;
    let mut sh = open_shell(cfg)?;
    let id = identify(&mut sh)?;
    let (start, stop, points) = sweep_args(args, &id, cfg)?;
    let pts = measure(&mut sh, &id, cfg, start, stop, points)?;
    let setup = vnwa_open::cal::Setup {
        cable: cable.clone(),
        ..Default::default()
    };
    crate::vna_tools::store_standard(&setup, &pts, out, &session, &standard)
}

fn power_cycle(inst: &crate::config::Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_nanovna_power_cycle]
    let hub = inst.vna.hub.clone().ok_or(Error::NotSupported(
        "no hub configured for this NanoVNA: set \"hub\" and \"hub_port\" in its entry of the bench config".into(),
    ))?;
    let path = inst.nanovna.serial.clone().unwrap_or_default();
    let port = crate::usb_hub::power::PortRef {
        hub: hub.hub,
        port: u32::from(hub.port),
        exact: false,
    };
    let schedule = [crate::usb_hub::power::Opts::new(
        Duration::from_secs(3),
        Duration::from_secs(25),
    )];
    let mut ready = || std::path::Path::new(&path).exists();
    let report = crate::usb_hub::power::cycle_escalating(
        &crate::usb_hub::uhubctl::ProcessRunner,
        std::path::Path::new("/sys"),
        &port,
        &schedule,
        Some(&mut ready),
    )?;
    Ok(
        json!({"power_cycled": true, "hub_power": report.json(), "note": "the unit restarted: its sweep range and any unsaved settings are back to power-on values"}),
    )
}

/// An in-memory NanoVNA shell and a pty that serves it, for tests.
#[cfg(test)]
pub mod testkit {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::sync::{Arc, Mutex};

    /// How the fake behaves.
    #[derive(Clone)]
    pub struct Fake {
        pub scan: bool,
        pub echo: bool,
        pub board: &'static str,
        /// Bytes returned per read (0: all that is queued).
        pub chunk: usize,
        pub garbage: bool,
        pub silent: bool,
        pub sweep: (u64, u64, usize),
        pub log: Arc<Mutex<Vec<String>>>,
        pub line: String,
        pub out: Vec<u8>,
    }

    impl Fake {
        pub fn new() -> Fake {
            Fake {
                scan: true,
                echo: true,
                board: "NanoVNA-H",
                chunk: 0,
                garbage: false,
                silent: false,
                sweep: (50_000, 900_000_000, 101),
                log: Arc::new(Mutex::new(Vec::new())),
                line: String::new(),
                out: Vec::new(),
            }
        }

        /// The S11 and S21 the fake reports at a frequency.
        pub fn s(f: f64) -> (Complex, Complex) {
            (
                Complex {
                    re: 0.1 + f / 2e9,
                    im: -0.2,
                },
                Complex {
                    re: 0.9,
                    im: f / 1e10,
                },
            )
        }

        fn freqs(&self, a: u64, b: u64, n: usize) -> Vec<f64> {
            (0..n)
                .map(|i| a as f64 + (b - a) as f64 * i as f64 / (n - 1).max(1) as f64)
                .map(f64::round)
                .collect()
        }

        fn reply(&mut self, line: &str) -> Vec<String> {
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| w.get(i).and_then(|s| s.parse::<u64>().ok());
            match w.first().copied() {
                Some("version") => vec!["0.2.3".into()],
                Some("info") => vec![
                    "Kernel: 4.0.0".into(),
                    format!("Board: {}", self.board),
                    "Version: 0.2.3".into(),
                ],
                Some("help") => {
                    let mut c =
                        "Commands: help exit info version pause resume sweep frequencies data"
                            .to_string();
                    if self.scan {
                        c += " scan";
                    }
                    vec![c]
                }
                Some("pause") | Some("resume") => vec![],
                Some("sweep") if w.len() == 1 => {
                    vec![format!(
                        "{} {} {}",
                        self.sweep.0, self.sweep.1, self.sweep.2
                    )]
                }
                Some("sweep") => {
                    if let (Some(a), Some(b), Some(n)) = (num(1), num(2), num(3)) {
                        self.sweep = (a, b, n as usize);
                    }
                    vec![]
                }
                Some("frequencies") => self
                    .freqs(self.sweep.0, self.sweep.1, self.sweep.2)
                    .iter()
                    .map(|f| format!("{f}"))
                    .collect(),
                Some("data") => {
                    let which = num(1).unwrap_or(0);
                    self.freqs(self.sweep.0, self.sweep.1, self.sweep.2)
                        .iter()
                        .map(|f| {
                            let (a, b) = Fake::s(*f);
                            let z = if which == 0 { a } else { b };
                            format!("{:.6} {:.6}", z.re, z.im)
                        })
                        .collect()
                }
                Some("scan") if self.scan => {
                    let (a, b, n) = (
                        num(1).unwrap_or(0),
                        num(2).unwrap_or(0),
                        num(3).unwrap_or(0) as usize,
                    );
                    self.freqs(a, b, n)
                        .iter()
                        .map(|f| {
                            let (x, y) = Fake::s(*f);
                            format!("{f} {:.6} {:.6} {:.6} {:.6}", x.re, x.im, y.re, y.im)
                        })
                        .collect()
                }
                Some(other) => vec![format!("{other} ?")],
                None => vec![],
            }
        }

        /// Bytes arriving from the host; replies are queued for reading.
        pub fn feed(&mut self, bytes: &[u8]) {
            for &b in bytes {
                if b == b'\r' || b == b'\n' {
                    let line = std::mem::take(&mut self.line);
                    if line.is_empty() && b == b'\n' {
                        continue;
                    }
                    self.log.lock().unwrap().push(line.clone());
                    if self.silent {
                        continue;
                    }
                    if self.echo {
                        self.out.extend_from_slice(line.as_bytes());
                    }
                    self.out.extend_from_slice(b"\r\n");
                    if self.garbage {
                        self.out.extend_from_slice(&[0xff, 0x00, 0x9c]);
                    }
                    for l in self.reply(&line) {
                        self.out.extend_from_slice(l.as_bytes());
                        self.out.extend_from_slice(b"\r\n");
                    }
                    self.out.extend_from_slice(PROMPT);
                } else {
                    self.line.push(b as char);
                }
            }
        }

        pub fn take(&mut self, max: usize) -> Vec<u8> {
            let n = if self.chunk == 0 {
                max
            } else {
                self.chunk.min(max)
            }
            .min(self.out.len());
            self.out.drain(..n).collect()
        }
    }

    /// The fake as an in-memory port.
    pub struct MemPort(pub Fake);

    impl Read for MemPort {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let v = self.0.take(buf.len());
            if v.is_empty() {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            buf[..v.len()].copy_from_slice(&v);
            Ok(v.len())
        }
    }

    impl Write for MemPort {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.feed(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Port for MemPort {
        fn set_timeout(&mut self, _d: Duration) {}
    }

    /// A pty whose master side is served by `fake` until dropped; `path` is the slave to give as the instrument's serial.
    pub struct PtyServer {
        pub path: String,
        stop: Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
        _slave: std::fs::File,
    }

    impl PtyServer {
        pub fn start(mut fake: Fake) -> PtyServer {
            let (mut m, mut s) = (0, 0);
            let mut name = [0 as libc::c_char; 128];
            // SAFETY: plain openpty with out-pointers we own.
            let r = unsafe {
                libc::openpty(
                    &mut m,
                    &mut s,
                    name.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            assert_eq!(r, 0);
            // SAFETY: openpty returned these descriptors and nothing else owns them.
            let (master, slave) =
                unsafe { (std::fs::File::from_raw_fd(m), std::fs::File::from_raw_fd(s)) };
            let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = stop.clone();
            let thread = std::thread::spawn(move || {
                let mut master = master;
                let mut buf = [0u8; 4096];
                while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    let mut p = libc::pollfd {
                        fd: std::os::fd::AsRawFd::as_raw_fd(&master),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // SAFETY: one valid pollfd.
                    if unsafe { libc::poll(&mut p, 1, 20) } > 0 {
                        match master.read(&mut buf) {
                            Ok(n) if n > 0 => {
                                fake.feed(&buf[..n]);
                                loop {
                                    let v = fake.take(64);
                                    if v.is_empty() {
                                        break;
                                    }
                                    if master.write_all(&v).is_err() {
                                        break;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            });
            PtyServer {
                path,
                stop,
                thread: Some(thread),
                _slave: slave,
            }
        }
    }

    impl Drop for PtyServer {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                t.join().ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    fn shell(f: Fake) -> Shell<MemPort> {
        let mut s = Shell::new(MemPort(f));
        s.timeout = Duration::from_millis(300);
        s
    }

    fn cfg() -> Config {
        Config {
            settle_ms: 0.0,
            ..Config::default()
        }
    }

    // Tests [@ANCHOR: bench_nanovna_parse_reply]
    // Tests [@ANCHOR: bench_nanovna_command]
    #[test]
    fn replies_lose_the_echo_and_prompt_and_survive_partial_reads() {
        for chunk in [0, 1, 3, 7] {
            let mut s = shell(Fake {
                chunk,
                ..Fake::new()
            });
            assert_eq!(
                s.command("version").unwrap(),
                vec!["0.2.3"],
                "chunk {chunk}"
            );
        }
        let mut s = shell(Fake {
            echo: false,
            ..Fake::new()
        });
        assert_eq!(s.command("version").unwrap(), vec!["0.2.3"]);
        assert_eq!(
            parse_reply("scan 1 2 3 7", b"scan 1 2 3 7\r\nscan ?\r\nch> "),
            Err(Error::UnknownCommand("scan".into()))
        );
    }

    // Tests [@ANCHOR: bench_nanovna_command]
    // Tests [@ANCHOR: bench_nanovna_parse_reply]
    // Tests [@ANCHOR: bench_nanovna_error_display]
    #[test]
    fn garbage_and_silence_are_typed_errors() {
        let mut s = shell(Fake {
            garbage: true,
            ..Fake::new()
        });
        assert!(matches!(s.command("version"), Err(Error::Protocol(_))));
        let mut s = shell(Fake {
            silent: true,
            ..Fake::new()
        });
        let e = s.command("version").unwrap_err();
        assert!(matches!(e, Error::Timeout(_)), "{e}");
        let e = s.command("version").unwrap_err().to_string();
        assert!(e.contains("no prompt"), "{e}");
    }

    // Tests [@ANCHOR: bench_nanovna_identify]
    // Tests [@ANCHOR: bench_nanovna_limits]
    #[test]
    fn identity_and_limits_come_from_the_shell() {
        let mut s = shell(Fake::new());
        let id = identify(&mut s).unwrap();
        assert_eq!(id.version, "0.2.3");
        assert_eq!(id.board.as_deref(), Some("NanoVNA-H"));
        assert!(id.has("scan") && id.has("data"));
        assert_eq!(id.limits(&cfg()).max_points, 101);
        let mut s = shell(Fake {
            board: "NanoVNA-H4",
            scan: false,
            ..Fake::new()
        });
        let id = identify(&mut s).unwrap();
        assert!(!id.has("scan"));
        assert_eq!(id.limits(&cfg()).max_points, 401);
        let over = Config {
            max_points: Some(51),
            ..cfg()
        };
        assert_eq!(id.limits(&over).max_points, 51);
    }

    // Tests [@ANCHOR: bench_nanovna_measure]
    // Tests [@ANCHOR: bench_nanovna_parse_columns]
    // Tests [@ANCHOR: bench_nanovna_parse_sweep_query]
    // Tests [@ANCHOR: bench_nanovna_scan_points]
    // Tests [@ANCHOR: bench_nanovna_sweep_and_read]
    #[test]
    fn a_sweep_by_scan_or_by_sweep_and_data_gives_the_same_points_and_restores_the_range() {
        for scan in [true, false] {
            let fake = Fake {
                scan,
                chunk: 5,
                ..Fake::new()
            };
            let log = fake.log.clone();
            let mut s = shell(fake);
            let id = identify(&mut s).unwrap();
            let p = measure(&mut s, &id, &cfg(), 1e6, 3e6, 5).unwrap();
            assert_eq!(p.len(), 5);
            assert_eq!(p[0].freq_hz, 1e6);
            assert_eq!(p[4].freq_hz, 3e6);
            let (a, b) = Fake::s(2e6);
            assert!((p[2].s11.re - a.re).abs() < 1e-6 && (p[2].s21.im - b.im).abs() < 1e-6);
            let l = log.lock().unwrap().clone();
            assert_eq!(l.iter().any(|c| c.starts_with("scan ")), scan, "{l:?}");
            if !scan {
                assert_eq!(l.last().unwrap(), "resume");
                assert!(
                    l.contains(&"sweep 50000 900000000 101".to_string()),
                    "{l:?}"
                );
            }
            assert!(!l.iter().any(|c| ["save", "cal", "reset", "saveconfig"]
                .iter()
                .any(|w| c.starts_with(w))));
        }
        assert!(parse_sweep_query(&["1 2".into()]).is_err());
        assert!(parse_columns(&["1 x".into()], 2, "t").is_err());
    }

    // Tests [@ANCHOR: bench_nanovna_check_range]
    // Tests [@ANCHOR: bench_nanovna_sweep_args]
    #[test]
    fn sweep_arguments_are_checked_against_the_units_limits() {
        let id = identify(&mut shell(Fake::new())).unwrap();
        let ok = sweep_args(
            &json!({"start_hz": 1e6, "stop_hz": 2e6, "points": 11}),
            &id,
            &cfg(),
        )
        .unwrap();
        assert_eq!(ok, (1e6, 2e6, 11));
        assert_eq!(
            sweep_args(&json!({"start_hz": 1e6, "stop_hz": 2e6}), &id, &cfg())
                .unwrap()
                .2,
            101
        );
        for bad in [
            json!({}),
            json!({"start_hz": 1e6}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "points": 102}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "points": 1}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "points": 2.5}),
            json!({"start_hz": 10.0, "stop_hz": 2e6}),
            json!({"start_hz": 1e6, "stop_hz": 1.2e9}),
            json!({"start_hz": 2e6, "stop_hz": 1e6}),
            json!({"start_hz": "1e6", "stop_hz": 2e6}),
        ] {
            assert!(sweep_args(&bad, &id, &cfg()).is_err(), "{bad}");
        }
        let e = sweep_args(
            &json!({"start_hz": 1e6, "stop_hz": 2e6, "if_hz": 4000}),
            &id,
            &cfg(),
        )
        .unwrap_err();
        assert!(
            e.to_string().contains("not supported by this instrument"),
            "{e}"
        );
    }

    // Tests [@ANCHOR: bench_nanovna_parse_config]
    #[test]
    fn the_config_needs_a_serial_port_and_checks_its_overrides() {
        assert!(parse_config(&json!({})).is_err());
        let c = parse_config(
            &json!({"serial": "/dev/x", "max_points": 401, "settle_ms": 5, "use_scan": false}),
        )
        .unwrap();
        assert_eq!(
            (c.max_points, c.settle_ms, c.use_scan),
            (Some(401), 5.0, false)
        );
        assert!(parse_config(&json!({"serial": "/dev/x", "max_points": 1.5})).is_err());
        assert!(parse_config(&json!({"serial": "/dev/x", "settle_ms": -1})).is_err());
        assert!(parse_config(&json!({"serial": "/dev/x", "use_scan": "y"})).is_err());
    }

    // Tests [@ANCHOR: bench_nanovna_call]
    // Tests [@ANCHOR: bench_nanovna_open_shell]
    // Tests [@ANCHOR: bench_nanovna_sweep_tool]
    // Tests [@ANCHOR: bench_nanovna_power_cycle]
    #[test]
    fn tools_work_over_a_pty_and_unsupported_ones_and_a_missing_unit_are_typed_errors() {
        let dir = std::env::temp_dir().join(format!("bench_nano_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pty = PtyServer::start(Fake::new());
        let text = |serial: &str| config_for(serial);
        let insts = text(&pty.path);
        let r = call(
            &insts[0],
            "bench_vna_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6, "points": 3}),
            &dir,
        )
        .unwrap();
        assert_eq!(r["points"], 3);
        assert!(
            std::fs::read_to_string(r["file"].as_str().unwrap())
                .unwrap()
                .lines()
                .count()
                == 4
        );
        let r = call(&insts[0], "bench_identify", &json!({}), &dir).unwrap();
        assert!(r["idn"]
            .as_str()
            .unwrap()
            .contains("NanoVNA-H version 0.2.3"));
        for t in [
            "bench_vna_generator_am",
            "bench_vna_generator_on",
            "bench_vna_meter",
            "bench_vna_spectrum",
        ] {
            let e = call(&insts[0], t, &json!({}), &dir).unwrap_err();
            assert!(e.starts_with("not supported by this instrument"), "{e}");
        }
        let e = call(&insts[0], "bench_vna_power_cycle", &json!({}), &dir).unwrap_err();
        assert!(e.contains("not supported") && e.contains("hub"), "{e}");
        let gone = config_for("/dev/serial/by-id/nanovna-absent");
        let e = call(
            &gone[0],
            "bench_vna_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6}),
            &dir,
        )
        .unwrap_err();
        assert!(e.starts_with("device not present"), "{e}");
        drop(pty);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn config_for(serial: &str) -> Vec<crate::config::Instrument> {
        crate::config::parse(&format!(
            r#"{{"instruments":{{"nano":{{"driver":"nanovna","tier":"read_config","serial":{serial:?},"settle_ms":0}}}}}}"#
        ))
        .unwrap()
    }

    // Tests [@ANCHOR: bench_nanovna_command]
    #[test]
    fn the_shell_works_against_a_pty_with_a_firmware_without_scan() {
        let pty = PtyServer::start(Fake {
            scan: false,
            ..Fake::new()
        });
        let mut sh = Shell::new(Serial::open(&pty.path, 115200).unwrap());
        let id = identify(&mut sh).unwrap();
        assert!(!id.has("scan"));
        let p = measure(&mut sh, &id, &cfg(), 1e6, 2e6, 4).unwrap();
        assert_eq!(p.len(), 4);
    }

    /// A port that replays a recorded transcript: reads give the next `rx` entry, writes must equal the next `tx` entry.
    struct Replay(Vec<(bool, Vec<u8>)>, usize);

    impl Replay {
        fn load(text: &str) -> Replay {
            Replay(
                text.lines()
                    .filter(|l| !l.starts_with('#') && !l.is_empty())
                    .map(|l| {
                        let f: Vec<&str> = l.split(' ').collect();
                        let bytes = (0..f[2].len() / 2)
                            .map(|i| u8::from_str_radix(&f[2][2 * i..2 * i + 2], 16).unwrap())
                            .collect();
                        (f[0] == "tx", bytes)
                    })
                    .collect(),
                0,
            )
        }
    }

    impl std::io::Read for Replay {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.0.get_mut(self.1) {
                Some((false, b)) => {
                    let n = b.len().min(buf.len());
                    buf[..n].copy_from_slice(&b[..n]);
                    b.drain(..n);
                    if b.is_empty() {
                        self.1 += 1;
                    }
                    Ok(n)
                }
                _ => Err(std::io::ErrorKind::TimedOut.into()),
            }
        }
    }

    impl std::io::Write for Replay {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let (is_tx, want) = self.0.get(self.1).cloned().expect("transcript ended");
            assert!(
                is_tx && want == buf,
                "unexpected write {:?}, wanted {:?}",
                String::from_utf8_lossy(buf),
                String::from_utf8_lossy(&want)
            );
            self.1 += 1;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Port for Replay {
        fn set_timeout(&mut self, _d: Duration) {}
    }

    // Tests [@ANCHOR: bench_nanovna_identify]
    // Tests [@ANCHOR: bench_nanovna_measure]
    #[test]
    fn the_recorded_transcript_replays_through_the_parser() {
        let mut sh = Shell::new(Replay::load(include_str!(
            "../tests/nanovna_fixtures/synthetic_transcript.txt"
        )));
        sh.timeout = Duration::from_millis(200);
        let id = identify(&mut sh).unwrap();
        assert_eq!(id.version, "0.2.3");
        let p = measure(&mut sh, &id, &cfg(), 1e6, 3e6, 3).unwrap();
        assert_eq!(
            p.iter().map(|x| x.freq_hz).collect::<Vec<_>>(),
            [1e6, 2e6, 3e6]
        );
        assert!((p[1].s11.re - 0.101).abs() < 1e-9 && (p[2].s21.im - 0.0003).abs() < 1e-9);
        assert_eq!(
            sh.port.1,
            sh.port.0.len(),
            "the whole transcript was consumed"
        );
    }

    // Tests [@ANCHOR: bench_nanovna_call]
    // Tests [@ANCHOR: bench_vna_tools_store_standard]
    // Tests [@ANCHOR: bench_vna_tools_apply_setup]
    // Tests [@ANCHOR: bench_vna_tools_measure_names]
    // Tests [@ANCHOR: bench_nanovna_calibrate_measure]
    #[test]
    fn the_shared_calibration_machinery_corrects_nanovna_sweeps() {
        let dir = std::env::temp_dir().join(format!("bench_nano_cal_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pty = PtyServer::start(Fake::new());
        let insts = config_for(&pty.path);
        let range =
            json!({"start_hz": 1e6, "stop_hz": 3e6, "points": 3, "session": "n1", "cable": "none"});
        // The fake reads the same S11 whatever is connected, so open, short and load are one number: the build must refuse
        // the degenerate set rather than divide by zero, and a thru-only set calibrates S21.
        let mut m = range.clone();
        m["standard"] = json!("thru");
        let r = call(&insts[0], "bench_vna_calibrate_measure", &m, &dir).unwrap();
        assert_eq!(r["points"], 3);
        let b = call(
            &insts[0],
            "bench_vna_calibrate_build",
            &json!({"session": "n1"}),
            &dir,
        )
        .unwrap();
        assert_eq!(b["s21_calibrated"], true);
        let cal = b["file"].as_str().unwrap().to_string();
        let mut sw = range.clone();
        sw["calibration"] = json!(cal);
        let r = call(&insts[0], "bench_vna_sweep", &sw, &dir).unwrap();
        assert_eq!(r["calibrated"], true);
        let text = std::fs::read_to_string(r["calibrated_file"].as_str().unwrap()).unwrap();
        // The thru was this same S21, so the corrected S21 is 1 at 0 degrees.
        assert!(
            text.lines()
                .nth(1)
                .unwrap()
                .contains(",1.000000,0.000000,0.000,0.000"),
            "{text}"
        );
        sw["cable"] = json!("other cable");
        assert!(call(&insts[0], "bench_vna_sweep", &sw, &dir)
            .unwrap_err()
            .contains("cable"));
        let mut bad = range.clone();
        bad["standard"] = json!("match");
        assert!(call(&insts[0], "bench_vna_calibrate_measure", &bad, &dir).is_err());
        drop(pty);
        std::fs::remove_dir_all(&dir).ok();
    }
}

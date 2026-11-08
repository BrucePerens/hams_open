// SPDX-License-Identifier: AGPL-3.0-or-later
//! ELAD receivers (FDM-DUO, FDM-S2, FDM-S1) driven directly over USB by the `elad_usb` crate. Receive only: nothing here can
//! transmit, write CAT, or write the receiver's EEPROM, and no tool needs a tier above `read_config`.
//!
//! The receiver opens, streams and closes inside one tool call (the device keeps no session between calls), so the tuning,
//! sample rate, filter and attenuator are arguments of the tools that stream.
//!
//! | Tool | Tier | What |
//! |---|---|---|
//! | `bench_elad_list` | read_only | ELAD receivers on the USB bus (ids and model; opens nothing) |
//! | `bench_elad_identify` | read_only | opens one: firmware, hardware version, serial, EEPROM calibration |
//! | `bench_elad_capture` | read_config | a short I/Q recording to a cf32 file with a summary |
//! | `bench_elad_spectrum` | read_config | a spectrum snapshot as a CSV with peaks |
//! | `bench_elad_level` | read_config | the level at a frequency |
//!
//! An FDM-S1/S2 streams nothing until its FPGA image has been loaded by ELAD's own loader, which this server neither includes
//! nor runs. The owner says it was loaded with `"fpga_loaded": true`; without it those units are identified but not streamed.
//!
//! The config entry: `{"driver": "elad", "tier": "read_config", "capture_dir": "/var/tmp/bench_elad", "fpga_loaded": false,
//!   "max_capture_s": 5}`.

#[cfg(test)]
pub mod testkit;

use crate::config::{Instrument, Kind, Tier};
use crate::dsp::{self, Iq};
use elad_usb::fpga::FpgaState;
use elad_usb::radio::{OpenOptions, Radio};
use elad_usb::transport::Transport;
use elad_usb::usb::Found;
use elad_usb::Error as ElError;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most samples one call keeps in memory (128 MB of I/Q).
const MAX_SAMPLES: usize = 16_000_000;
/// The read timeout of one bulk transfer.
const READ_TIMEOUT: Duration = Duration::from_millis(500);
/// The default sample rate: the lowest, the least USB traffic.
const DEFAULT_RATE_HZ: f64 = 192_000.0;

/// The `elad` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The owner's statement that an FDM-S1/S2's FPGA image was loaded by other means. Default false.
    pub fpga_loaded: bool,
    /// The directory that capture and spectrum files are written into; the tools that write files need it.
    pub capture_dir: Option<PathBuf>,
    /// The longest a streaming call may run, seconds.
    pub max_capture_s: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            fpga_loaded: false,
            capture_dir: None,
            max_capture_s: 5.0,
        }
    }
}

/// Reads `fpga_loaded`, `capture_dir` (an absolute path) and `max_capture_s` (0.1 to 30). A bad value is refused when the
/// config file is read.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: elad_config]
    let mut cfg = Config::default();
    if let Some(v) = c.get("fpga_loaded") {
        cfg.fpga_loaded = v.as_bool().ok_or("fpga_loaded must be true or false")?;
    }
    if let Some(v) = c.get("capture_dir") {
        let s = v.as_str().ok_or("capture_dir must be a string")?;
        if !Path::new(s).is_absolute() {
            return Err(format!("capture_dir {s:?} must be an absolute path"));
        }
        cfg.capture_dir = Some(PathBuf::from(s));
    }
    if let Some(v) = c.get("max_capture_s") {
        let x = v.as_f64().ok_or("max_capture_s must be a number")?;
        if !(x.is_finite() && (0.1..=30.0).contains(&x)) {
            return Err(format!("max_capture_s {x} must be 0.1 to 30 seconds"));
        }
        cfg.max_capture_s = x;
    }
    Ok(cfg)
}

pub struct Spec {
    pub name: &'static str,
    pub tier: Tier,
    pub desc: &'static str,
    pub props: &'static str,
    pub required: &'static [&'static str],
}

macro_rules! props {
    ($($rest:tt)*) => { concat!("{", $($rest)*, "}") };
}

const COMMON: &str = r#""instrument":{"type":"string"},"freq_hz":{"type":"number","description":"centre frequency in Hz, within the receiver's range (FDM-S2 10000 to 54000000)"},"rate_hz":{"type":"integer","description":"I/Q sample rate in Hz: 192000 (default), 384000, 768000, 1536000, 3072000, 6144000 (FDM-S2 only). An FDM-DUO's rate is set by the unit; give the one it is set to"},"filters":{"type":"boolean","description":"FDM-S2 band filters in (default true)"},"attenuator":{"type":"boolean","description":"FDM-S2 attenuator on (default false)"}"#;

/// Every tool of this instrument.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_elad_list",
        tier: Tier::ReadOnly,
        desc: "ELAD receivers: lists the ELAD receivers on the USB bus (bus, address, USB id, model, whether the model needs an FPGA image before it streams). Opens nothing and sends nothing.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_elad_identify",
        tier: Tier::ReadOnly,
        desc: "ELAD receiver: opens the first receiver and reads its firmware version, hardware version, serial number and EEPROM calibration (read-only control requests). Starts no stream and tunes nothing. Says whether the unit can stream under this config.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_elad_capture",
        tier: Tier::ReadConfig,
        desc: "ELAD receiver, RECEIVE ONLY: tunes to `freq_hz`, streams `seconds` of I/Q at `rate_hz` and writes it to a new file `name` (interleaved little-endian 32-bit float I,Q: cf32) in the configured capture_dir. Returns the sample count, the measured rate against the asked one, bulk timeouts, RMS, peak, DC offset and clipped samples. An FDM-S1/S2 whose FPGA is not declared loaded is refused with the reason.",
        props: props!(r#"{COMMON},"seconds":{"type":"number","description":"capture length, 0.01 to the config's max_capture_s (default 5), and at most 16 million samples"},"name":{"type":"string","description":"file name, 1 to 64 of letters, digits, dot, dash and underscore; must not exist"}"#),
        required: &["freq_hz", "seconds", "name"],
    },
    Spec {
        name: "bench_elad_spectrum",
        tier: Tier::ReadConfig,
        desc: "ELAD receiver, RECEIVE ONLY: a spectrum snapshot around `freq_hz` (Welch average, Hann window) written as a CSV (offset_hz, freq_hz, power_dbfs) in capture_dir, with the noise floor and the strongest peaks. `fft_len` is a power of two from 256 to 65536 (default 4096).",
        props: props!(r#"{COMMON},"fft_len":{"type":"integer","description":"FFT length, a power of two, 256 to 65536, default 4096"},"name":{"type":"string","description":"CSV file name, 1 to 64 of letters, digits, dot, dash and underscore; must not exist"}"#),
        required: &["freq_hz", "name"],
    },
    Spec {
        name: "bench_elad_level",
        tier: Tier::ReadConfig,
        desc: "ELAD receiver, RECEIVE ONLY: the signal level at `freq_hz`: tunes the receiver to it, averages the spectrum and returns the strongest bin and the band power within `bandwidth_hz` of the centre, in dBFS, with the EEPROM calibration offset. The dBm figure is an estimate and is marked unverified until the receiver has been checked against a known generator.",
        props: props!(r#"{COMMON},"bandwidth_hz":{"type":"number","description":"half-width of the band to sum, 100 to 20000 Hz, default 1000"}"#),
        required: &["freq_hz"],
    },
];

/// The USB side, so the tests run the whole path against a fake unit.
pub trait Bench {
    /// Lists the receivers on the bus.
    fn find(&self) -> Result<Vec<Found>, String>;
    /// Opens `found` with `opts`.
    fn open(&self, found: &Found, opts: OpenOptions) -> Result<Radio<Box<dyn Transport>>, String>;
}

/// The real bus.
pub struct Usb;

impl Bench for Usb {
    fn find(&self) -> Result<Vec<Found>, String> {
        elad_usb::usb::find().map_err(|e| next_step(&e.to_string()))
    }
    fn open(&self, found: &Found, opts: OpenOptions) -> Result<Radio<Box<dyn Transport>>, String> {
        let t = elad_usb::usb::UsbTransport::open(found).map_err(|e| {
            next_step(&format!(
                "could not open the receiver ({e}); on Linux the device node needs permission: install elad_usb/udev/70-elad-fdm.rules or grant an ACL on it"
            ))
        })?;
        Radio::open(Box::new(t) as Box<dyn Transport>, found.model, opts)
            .map_err(|e| next_step(&e.to_string()))
    }
}

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: bench_mcp_elad_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Elad).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no elad instrument named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no elad instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: an entry of `SPECS` appears only when an elad instrument is configured at or above its tier.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Elad && i.tier >= s.tier))
        .map(|s| {
            let props = s.props.replace("{COMMON}", COMMON);
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(&props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// The text to end an error with: the next step. Every failure ends with one.
fn next_step(what: &str) -> String {
    format!(
        "{what}. Next: check that the receiver is plugged in and shows in bench_elad_list; if it does not, check the USB cable and port (bench_usb_port_power on a switchable hub port, else replug by hand). No value from before this failure has been used."
    )
}

/// A number argument that must be present (or defaulted), finite and in `lo..=hi`.
fn number(
    args: &Value,
    key: &str,
    lo: f64,
    hi: f64,
    default: Option<f64>,
    why: &str,
) -> Result<f64, String> {
    // [@ANCHOR: elad_args]
    let v = match (args.get(key), default) {
        (None | Some(Value::Null), Some(d)) => d,
        (None | Some(Value::Null), None) => {
            return Err(format!("{key} is required and must be a number"))
        }
        (Some(v), _) => v
            .as_f64()
            .ok_or_else(|| format!("{key} must be a number"))?,
    };
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(format!("{key} {v} is outside {lo}..={hi} ({why})"))
    }
}

fn flag(args: &Value, key: &str, default: bool) -> Result<bool, String> {
    // [@ANCHOR: elad_flag]
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// A file name for a new file in capture_dir: letters, digits, dot, dash, underscore; no leading dot; refused if it exists.
fn new_file(cfg: &Config, args: &Value) -> Result<PathBuf, String> {
    // [@ANCHOR: elad_new_file]
    let dir = cfg
        .capture_dir
        .as_ref()
        .ok_or("this tool writes a file and the config has no capture_dir")?;
    let name = args
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or("name is required and must be a string")?;
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !ok {
        return Err(format!("name {name:?} must be 1 to 64 of letters, digits, dot, dash and underscore, not starting with a dot"));
    }
    let path = dir.join(name);
    if path.exists() {
        return Err(format!(
            "{} already exists; choose another name, nothing was overwritten",
            path.display()
        ));
    }
    Ok(path)
}

/// The rate argument checked against the first receiver's model (the model decides the list).
fn rate_of(args: &Value, found: &Found) -> Result<u32, String> {
    // [@ANCHOR: elad_rate_arg]
    let r = number(
        args,
        "rate_hz",
        1.0,
        1e9,
        Some(DEFAULT_RATE_HZ),
        "an I/Q sample rate",
    )? as u32;
    if found.model.sample_rates_hz().contains(&r) {
        Ok(r)
    } else {
        Err(format!(
            "rate_hz {r} is not one of the {} rates {:?}",
            found.model.name(),
            found.model.sample_rates_hz()
        ))
    }
}

/// One acquisition: the samples, what the radio said and how long it took.
struct Acquired {
    samples: Vec<Iq>,
    rate_hz: u32,
    elapsed_s: f64,
    stats: elad_usb::radio::Stats,
    offset_db: f64,
    model: &'static str,
}

/// Opens the first receiver, tunes it, streams until `target` samples or `deadline_s`, stops, and returns the samples.
/// Refuses an FDM-S1/S2 whose FPGA is not declared loaded before anything is sent.
fn acquire(
    bench: &dyn Bench,
    cfg: &Config,
    args: &Value,
    target: usize,
    deadline_s: f64,
) -> Result<Acquired, String> {
    // [@ANCHOR: elad_acquire]
    let found = bench
        .find()?
        .into_iter()
        .next()
        .ok_or_else(|| next_step("no ELAD receiver is on the USB bus"))?;
    let rate = rate_of(args, &found)?;
    let freq = number(
        args,
        "freq_hz",
        0.0,
        1e9,
        None,
        "the receiver's tuning range",
    )?;
    let filters = flag(args, "filters", true)?;
    let att = flag(args, "attenuator", false)?;
    let fpga = if cfg.fpga_loaded {
        FpgaState::LoadedExternally
    } else {
        FpgaState::Unknown
    };
    let mut radio = bench.open(
        &found,
        OpenOptions {
            fpga,
            sample_rate_hz: rate,
        },
    )?;
    radio.tune(freq).map_err(|e| next_step(&e.to_string()))?;
    if found.model.has_front_end_switches() {
        radio
            .set_filters(filters)
            .map_err(|e| next_step(&e.to_string()))?;
        radio
            .set_attenuator(att)
            .map_err(|e| next_step(&e.to_string()))?;
    } else if !filters || att {
        return Err(format!("the {} has no switchable filters or attenuator; leave `filters` and `attenuator` at their defaults", found.model.name()));
    }
    radio.start().map_err(|e| match e {
        ElError::FpgaImageRequired => format!("{e}. Nothing was sent to the receiver beyond identification and tuning. To stream, see the elad setup page and set \"fpga_loaded\": true only after the image really was loaded."),
        other => next_step(&other.to_string()),
    })?;
    let begin = Instant::now();
    let mut buf: Vec<f32> = Vec::new();
    let mut samples: Vec<Iq> = Vec::with_capacity(target.min(1 << 20));
    let mut failure: Option<String> = None;
    while samples.len() < target && begin.elapsed().as_secs_f64() < deadline_s {
        buf.clear();
        match radio.read(&mut buf, READ_TIMEOUT) {
            Ok(_) => {
                for p in buf.as_chunks::<2>().0 {
                    if samples.len() < target {
                        samples.push((p[0], p[1]));
                    }
                }
            }
            Err(ElError::Timeout) => {}
            Err(e) => {
                failure = Some(match e {
                    ElError::NoSamples => format!("{e}. The FPGA image is probably not loaded (or was lost at power-up or at a rate change); the receiver was stopped"),
                    other => other.to_string(),
                });
                break;
            }
        }
    }
    let elapsed_s = begin.elapsed().as_secs_f64();
    let _ = radio.stop();
    if let Some(f) = failure {
        return Err(next_step(&f));
    }
    if samples.is_empty() {
        return Err(next_step(
            "the receiver delivered no samples before the deadline",
        ));
    }
    Ok(Acquired {
        samples,
        rate_hz: rate,
        elapsed_s,
        stats: radio.stats(),
        offset_db: radio.level_offset_db(),
        model: found.model.name(),
    })
}

/// The target sample count for `seconds` at `rate`, capped by memory and by the config.
fn target_samples(
    cfg: &Config,
    args: &Value,
    rate: u32,
    default_s: f64,
) -> Result<(usize, f64), String> {
    // [@ANCHOR: elad_target_samples]
    let seconds = number(
        args,
        "seconds",
        0.01,
        cfg.max_capture_s,
        Some(default_s.min(cfg.max_capture_s)),
        "the config's max_capture_s",
    )?;
    let n = (seconds * f64::from(rate)) as usize;
    if n > MAX_SAMPLES {
        return Err(format!("{seconds} s at {rate} Hz is {n} samples; at most {MAX_SAMPLES} fit in one call (lower seconds or rate_hz)"));
    }
    Ok((n.max(1), seconds))
}

fn summary(a: &Acquired) -> Value {
    let (rms, peak, dc, clipped) = dsp::stats(&a.samples);
    let measured = a.samples.len() as f64 / a.elapsed_s.max(1e-9);
    json!({
        "model": a.model, "samples": a.samples.len(), "rate_hz": a.rate_hz,
        "measured_rate_hz": measured,
        "elapsed_s": a.elapsed_s,
        "bulk_timeouts": a.stats.timeouts, "bulk_empty_reads": a.stats.empty_reads, "bytes": a.stats.bytes,
        "rms": rms, "peak": peak, "dc_i": dc.0, "dc_q": dc.1, "clipped_samples": clipped,
        "calibration_offset_db": a.offset_db,
    })
}

pub fn list(bench: &dyn Bench, cfg: &Config) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_list]
    let found = bench.find()?;
    let rows: Vec<Value> = found
        .iter()
        .map(|f| {
            json!({
                "bus": f.bus, "address": f.address, "usb_id": format!("1721:{:04x}", f.pid), "model": f.model.name(),
                "needs_fpga_image": f.model.needs_fpga_image(),
                "streams_under_this_config": !f.model.needs_fpga_image() || cfg.fpga_loaded,
            })
        })
        .collect();
    Ok(json!({"receivers": rows, "count": rows.len()}))
}

pub fn identify(bench: &dyn Bench, cfg: &Config) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_identify]
    let found = bench
        .find()?
        .into_iter()
        .next()
        .ok_or_else(|| next_step("no ELAD receiver is on the USB bus"))?;
    let fpga = if cfg.fpga_loaded {
        FpgaState::LoadedExternally
    } else {
        FpgaState::Unknown
    };
    let radio = bench.open(
        &found,
        OpenOptions {
            fpga,
            sample_rate_hz: 192_000,
        },
    )?;
    let i = radio.info();
    let c = &i.calibration;
    Ok(json!({
        "model": found.model.name(), "usb_id": format!("1721:{:04x}", found.pid),
        "firmware": i.firmware.map(|f| format!("{}.{}", f[0], f[1])),
        "hardware": c.hardware.map(|(a, b)| format!("{a}.{b}")),
        "serial": c.serial,
        "rate_correction_hz": c.rate_correction_hz,
        "offset_global_db": c.global_db, "offset_lp_db": c.lp_db, "offset_att_db": c.att_db,
        "rx_range_hz": [found.model.rx_range_hz().0, found.model.rx_range_hz().1],
        "sample_rates_hz": found.model.sample_rates_hz(),
        "needs_fpga_image": found.model.needs_fpga_image(),
        "streams_under_this_config": !found.model.needs_fpga_image() || cfg.fpga_loaded,
    }))
}

pub fn capture(bench: &dyn Bench, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_capture]
    let path = new_file(cfg, args)?;
    let found = bench
        .find()?
        .into_iter()
        .next()
        .ok_or_else(|| next_step("no ELAD receiver is on the USB bus"))?;
    let (target, seconds) = target_samples(cfg, args, rate_of(args, &found)?, 1.0)?;
    let a = acquire(bench, cfg, args, target, seconds + 3.0)?;
    let mut f = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::with_capacity(a.samples.len() * 8);
    for (i, q) in &a.samples {
        bytes.extend_from_slice(&i.to_le_bytes());
        bytes.extend_from_slice(&q.to_le_bytes());
    }
    f.write_all(&bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = summary(&a);
    out["file"] = json!(path.display().to_string());
    out["format"] = json!("cf32 little-endian interleaved I,Q, full scale 1.0");
    out["file_bytes"] = json!(bytes.len());
    out["centre_hz"] = args.get("freq_hz").cloned().unwrap_or(Value::Null);
    Ok(out)
}

fn fft_len(args: &Value) -> Result<usize, String> {
    // [@ANCHOR: elad_fft_len]
    let n = number(
        args,
        "fft_len",
        256.0,
        65536.0,
        Some(4096.0),
        "a power of two",
    )? as usize;
    if n.is_power_of_two() {
        Ok(n)
    } else {
        Err(format!(
            "fft_len {n} must be a power of two from 256 to 65536"
        ))
    }
}

pub fn spectrum(bench: &dyn Bench, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_spectrum]
    let path = new_file(cfg, args)?;
    let n = fft_len(args)?;
    let found = bench
        .find()?
        .into_iter()
        .next()
        .ok_or_else(|| next_step("no ELAD receiver is on the USB bus"))?;
    let rate = rate_of(args, &found)?;
    let target = (n * 16).min(MAX_SAMPLES).max(n * 2);
    let a = acquire(
        bench,
        cfg,
        args,
        target,
        (target as f64 / f64::from(rate)).max(0.5) + 3.0,
    )?;
    let sp = dsp::welch(&a.samples, n, f64::from(rate))?;
    let centre = number(args, "freq_hz", 0.0, 1e9, None, "the tuning range")?;
    let mut csv = String::from("offset_hz,freq_hz,power_dbfs\n");
    for (i, p) in sp.power_db.iter().enumerate() {
        let off = sp.offset_hz(i);
        csv.push_str(&format!("{off:.3},{:.3},{p:.2}\n", centre + off));
    }
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .and_then(|mut f| f.write_all(csv.as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let peaks: Vec<Value> = dsp::peaks(&sp, 5, 40.0, 3, 3)
        .iter()
        .map(|p| json!({"offset_hz": p.offset_hz, "freq_hz": centre + p.offset_hz, "power_dbfs": p.power_db}))
        .collect();
    let mut out = summary(&a);
    out["file"] = json!(path.display().to_string());
    out["bins"] = json!(sp.power_db.len());
    out["bin_hz"] = json!(sp.bin_hz);
    out["noise_floor_dbfs"] = json!(dsp::noise_floor_db(&sp));
    out["peaks"] = json!(peaks);
    Ok(out)
}

pub fn level(bench: &dyn Bench, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_level]
    let bw = number(
        args,
        "bandwidth_hz",
        100.0,
        20_000.0,
        Some(1000.0),
        "the band to sum",
    )?;
    let found = bench
        .find()?
        .into_iter()
        .next()
        .ok_or_else(|| next_step("no ELAD receiver is on the USB bus"))?;
    let rate = rate_of(args, &found)?;
    let n = 4096usize;
    let target = n * 32;
    let a = acquire(
        bench,
        cfg,
        args,
        target,
        (target as f64 / f64::from(rate)).max(0.5) + 3.0,
    )?;
    let sp = dsp::welch(&a.samples, n, f64::from(rate))?;
    let (mut peak_db, mut sum) = (f64::NEG_INFINITY, 0.0f64);
    for (i, p) in sp.power_db.iter().enumerate() {
        if sp.offset_hz(i).abs() <= bw {
            peak_db = peak_db.max(*p);
            sum += 10f64.powf(p / 10.0);
        }
    }
    let band_db = 10.0 * sum.max(1e-30).log10();
    let mut out = summary(&a);
    out["band_half_width_hz"] = json!(bw);
    out["peak_bin_dbfs"] = json!(peak_db);
    out["band_power_dbfs"] = json!(band_db);
    out["level_dbm_estimate_unverified"] = json!(peak_db + a.offset_db);
    out["note"] = json!("dBFS is relative to a full-scale tone; the dBm figure adds the EEPROM calibration offset and is unverified until checked against a generator");
    Ok(out)
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    call_with(&Usb, insts, name, args)
}

/// `call` with the USB side given, so the tests run the whole path (tier check, argument checks, the driver) against a fake unit.
pub fn call_with(
    bench: &dyn Bench,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_call_with]
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    let cfg = &inst.elad;
    match name {
        "bench_elad_list" => list(bench, cfg),
        "bench_elad_identify" => identify(bench, cfg),
        "bench_elad_capture" => capture(bench, cfg, args),
        "bench_elad_spectrum" => spectrum(bench, cfg, args),
        "bench_elad_level" => level(bench, cfg, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Glasgow Interface Explorer (driver `glasgow`): the open-hardware FPGA multi-tool (1BitSquared, USB 20b7:9db1) driven through
//! the `glasgow` command line of a pinned, isolated Python environment (`glasgow/install_glasgow.py`; never the system Python).
//!
//! Safety model (the bench's rules applied to a device whose pins can drive a target):
//! * Nothing is driven until a person-approved `bench_glasgow_port_set` has set that port's I/O voltage. The voltage limit register is set
//!   to the same value first, so the hardware itself refuses more. Every applet tool takes the voltage from this server's record of what
//!   was set, never from its own arguments, and reads the device back first: a voltage that is not the recorded one is a fault, the
//!   ports are switched off, and the call fails.
//! * The ports have a clock: `max_on_s` after a port was set, a watchdog thread switches every port off (`glasgow safe`).
//! * After every tool that drove pins, the used ports are parked (all pins inputs, pulls released) and the answer says so.
//! * One session at a time: calls to one instrument are serialised by the server, and an `flock` on the install's lock file keeps a second
//!   server process or a stray `glasgow` of this user's out of the device while a call runs.
//! * Fail closed: a failed or unparsable readback, a changed voltage, a failed park, a failed `safe`: the ports are switched off, proved at
//!   0 V, and the error says what to do. A timeout or an applet failure parks the pins and reports. A link that is not at high speed refuses
//!   the bulk-transfer tools before running anything (the ports stay as set).
//! * No tool takes a command string, firmware, `flash`, `factory` or any applet outside the allow-list in `ops.rs`.
//!
//! `bench_glasgow_safe_state` is the safe-state companion; the server also runs it when it stops.

mod ops;
pub mod parse;
pub mod runner;
#[cfg(test)]
pub mod testkit;

use crate::config::{Instrument, Kind, Tier};
use parse::PortRow;
use rmcp::model::Tool;
use runner::{Out, Req, Runner};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The commit of Glasgow's software this server was written against and tested with.
pub const PINNED_COMMIT: &str = "70cb38c9b03471acd2eabb76c82308891bec2ca5";
/// A readback within this many volts (or 5 %) of the recorded setting counts as the same.
const VOLT_TOLERANCE_V: f64 = 0.15;

/// The `glasgow` settings of an instrument entry. `serial` (the device's serial, `C3-...`), `max_volts` (default 5.5, the highest of any
/// revision) and `max_on_s` are the ordinary instrument keys.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The install directory made by `install_glasgow.py` (holds `venv/` and `cache/`). Default: `~/.local/share/bench_mcp/glasgow-70cb38c9`.
    pub venv: Option<PathBuf>,
    /// The longest capture or listen of one call, seconds.
    pub max_capture_s: f64,
    /// The largest capture file, megabytes; the capture is stopped when it grows past this.
    pub max_capture_mb: u64,
    /// The longest an applet call may take, seconds, including a first-use gateware build.
    pub op_timeout_s: u64,
    /// The longest `bench_glasgow_prepare` may take.
    pub build_timeout_s: u64,
    /// The highest current limit a tool may ask the port supply to trip at, amperes (the hardware's range is 0 to 0.325).
    pub max_trip_a: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            venv: None,
            max_capture_s: 30.0,
            max_capture_mb: 64,
            op_timeout_s: 120,
            build_timeout_s: 600,
            max_trip_a: 0.1,
        }
    }
}

/// Reads the `glasgow` keys; a bad value is refused when the config file is read.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: glasgow_config]
    let mut cfg = Config::default();
    if let Some(v) = c.get("venv") {
        let p = PathBuf::from(v.as_str().ok_or("venv must be a path")?);
        if !p.is_absolute() {
            return Err(format!("venv {p:?} must be an absolute path"));
        }
        cfg.venv = Some(p);
    }
    let num = |key: &str, lo: f64, hi: f64| -> Result<Option<f64>, String> {
        match c.get(key) {
            None => Ok(None),
            Some(v) => {
                let x = v.as_f64().ok_or(format!("{key} must be a number"))?;
                if x.is_finite() && (lo..=hi).contains(&x) {
                    Ok(Some(x))
                } else {
                    Err(format!("{key} {x} must be {lo} to {hi}"))
                }
            }
        }
    };
    if let Some(x) = num("max_capture_s", 1.0, 120.0)? {
        cfg.max_capture_s = x;
    }
    if let Some(x) = num("max_capture_mb", 1.0, 512.0)? {
        cfg.max_capture_mb = x as u64;
    }
    if let Some(x) = num("op_timeout_s", 10.0, 300.0)? {
        cfg.op_timeout_s = x as u64;
    }
    if let Some(x) = num("build_timeout_s", 30.0, 1800.0)? {
        cfg.build_timeout_s = x as u64;
    }
    if let Some(x) = num("max_trip_a", 0.01, 0.325)? {
        cfg.max_trip_a = x;
    }
    if let Some(s) = c.get("serial") {
        let s = s.as_str().ok_or("serial must be a string")?;
        if !parse::serial_ok(s) {
            return Err(format!(
                "serial {s:?} is not a Glasgow serial such as C3-20240904T183337Z"
            ));
        }
    }
    if let Some(v) = c.get("max_volts").and_then(|v| v.as_f64()) {
        if v > 5.5 {
            return Err(format!(
                "max_volts {v} is above the 5.5 V the hardware allows"
            ));
        }
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

/// Every tool of this instrument; the applet tools are in `ops::SPECS`.
pub const SPECS: &[Spec] = &[
    Spec { name: "bench_glasgow_list", tier: Tier::ReadOnly,
        desc: "GLASGOW: the Glasgow devices on this host: serial, hardware revision, the USB link (speed in Mb/s: below 480 the bulk-transfer applets cannot work), the pinned software version and whether the install and its gateware cache are present. Changes nothing, drives nothing.",
        props: props!(r#""instrument":{"type":"string"}"#), required: &[] },
    Spec { name: "bench_glasgow_status", tier: Tier::ReadOnly,
        desc: "GLASGOW: the voltage of each port as the device reports it (supply setting, limit, measured, current, fault) and the state this server recorded: which ports a person set and to what, how long they have left, and whether any output can be enabled (`outputs_enabled`, `pins`). Reads only.",
        props: props!(r#""instrument":{"type":"string"}"#), required: &[] },
    Spec { name: "bench_glasgow_applets", tier: Tier::ReadOnly,
        desc: "GLASGOW: the applets this server can run (UART, I2C, SPI, JTAG, SWD, logic analyzer, GPIO, pattern generator), the tool for each, its tier and its pin roles with defaults. Static; no device access.",
        props: props!(r#""instrument":{"type":"string"}"#), required: &[] },
    Spec { name: "bench_glasgow_safe_state", tier: Tier::ReadConfig,
        desc: "GLASGOW: `glasgow safe`: every port's voltage regulator and every pin driver off, then reads every port back and proves it is 0 V. Forgets what was set. Safe at any time; use it first when anything went wrong. If it cannot confirm, the answer says what to do by hand (unplug the USB cable).",
        props: props!(r#""instrument":{"type":"string"}"#), required: &[] },
    Spec { name: "bench_glasgow_port_set", tier: Tier::Full,
        desc: "GLASGOW, DRIVES A SUPPLY RAIL: sets one port's I/O voltage `volts_v` (0 switches it off; otherwise 1.65 to 5.0 V on revisions A to C, 0.9 to 5.5 V on revision D, and never above the config's max_volts) with a supply current trip of `trip_current_a` (default 0.1 A, at most the config's max_trip_a). The hardware voltage limit is set to the same value first. Every pin tool uses exactly this voltage and refuses a port that was not set here. `confirm` must be true, from the person at the bench who has checked the target's I/O voltage. The port switches itself off after the config's max_on_s. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"port":{"type":"string","enum":["A","B"],"description":"A or B"},"volts_v":{"type":"number","description":"port I/O voltage in volts; 0 = off"},"trip_current_a":{"type":"number","description":"supply current trip, amperes, default 0.1"},"confirm":{"type":"boolean","description":"true: the person at the bench agrees this port may be powered at volts_v"},"dry_run":{"type":"boolean","description":"true: return the plan and send nothing"}"#),
        required: &["port", "volts_v"] },
    Spec { name: "bench_glasgow_prepare", tier: Tier::ReadConfig,
        desc: "GLASGOW: builds (and caches) the gateware for a tool's applet with the pins you will use, so the first real call does not spend 5 to 20 s building and cannot time out. Needs no device and drives nothing: it only runs the pinned yosys and nextpnr at low priority. `tool` names the applet tool (i2c, uart, uart_sniff, spi, logic, gpio, pattern, jtag, swd); pass the same pin arguments you will pass to it. Returns the build time and the cache size.",
        props: props!(r#""instrument":{"type":"string"},"tool":{"type":"string","enum":["i2c","uart","uart_sniff","spi","logic","gpio","pattern","jtag","swd"],"description":"the tool whose applet to build"},"revision":{"type":"string","description":"hardware revision such as C3; default: from the device serial"},"pins":{"type":"string","description":"gpio, pattern, logic: the pin spec such as A0:3"},"scl":{"type":"string","description":"pin, default A0"},"sda":{"type":"string","description":"pin, default A1"},"rx":{"type":"string","description":"pin, default A0"},"tx":{"type":"string","description":"pin, default A1"},"cs":{"type":"string","description":"pin, default A0"},"sck":{"type":"string","description":"pin, default A1"},"copi":{"type":"string","description":"pin, default A2"},"cipo":{"type":"string","description":"pin, default A3"},"tck":{"type":"string","description":"pin, default A0"},"tms":{"type":"string","description":"pin, default A1"},"tdi":{"type":"string","description":"pin, default A2"},"tdo":{"type":"string","description":"pin, default A3"},"swclk":{"type":"string","description":"pin, default A0"},"swdio":{"type":"string","description":"pin, default A1"},"parity":{"type":"string","enum":["none","even","odd","zero","one"],"description":"none, even, odd, zero or one; default none"},"stop_bits":{"type":"integer","description":"1 to 8, default 1"}"#),
        required: &["tool"] },
];

/// Everything a call touches outside the process, so the tests put a simulated device and a fake sysfs behind the production code.
pub struct Ctx {
    pub runner: Arc<dyn Runner>,
    /// `/sys/bus/usb/devices`.
    pub sysfs: PathBuf,
    /// Captures are written here.
    pub out_dir: PathBuf,
    /// The lock file of the install.
    pub lock_path: PathBuf,
}

/// The install directory: the config's `venv`, else `~/.local/share/bench_mcp/glasgow-70cb38c9`.
pub fn install_dir(cfg: &Config) -> PathBuf {
    // [@ANCHOR: glasgow_install_dir]
    cfg.venv.clone().unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/nonexistent".into()))
            .join(".local/share/bench_mcp/glasgow-70cb38c9")
    })
}

fn real_ctx(inst: &Instrument) -> Ctx {
    // [@ANCHOR: glasgow_real_ctx]
    let dir = install_dir(&inst.glasgow);
    let (program, cache_dir) = runner::install_paths(&dir);
    Ctx {
        runner: Arc::new(runner::RealRunner { program, cache_dir }),
        sysfs: PathBuf::from("/sys/bus/usb/devices"),
        out_dir: crate::out_dir(),
        lock_path: dir.join("glasgow.lock"),
    }
}

/// What this server knows about one port it powered.
#[derive(Clone, Debug)]
struct PortRec {
    volts: f64,
    deadline: Instant,
}

struct Session {
    ports: BTreeMap<char, PortRec>,
    runner: Arc<dyn Runner>,
    serial: Option<String>,
    timeout: Duration,
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    // [@ANCHOR: glasgow_sessions]
    static S: std::sync::OnceLock<Mutex<HashMap<String, Session>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_session<T>(f: impl FnOnce(&mut HashMap<String, Session>) -> T) -> T {
    let mut g = sessions().lock().unwrap_or_else(|p| p.into_inner());
    f(&mut g)
}

/// The `--serial` option for the instrument, when its entry names the device.
fn base_args(inst: &Instrument) -> Vec<String> {
    // [@ANCHOR: glasgow_base_args]
    match &inst.serial {
        Some(s) => vec!["--serial".into(), s.clone()],
        None => vec![],
    }
}

fn next_step(what: &str) -> String {
    format!("{what}. Next: bench_glasgow_list (is the device on the bus, at what speed, is the install present), then bench_glasgow_safe_state. No value from before this failure has been used.")
}

/// Holds the install's lock file for the length of a call: a second process cannot use the device meanwhile.
struct DeviceLock(#[allow(dead_code)] std::fs::File);

fn device_lock(path: &Path) -> Result<DeviceLock, String> {
    // [@ANCHOR: glasgow_device_lock]
    use std::os::fd::AsRawFd;
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // Waits up to two seconds: a lock another process is just letting go of (or a forked child that has not yet exec'd) is not a second session.
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        // SAFETY: flock(2) on a descriptor this function owns.
        let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            break;
        }
        if Instant::now() >= until {
            return Err(format!("another process holds {} (a second bench_mcp or a glasgow command of this user is using the Glasgow). One session at a time: wait for it or end it, then retry.", path.display()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(DeviceLock(f))
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}

/// Runs a command line and returns its output; a child that did not end, or ended by this runner's timeout, is an error.
fn run(ctx: &Ctx, req: Req, what: &str) -> Result<Out, String> {
    // [@ANCHOR: glasgow_run]
    let out = ctx.runner.run(&req)?;
    if out.stuck {
        return Err(out.stderr);
    }
    if out.timed_out {
        return Err(next_step(&format!(
            "{what} did not finish within {} s and was interrupted ({})",
            req.timeout.as_secs(),
            tail(&out.stderr, 3)
        )));
    }
    Ok(out)
}

fn req(args: Vec<String>, secs: u64) -> Req {
    Req {
        args,
        timeout: Duration::from_secs(secs),
        stop_after: None,
        watch: None,
        nice: false,
    }
}

/// `glasgow voltage`: every port's row. A non-zero exit or an unreadable table is an error with the permission hint.
fn read_ports(ctx: &Ctx, inst: &Instrument) -> Result<(Vec<PortRow>, Vec<String>), String> {
    // [@ANCHOR: glasgow_read_ports]
    let mut a = base_args(inst);
    a.push("voltage".into());
    let out = run(ctx, req(a, 30), "glasgow voltage")?;
    if out.code != Some(0) {
        let hint = if out.stderr.contains("missing permissions") {
            " The user cannot open the USB device: install daemons/bench_mcp/glasgow/70-glasgow-bench.rules (manual instruments/glasgow/setup)."
        } else {
            ""
        };
        return Err(next_step(&format!(
            "glasgow voltage failed ({}){hint}",
            tail(&out.stderr, 3)
        )));
    }
    let rows = parse::voltage_table(&out.stdout).map_err(|e| next_step(&e))?;
    Ok((rows, parse::log_problems(&out.stderr)))
}

/// `glasgow safe`, then the readback that proves every port is at 0 V. The session record is cleared whatever happens.
pub fn safe(ctx: &Ctx, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: glasgow_safe]
    let mut a = base_args(inst);
    a.push("safe".into());
    let ran = run(ctx, req(a, 30), "glasgow safe");
    with_session(|s| s.remove(&inst.name));
    let manual = "SAFE STATE NOT CONFIRMED: a port may still be powered or a pin driven. Do this now: unplug the Glasgow's USB cable (every output goes off), and do not use it again until the owner has looked at the target.";
    let out = ran.map_err(|e| format!("{manual} ({e})"))?;
    if out.code != Some(0) {
        return Err(format!(
            "{manual} (glasgow safe exited {:?}: {})",
            out.code,
            tail(&out.stderr, 3)
        ));
    }
    let (rows, _) =
        read_ports(ctx, inst).map_err(|e| format!("{manual} (readback failed: {e})"))?;
    if let Some(r) = rows.iter().find(|r| r.vsupply_v.abs() > 0.05) {
        return Err(format!(
            "{manual} (port {} still reads {:.3} V)",
            r.port, r.vsupply_v
        ));
    }
    Ok(
        json!({"outputs_enabled": false, "verified": true, "ports": rows.iter().map(|r| json!({"port": r.port, "vsupply_v": r.vsupply_v, "vsense_v": r.vsense_v})).collect::<Vec<_>>()}),
    )
}

fn row_json(r: &PortRow) -> Value {
    json!({"port": r.port, "supply_set_v": r.vsupply_v, "limit_v": r.vlimit_v, "measured_v": r.vsense_v,
        "current_a": r.isupply_a, "short": r.short, "current_trip_a": r.ialert_a, "fault": r.fault})
}

/// The tool `bench_glasgow_list`.
pub fn list(ctx: &Ctx, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: glasgow_list]
    let dir = install_dir(&inst.glasgow);
    let (program, cache) = runner::install_paths(&dir);
    let installed = program.exists();
    let usb = parse::usb_device(&ctx.sysfs);
    let mut devices = Value::Null;
    let mut problems = vec![];
    let mut version = Value::Null;
    if installed {
        let _lock = device_lock(&ctx.lock_path)?;
        let out = run(ctx, req(vec!["list".into()], 30), "glasgow list")?;
        problems = parse::log_problems(&out.stderr);
        let serials = parse::device_list(&out.stdout);
        devices = json!(serials.iter().map(|s| {
            let r = parse::revision_limits(s);
            json!({"serial": s, "revision": r.as_ref().map(|r| r.0.clone()), "port_voltage_range_v": r.as_ref().map(|r| [r.1, r.2])})
        }).collect::<Vec<_>>());
        if let Ok(v) = std::fs::read_to_string(dir.join("INSTALLED.json")) {
            version = serde_json::from_str(&v).unwrap_or(Value::Null);
        }
        if serials.len() > 1 && inst.serial.is_none() {
            return Err(format!("{} Glasgow devices are connected ({}); set \"serial\" in this instrument's config entry so one is chosen", serials.len(), serials.join(", ")));
        }
    }
    let slow = usb
        .as_ref()
        .and_then(|u| u.get("high_speed"))
        .and_then(|h| h.as_bool())
        == Some(false);
    Ok(
        json!({"installed": installed, "install_dir": dir.display().to_string(), "pinned_glasgow_commit": PINNED_COMMIT, "install": version,
        "gateware_cache_present": cache.join("GlasgowEmbedded/bitstreams").is_dir(),
        "usb": usb, "devices": devices, "log_problems": problems,
        "warning": if slow { Some("the device linked at full speed (12 Mb/s), not high speed (480 Mb/s): the Linux kernel cannot use its 512-byte bulk endpoints, so every applet that streams data fails. Try a short data-rated USB 2.0 cable and another port; see manual instruments/glasgow/troubleshooting") } else { None }}),
    )
}

/// The tool `bench_glasgow_status`.
pub fn status(ctx: &Ctx, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: glasgow_status]
    let _lock = device_lock(&ctx.lock_path)?;
    let (rows, problems) = read_ports(ctx, inst)?;
    let now = Instant::now();
    let rec = with_session(|s| {
        s.get(&inst.name).map(|x| x.ports.iter().map(|(p, r)| json!({"port": p.to_string(), "set_v": r.volts, "seconds_left": r.deadline.saturating_duration_since(now).as_secs_f64()})).collect::<Vec<_>>())
    });
    let on = rows.iter().any(|r| r.vsupply_v > 0.05);
    Ok(json!({"outputs_enabled": on,
        "pins": if on { "inputs unless a tool is running (every tool parks the pins it used when it ends)" } else { "no port is powered: every pin is a high-impedance input" },
        "ports": rows.iter().map(row_json).collect::<Vec<_>>(), "recorded_by_this_server": rec, "log_problems": problems,
        "limits": {"max_volts": inst.max_volts, "max_on_s": inst.max_on_s, "max_trip_a": inst.glasgow.max_trip_a}}))
}

/// The tool `bench_glasgow_safe_state`.
pub fn safe_state(ctx: &Ctx, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: glasgow_safe_state]
    let _lock = device_lock(&ctx.lock_path)?;
    safe(ctx, inst)
}

fn flag(args: &Value, key: &str) -> Result<bool, String> {
    // [@ANCHOR: glasgow_flag]
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// A number argument within `lo..=hi`; the error names the range and its source. `default` makes it optional.
pub(crate) fn number(
    args: &Value,
    key: &str,
    lo: f64,
    hi: f64,
    why: &str,
    default: Option<f64>,
) -> Result<f64, String> {
    // [@ANCHOR: glasgow_number]
    let v = match (args.get(key), default) {
        (None | Some(Value::Null), Some(d)) => d,
        (v, _) => v
            .and_then(|v| v.as_f64())
            .ok_or_else(|| format!("{key} is required and must be a number"))?,
    };
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(format!("{key} {v} is outside {lo}..={hi} ({why})"))
    }
}

/// The device's revision, from the serial in the config or, with one device attached, the one `glasgow list` shows.
pub(crate) fn revision(ctx: &Ctx, inst: &Instrument) -> Result<(String, f64, f64), String> {
    // [@ANCHOR: glasgow_revision]
    let serial =
        match &inst.serial {
            Some(s) => s.clone(),
            None => {
                let out = run(ctx, req(vec!["list".into()], 30), "glasgow list")?;
                let l = parse::device_list(&out.stdout);
                match l.as_slice() {
                    [one] => one.clone(),
                    [] => return Err(next_step("no Glasgow device is listed")),
                    _ => return Err(
                        "several Glasgow devices are connected; set \"serial\" in the config entry"
                            .into(),
                    ),
                }
            }
        };
    parse::revision_limits(&serial).ok_or_else(|| format!("{serial:?} is not a Glasgow serial"))
}

/// Switches every port off after a failure that may have left something powered; the original error is returned with the outcome.
pub(crate) fn fail_closed(ctx: &Ctx, inst: &Instrument, why: String) -> String {
    // [@ANCHOR: glasgow_fail_closed]
    match safe(ctx, inst) {
        Ok(_) => format!("{why}. The ports were switched off and read back at 0 V; the recorded settings were cleared."),
        Err(e) => format!("{why}. {e}"),
    }
}

/// The tool `bench_glasgow_port_set`.
pub fn port_set(ctx: &Ctx, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: glasgow_port_set]
    let port = args
        .get("port")
        .and_then(|p| p.as_str())
        .filter(|p| matches!(*p, "A" | "B"))
        .ok_or("port is required: \"A\" or \"B\"")?;
    let volts = number(
        args,
        "volts_v",
        0.0,
        inst.max_volts,
        "the max_volts of this instrument's config",
        None,
    )?;
    let trip = number(
        args,
        "trip_current_a",
        0.0,
        inst.glasgow.max_trip_a.min(0.325),
        "the config's max_trip_a, hardware limit 0.325 A",
        Some(0.1f64.min(inst.glasgow.max_trip_a)),
    )?;
    let dry = flag(args, "dry_run")?;
    let confirm = flag(args, "confirm")?;
    let (rev, lo, hi) = revision(ctx, inst)?;
    if volts != 0.0 && !(lo..=hi).contains(&volts) {
        return Err(format!("volts_v {volts} is outside {lo}..={hi} V (the range of a revision {rev} Glasgow); 0 switches the port off"));
    }
    let v = format!("{volts:.2}");
    let mut plan = vec![format!(
        "glasgow run control-gpio --pins {port}0:7  (loads an all-inputs gateware first)"
    )];
    plan.push(format!("glasgow voltage-limit {port} {v}"));
    plan.push(format!("glasgow voltage {port} {v} {trip:.3}"));
    plan.push("glasgow voltage (readback; both Vsupply and Vlimit must equal the setting)".into());
    if dry {
        return Ok(
            json!({"dry_run": true, "sent": [], "would": plan, "revision": rev, "note": "nothing was sent to the device"}),
        );
    }
    if volts != 0.0 && !confirm {
        return Err(format!("bench_glasgow_port_set powers port {port} at {volts} V; call it with confirm: true only when the person at the bench has said this port may be powered at that voltage (dry_run: true shows the plan)"));
    }
    let _lock = device_lock(&ctx.lock_path)?;
    // The deadline is built before anything energizes, so no failure can follow the rail coming on and leave it without a timer.
    let window = Duration::try_from_secs_f64(inst.max_on_s)
        .map_err(|e| format!("max_on_s {} cannot be used as a time: {e}", inst.max_on_s))?;
    let deadline = Instant::now()
        .checked_add(window)
        .ok_or_else(|| format!("max_on_s {} is too large", inst.max_on_s))?;
    let ch = port.chars().next().unwrap_or('A');
    // The pins are made inputs while the rail is still off, so nothing a previous gateware drove is connected when the rail comes up.
    ops::park(ctx, inst, ch, false)?;
    let (rows, _) = read_ports(ctx, inst)?;
    let cur = rows
        .iter()
        .find(|r| r.port == port)
        .ok_or_else(|| format!("the device has no port {port}"))?;
    let b = base_args(inst);
    let step = |cmd: &[&str]| -> Result<(), String> {
        let mut a = b.clone();
        a.extend(cmd.iter().map(|s| s.to_string()));
        let o = run(ctx, req(a, 30), "glasgow voltage")?;
        if o.code != Some(0) {
            return Err(format!(
                "glasgow {} exited {:?}: {}",
                cmd[0],
                o.code,
                tail(&o.stderr, 3)
            ));
        }
        Ok(())
    };
    let go = || -> Result<(), String> {
        let (lim, sup) = (
            ["voltage-limit", port, v.as_str()],
            ["voltage", port, v.as_str(), &format!("{trip:.3}")],
        );
        // Lowering: the supply first, then the limit. Raising: the limit first, so the hardware never has less limit than supply.
        if volts < cur.vsupply_v {
            step(&sup)?;
            step(&lim)?;
        } else {
            step(&lim)?;
            step(&sup)?;
        }
        Ok(())
    };
    if let Err(e) = go() {
        return Err(fail_closed(
            ctx,
            inst,
            format!("setting port {port} failed ({e})"),
        ));
    }
    let (after, _) = read_ports(ctx, inst).map_err(|e| {
        fail_closed(
            ctx,
            inst,
            format!("the readback after setting port {port} failed ({e})"),
        )
    })?;
    let row = after
        .iter()
        .find(|r| r.port == port)
        .ok_or_else(|| fail_closed(ctx, inst, "the readback has no such port".into()))?;
    let tol = VOLT_TOLERANCE_V.max(volts * 0.05);
    if (row.vsupply_v - volts).abs() > tol
        || (volts > 0.0 && (row.vlimit_v - volts).abs() > 0.05)
        || row.fault
        || row.short
    {
        return Err(fail_closed(
            ctx,
            inst,
            format!(
                "port {port} reads supply {:.3} V, limit {:.3} V{}{} after setting {volts} V",
                row.vsupply_v,
                row.vlimit_v,
                if row.fault { ", FAULT" } else { "" },
                if row.short { ", SHORT" } else { "" }
            ),
        ));
    }
    let (runner, serial, tmo) = (ctx.runner.clone(), inst.serial.clone(), window);
    with_session(|s| {
        let e = s.entry(inst.name.clone()).or_insert_with(|| Session {
            ports: BTreeMap::new(),
            runner: runner.clone(),
            serial,
            timeout: tmo,
        });
        e.runner = runner;
        if volts == 0.0 {
            e.ports.remove(&ch);
        } else {
            e.ports.insert(ch, PortRec { volts, deadline });
        }
        if e.ports.is_empty() {
            s.remove(&inst.name);
        }
    });
    if volts != 0.0 {
        spawn_watchdog(
            inst.clone(),
            ctx.lock_path.clone(),
            ctx.runner.clone(),
            ctx.sysfs.clone(),
            ctx.out_dir.clone(),
            deadline,
        );
    }
    Ok(
        json!({"port": port, "set_v": volts, "limit_v": row.vlimit_v, "measured_v": row.vsense_v, "trip_current_a": trip, "outputs_enabled": volts != 0.0,
        "pins": "inputs (parked)", "switches_off_in_s": if volts != 0.0 { Some(inst.max_on_s) } else { None }, "ports": after.iter().map(row_json).collect::<Vec<_>>()}),
    )
}

/// After `deadline` (the config's `max_on_s`), switches every port off unless the port was set again meanwhile.
fn spawn_watchdog(
    inst: Instrument,
    lock_path: PathBuf,
    runner: Arc<dyn Runner>,
    sysfs: PathBuf,
    out_dir: PathBuf,
    deadline: Instant,
) {
    // [@ANCHOR: glasgow_watchdog]
    std::thread::spawn(move || loop {
        let now = Instant::now();
        if now < deadline {
            std::thread::sleep((deadline - now).min(Duration::from_millis(200)));
            continue;
        }
        let guard = crate::instrument_lock(&inst.name);
        let _held = guard.lock().unwrap_or_else(|p| p.into_inner());
        let due = with_session(|s| {
            s.get(&inst.name)
                .map(|x| x.ports.values().any(|p| p.deadline <= Instant::now()))
        });
        if due == Some(true) {
            let ctx = Ctx {
                runner: runner.clone(),
                sysfs: sysfs.clone(),
                out_dir: out_dir.clone(),
                lock_path: lock_path.clone(),
            };
            match device_lock(&lock_path) {
                Ok(_l) => {
                    if let Err(e) = safe(&ctx, &inst) {
                        eprintln!("bench_mcp glasgow: the max_on_s watchdog could not confirm the safe state: {e}");
                    }
                }
                Err(e) => eprintln!("bench_mcp glasgow: the watchdog could not take the lock: {e}"),
            }
        }
        return;
    });
}

/// Called when the server stops: every Glasgow this process powered is switched off.
pub fn shutdown() {
    shutdown_prefix("");
}

/// The same for the instruments whose names start with `prefix` (the tests use their own names; production uses the empty prefix).
pub fn shutdown_prefix(prefix: &str) {
    // [@ANCHOR: glasgow_shutdown]
    let names: Vec<String> = with_session(|s| {
        s.keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    });
    for n in names {
        let rec = with_session(|s| {
            s.get(&n)
                .map(|x| (x.runner.clone(), x.serial.clone(), x.timeout))
        });
        let Some((runner, serial, _)) = rec else {
            continue;
        };
        let mut a = vec![];
        if let Some(s) = serial {
            a.extend(["--serial".to_string(), s]);
        }
        a.push("safe".into());
        match runner.run(&req(a, 30)) {
            Ok(o) if o.code == Some(0) => {}
            other => eprintln!("bench_mcp glasgow: could not confirm 'glasgow safe' for {n} at shutdown: {other:?}. Unplug the Glasgow."),
        }
        with_session(|s| s.remove(&n));
    }
}

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: glasgow_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Glasgow).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no glasgow instrument named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no glasgow instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: those of the `glasgow` driver at or below the configured tier.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    // [@ANCHOR: glasgow_tool_defs]
    SPECS
        .iter()
        .chain(ops::SPECS.iter())
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Glasgow && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    let inst = instrument(insts, args)?;
    call_with(&real_ctx(inst), insts, name, args)
}

/// `call` with the way to reach the device given, so the tests run the whole path (tier check, argument checks, the dialect).
pub fn call_with(
    ctx: &Ctx,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: glasgow_call_with]
    let spec = SPECS
        .iter()
        .chain(ops::SPECS.iter())
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    match name {
        "bench_glasgow_list" => list(ctx, inst),
        "bench_glasgow_status" => status(ctx, inst),
        "bench_glasgow_applets" => Ok(ops::applets()),
        "bench_glasgow_safe_state" => safe_state(ctx, inst),
        "bench_glasgow_port_set" => port_set(ctx, inst, args),
        "bench_glasgow_prepare" => ops::prepare(ctx, inst, args),
        _ => ops::call(ctx, inst, name, args),
    }
}

#[cfg(test)]
mod tests;

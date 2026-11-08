// SPDX-License-Identifier: AGPL-3.0-or-later
//! The applet tools of the Glasgow module. Each tool is one named applet from the allow-list below with its pins; the pins' ports must
//! have been set by `bench_glasgow_port_set`. `execute` is the one path every tool takes: lock, session check, device readback, link
//! check, run, park the pins, answer.

use super::parse;
use super::runner::Req;
use super::{base_args, fail_closed, flag, next_step, number, read_ports, run, tail, with_session, Ctx, Spec, VOLT_TOLERANCE_V};
use crate::config::{Instrument, Tier};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

macro_rules! props {
    ($($rest:tt)*) => { concat!("{", $($rest)*, "}") };
}

const HELPER: &str = include_str!("ops.py");

/// The applet tools. Pins are `A0` to `A7` and `B0` to `B7`; a role without a default must be given.
pub const SPECS: &[Spec] = &[
    Spec { name: "bench_glasgow_i2c_scan", tier: Tier::Full,
        desc: "GLASGOW, DRIVES SCL AND SDA (open-drain, pull-ups are the target's): addresses 0x08 to 0x77 (or `from_address` to `to_address`) probed with an empty write; returns the addresses that acknowledged. Needs the port set (bench_glasgow_port_set) and a high-speed USB link. A scan can upset a device that does not expect it; `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"scl":{"type":"string","description":"pin, default A0"},"sda":{"type":"string","description":"pin, default A1"},"frequency_khz":{"type":"integer","description":"SCL, 100 to 400, default 100"},"from_address":{"type":"integer","description":"8 to 119, default 8"},"to_address":{"type":"integer","description":"8 to 119, default 119"},"dry_run":{"type":"boolean"}"#), required: &[] },
    Spec { name: "bench_glasgow_i2c_read", tier: Tier::Full,
        desc: "GLASGOW, DRIVES SCL AND SDA: reads `count` bytes (1 to 256) from the device at `address`, first writing the optional `register_hex` bytes (a repeated start, as EEPROMs and sensors expect). Returns whether the device acknowledged, and the bytes as hex. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"address":{"type":"integer","description":"7-bit address, 8 to 119"},"register_hex":{"type":"string","description":"bytes to write first, e.g. 0f or 00 10"},"count":{"type":"integer","description":"1 to 256, default 1"},"scl":{"type":"string"},"sda":{"type":"string"},"frequency_khz":{"type":"integer"},"dry_run":{"type":"boolean"}"#), required: &["address"] },
    Spec { name: "bench_glasgow_i2c_write", tier: Tier::Full,
        desc: "GLASGOW, WRITES TO THE TARGET: sends `data_hex` (at most 256 bytes, the register address first if the device has one) to the device at `address`. `confirm` must be true, from the person at the bench who knows what this write does to the target. Returns whether every byte was acknowledged. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"address":{"type":"integer","description":"7-bit address, 8 to 119"},"data_hex":{"type":"string"},"confirm":{"type":"boolean","description":"true: the person at the bench agrees this write may be made"},"scl":{"type":"string"},"sda":{"type":"string"},"frequency_khz":{"type":"integer"},"dry_run":{"type":"boolean"}"#), required: &["address", "data_hex"] },
    Spec { name: "bench_glasgow_spi_jedec_id", tier: Tier::Full,
        desc: "GLASGOW, DRIVES CS, SCK AND COPI: reads a SPI flash's JEDEC ID (opcode 0x9f, three bytes back). Returns the manufacturer, memory type and capacity bytes and the raw reply; `all_ones` or `all_zeros` true means nothing drove the data line. Needs the port set. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"cs":{"type":"string","description":"default A0"},"sck":{"type":"string","description":"default A1"},"copi":{"type":"string","description":"default A2"},"cipo":{"type":"string","description":"default A3"},"mode":{"type":"integer","description":"SPI mode 0 to 3, default 0"},"frequency_khz":{"type":"integer","description":"10 to 8000, default 100"},"dry_run":{"type":"boolean"}"#), required: &[] },
    Spec { name: "bench_glasgow_uart_transact", tier: Tier::Full,
        desc: "GLASGOW, DRIVES TX when it sends: optionally sends `send_hex` or `send_text` (plus `append`), then collects what comes back until `until_hex` is seen, `idle_ms` of silence after data, `max_bytes`, or `timeout_s` (at most the config's max_capture_s). Returns the bytes as hex, how it ended and the elapsed time. Sending needs `confirm: true`. 8 data bits, 1 to 8 stop bits, parity as given. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"rx":{"type":"string","description":"default A0"},"tx":{"type":"string","description":"default A1"},"baud":{"type":"integer","description":"50 to 3000000, default 115200"},"parity":{"type":"string","enum":["none","even","odd","zero","one"]},"stop_bits":{"type":"integer"},"send_hex":{"type":"string"},"send_text":{"type":"string"},"append":{"type":"string","enum":["none","cr","lf","crlf"]},"until_hex":{"type":"string"},"timeout_s":{"type":"number","description":"default 2"},"idle_ms":{"type":"integer","description":"default 200"},"max_bytes":{"type":"integer","description":"1 to 65536, default 4096"},"confirm":{"type":"boolean","description":"true: sending to the target is agreed"},"dry_run":{"type":"boolean"}"#), required: &[] },
    Spec { name: "bench_glasgow_uart_sniff", tier: Tier::ReadConfig,
        desc: "GLASGOW, PASSIVE: listens on two UART lines (both inputs; the pin only says which channel the bytes belong to) for `duration_s` (at most the config's max_capture_s), writes the capture to a CSV file in the output directory and returns the bytes per channel as hex and text with frame and parity error counts. Needs the port set (it fixes the logic threshold).",
        props: props!(r#""instrument":{"type":"string"},"rx":{"type":"string","description":"default A0"},"tx":{"type":"string","description":"default A1"},"baud":{"type":"integer","description":"default 115200"},"parity":{"type":"string","enum":["none","even","odd","zero","one"]},"duration_s":{"type":"number","description":"1 up to the config's max_capture_s, default 5"}"#), required: &[] },
    Spec { name: "bench_glasgow_logic_capture", tier: Tier::ReadConfig,
        desc: "GLASGOW, PASSIVE: records 1 to 16 pins (`pins`, e.g. A0:3) as a logic analyzer for `duration_s` (at most the config's max_capture_s; also stopped at the config's max_capture_mb), saves a VCD waveform file in the output directory and returns a summary per pin: transitions, fraction of time high, shortest and longest pulse, rising-edge frequency, first and last level, and whether the FIFO overran. `pull` enables weak pull-ups or pull-downs on every pin. Needs the port set.",
        props: props!(r#""instrument":{"type":"string"},"pins":{"type":"string","description":"pin spec, e.g. A0:3 or A0:1,B0; default A0:3"},"duration_s":{"type":"number","description":"default 2"},"pull":{"type":"string","enum":["none","up","down"]},"names":{"type":"string","description":"comma separated pin names for the VCD"}"#), required: &[] },
    Spec { name: "bench_glasgow_gpio", tier: Tier::Full,
        desc: "GLASGOW, DRIVES PINS: applies `actions` to pins in order: `A0=1` or `A0=0` drive strongly, `A1=H` or `A1=L` pull weakly, `A2=Z` release, `A3` reads. The levels are held for `hold_s` (0 to 30), then every pin is released again (parked as inputs) and the answer says so. Returns each read. Driving a level needs `confirm: true`. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"pins":{"type":"string","description":"the pins the applet owns, e.g. A0:3"},"actions":{"type":"array","items":{"type":"string"}},"hold_s":{"type":"number"},"confirm":{"type":"boolean"},"dry_run":{"type":"boolean"}"#), required: &["pins", "actions"] },
    Spec { name: "bench_glasgow_pattern", tier: Tier::Full,
        desc: "GLASGOW, DRIVES PINS: a pattern or pulse generator timed from the host (about a millisecond, not the FPGA clock): `steps` is a list of {value, hold_ms}; `value` is a bit mask over `pins` (bit 0 = the first pin), `hold_ms` 0.5 to 5000. Repeated `repeat` times (1 to 100000); the whole run is at most the config's max_capture_s. All pins are driven during the run and released at the end, even on failure. `confirm` must be true. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"pins":{"type":"string","description":"1 to 8 pins, e.g. A0:3"},"steps":{"type":"array","items":{"type":"object","properties":{"value":{"type":"integer"},"hold_ms":{"type":"number"}},"required":["value","hold_ms"]}},"repeat":{"type":"integer"},"confirm":{"type":"boolean"},"dry_run":{"type":"boolean"}"#), required: &["pins", "steps"] },
    Spec { name: "bench_glasgow_jtag_scan", tier: Tier::Full,
        desc: "GLASGOW, DRIVES TCK, TMS AND TDI: scans the JTAG chain and returns each TAP's IR length and IDCODE with the manufacturer id, part number and version fields. Does not use the dangerous IR enumeration. Needs the port set. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"tck":{"type":"string","description":"default A0"},"tms":{"type":"string","description":"default A1"},"tdi":{"type":"string","description":"default A2"},"tdo":{"type":"string","description":"default A3"},"frequency_khz":{"type":"integer","description":"10 to 4000, default 100"},"dry_run":{"type":"boolean"}"#), required: &[] },
    Spec { name: "bench_glasgow_swd_idcode", tier: Tier::Full,
        desc: "GLASGOW, DRIVES SWCLK AND SWDIO: the ARM SWD line reset and JTAG-to-SWD switch sequences, then reads DPIDR; returns it with its version, designer, part number and revision fields. Reads nothing else from the target. Needs the port set. `dry_run` returns the plan.",
        props: props!(r#""instrument":{"type":"string"},"swclk":{"type":"string","description":"default A0"},"swdio":{"type":"string","description":"default A1"},"frequency_khz":{"type":"integer","description":"10 to 4000, default 100"},"dry_run":{"type":"boolean"}"#), required: &[] },
];

/// The tool `bench_glasgow_applets`: what can be run, from the same table the tools use.
pub fn applets() -> Value {
    // [@ANCHOR: glasgow_applets]
    json!({"applets": [
        {"tool": "bench_glasgow_i2c_scan, _i2c_read, _i2c_write", "applet": "i2c-controller", "pins": {"scl": "A0", "sda": "A1"}},
        {"tool": "bench_glasgow_spi_jedec_id", "applet": "spi-controller", "pins": {"cs": "A0", "sck": "A1", "copi": "A2", "cipo": "A3"}},
        {"tool": "bench_glasgow_uart_transact", "applet": "uart", "pins": {"rx": "A0", "tx": "A1"}},
        {"tool": "bench_glasgow_uart_sniff", "applet": "uart-analyzer", "pins": {"rx": "A0", "tx": "A1"}},
        {"tool": "bench_glasgow_logic_capture", "applet": "analyzer", "pins": {"pins": "A0:3"}},
        {"tool": "bench_glasgow_gpio, bench_glasgow_pattern", "applet": "control-gpio", "pins": {"pins": "given"}},
        {"tool": "bench_glasgow_jtag_scan", "applet": "jtag-probe", "pins": {"tck": "A0", "tms": "A1", "tdi": "A2", "tdo": "A3"}},
        {"tool": "bench_glasgow_swd_idcode", "applet": "swd-probe", "pins": {"swclk": "A0", "swdio": "A1"}}],
        "not_provided": ["flash", "factory", "firmware changes", "any applet outside this list", "free-form command lines", "I/O voltage set by a tool argument other than bench_glasgow_port_set"],
        "ports": ["A", "B"], "pins_per_port": 8, "note": "the first call of each pin configuration builds its gateware (5 to 20 s): bench_glasgow_prepare does it ahead of time"})
}

/// A pin argument: one pin (default from the table), returned as text and port.
fn pin(args: &Value, key: &str, default: Option<&str>) -> Result<(String, char), String> {
    // [@ANCHOR: glasgow_pin_arg]
    let spec = match (args.get(key), default) {
        (None | Some(Value::Null), Some(d)) => d.to_string(),
        (Some(v), _) => v.as_str().ok_or_else(|| format!("{key} must be a pin such as A0"))?.to_string(),
        (None, None) => return Err(format!("{key} is required (a pin such as A0)")),
    };
    let p = parse::pins(&spec).map_err(|e| format!("{key}: {e}"))?;
    if p.len() != 1 {
        return Err(format!("{key} must be one pin, not {spec:?}"));
    }
    Ok((spec, p[0].0))
}

/// What a tool will run.
struct Plan {
    tool: &'static str,
    applet: &'static str,
    /// Options of the applet, before the operation (pins, runtime settings).
    opts: Vec<String>,
    /// For a `run`: the operation and its arguments. For a `script`: empty (see `script`).
    tail: Vec<String>,
    /// For a `script`: the helper operation and its JSON payload.
    script: Option<(&'static str, Value)>,
    ports: BTreeSet<char>,
    needs_hs: bool,
    timeout: Duration,
    stop_after: Option<Duration>,
    watch: Option<(std::path::PathBuf, u64)>,
    hold: Duration,
    /// Roles the same pin must not fill twice.
    pins: Vec<String>,
}

fn distinct(pins: &[&String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for p in pins {
        if !seen.insert(p.as_str()) {
            return Err(format!("pin {p} is used for two roles"));
        }
    }
    Ok(())
}

fn freq(args: &Value, lo: f64, hi: f64, def: f64) -> Result<String, String> {
    Ok(format!("{}", number(args, "frequency_khz", lo, hi, "the applet's range", Some(def))? as u64))
}

/// Builds the plan of a tool from its arguments, checking every one before anything is sent.
fn plan(ctx: &Ctx, inst: &Instrument, name: &str, args: &Value) -> Result<Plan, String> {
    // [@ANCHOR: glasgow_plan]
    let cfg = &inst.glasgow;
    let op_timeout = Duration::from_secs(cfg.op_timeout_s);
    let mut p = Plan { tool: "", applet: "", opts: vec![], tail: vec![], script: None, ports: BTreeSet::new(), needs_hs: true, timeout: op_timeout, stop_after: None, watch: None, hold: Duration::ZERO, pins: vec![] };
    let role = |p: &mut Plan, flag: &str, key: &str, def: Option<&str>| -> Result<String, String> {
        let (spec, port) = pin(args, key, def)?;
        p.opts.extend([flag.to_string(), spec.clone()]);
        p.ports.insert(port);
        p.pins.push(spec.clone());
        Ok(spec)
    };
    let hex_arg = |key: &str, max: usize| -> Result<Vec<u8>, String> {
        let b = parse::hex_bytes(args.get(key).and_then(|v| v.as_str()).unwrap_or(""))?;
        if b.len() > max {
            return Err(format!("{key} is {} bytes; at most {max}", b.len()));
        }
        Ok(b)
    };
    let addr = || -> Result<u64, String> { Ok(number(args, "address", 8.0, 119.0, "the 7-bit I2C addresses outside the reserved ranges", None)? as u64) };
    match name {
        "bench_glasgow_i2c_scan" | "bench_glasgow_i2c_read" | "bench_glasgow_i2c_write" => {
            p.applet = "i2c-controller";
            role(&mut p, "--scl", "scl", Some("A0"))?;
            role(&mut p, "--sda", "sda", Some("A1"))?;
            distinct(&p.pins.iter().collect::<Vec<_>>())?;
            p.opts.extend(["-f".into(), freq(args, 100.0, 400.0, 100.0)?]);
            let (op, payload) = match name {
                "bench_glasgow_i2c_scan" => {
                    p.tool = "i2c";
                    let lo = number(args, "from_address", 8.0, 119.0, "the usable I2C addresses", Some(8.0))? as u64;
                    let hi = number(args, "to_address", 8.0, 119.0, "the usable I2C addresses", Some(119.0))? as u64;
                    if hi < lo {
                        return Err(format!("to_address {hi} is below from_address {lo}"));
                    }
                    ("i2c_scan", json!({"lo": lo, "hi": hi}))
                }
                "bench_glasgow_i2c_read" => {
                    p.tool = "i2c";
                    let count = number(args, "count", 1.0, 256.0, "bytes per read", Some(1.0))? as u64;
                    ("i2c_read", json!({"address": addr()?, "register": hex_arg("register_hex", 8)?, "count": count}))
                }
                _ => {
                    p.tool = "i2c";
                    let data = hex_arg("data_hex", 256)?;
                    if data.is_empty() {
                        return Err("data_hex is required and must not be empty".into());
                    }
                    ("i2c_write", json!({"address": addr()?, "data": data}))
                }
            };
            p.script = Some((op, payload));
        }
        "bench_glasgow_spi_jedec_id" => {
            p.tool = "spi";
            p.applet = "spi-controller";
            role(&mut p, "--cs", "cs", Some("A0"))?;
            role(&mut p, "--sck", "sck", Some("A1"))?;
            role(&mut p, "--copi", "copi", Some("A2"))?;
            role(&mut p, "--cipo", "cipo", Some("A3"))?;
            distinct(&p.pins.iter().collect::<Vec<_>>())?;
            let mode = number(args, "mode", 0.0, 3.0, "SPI modes", Some(0.0))? as u64;
            p.opts.extend(["-m".into(), mode.to_string(), "-f".into(), freq(args, 10.0, 8000.0, 100.0)?]);
            p.tail = ["9f", "00", "00", "00"].map(String::from).to_vec();
        }
        "bench_glasgow_uart_transact" | "bench_glasgow_uart_sniff" => {
            let sniff = name.ends_with("sniff");
            p.tool = if sniff { "uart_sniff" } else { "uart" };
            p.applet = if sniff { "uart-analyzer" } else { "uart" };
            role(&mut p, "--rx", "rx", Some("A0"))?;
            role(&mut p, "--tx", "tx", Some("A1"))?;
            distinct(&p.pins.iter().collect::<Vec<_>>())?;
            let parity = args.get("parity").and_then(|v| v.as_str()).unwrap_or("none");
            if !["none", "even", "odd", "zero", "one"].contains(&parity) {
                return Err(format!("parity {parity:?} must be none, even, odd, zero or one"));
            }
            p.opts.extend(["--parity".into(), parity.into()]);
            if !sniff {
                let sb = number(args, "stop_bits", 1.0, 8.0, "the UART applet's range", Some(1.0))? as u64;
                p.opts.extend(["--stop-bits".into(), sb.to_string()]);
            }
            let baud = number(args, "baud", 50.0, 3_000_000.0, "the UART applet at 48 MHz", Some(115200.0))? as u64;
            p.opts.extend(["-b".into(), baud.to_string()]);
            let maxc = cfg.max_capture_s;
            if sniff {
                let d = number(args, "duration_s", 1.0, maxc, "the config's max_capture_s", Some(5.0f64.min(maxc)))?;
                let file = ctx.out_dir.join(format!("glasgow_uart_{}.csv", stamp()));
                p.stop_after = Some(Duration::from_secs_f64(d));
                p.timeout = Duration::from_secs_f64(d) + op_timeout;
                p.watch = Some((file.clone(), cfg.max_capture_mb * 1_000_000));
                p.tail = vec![file.display().to_string()];
            } else {
                let mut send = hex_arg("send_hex", 4096)?;
                if let Some(t) = args.get("send_text").and_then(|v| v.as_str()) {
                    send.extend(t.as_bytes());
                }
                if !send.is_empty() {
                    send.extend(match args.get("append").and_then(|v| v.as_str()).unwrap_or("none") {
                        "none" => &b""[..],
                        "cr" => b"\r",
                        "lf" => b"\n",
                        "crlf" => b"\r\n",
                        o => return Err(format!("append {o:?} must be none, cr, lf or crlf")),
                    });
                }
                let t = number(args, "timeout_s", 0.1, maxc, "the config's max_capture_s", Some(2.0f64.min(maxc)))?;
                let idle = number(args, "idle_ms", 1.0, 10_000.0, "milliseconds of silence", Some(200.0))?;
                let mb = number(args, "max_bytes", 1.0, 65536.0, "bytes collected", Some(4096.0))? as u64;
                p.timeout = Duration::from_secs_f64(t) + op_timeout;
                p.script = Some(("uart_transact", json!({"send": send, "until": hex_arg("until_hex", 64)?, "timeout_s": t, "idle_ms": idle, "max_bytes": mb})));
            }
        }
        "bench_glasgow_logic_capture" => {
            p.tool = "logic";
            p.applet = "analyzer";
            let spec = args.get("pins").and_then(|v| v.as_str()).unwrap_or("A0:3");
            let pins = parse::pins(spec)?;
            p.ports.extend(pins.iter().map(|x| x.0));
            p.opts.extend(["--i".into(), spec.into()]);
            if let Some(n) = args.get("names").and_then(|v| v.as_str()) {
                if n.split(',').count() != pins.len() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == ',' || c == '_') {
                    return Err(format!("names must be {} comma separated names of letters, digits and underscores", pins.len()));
                }
                p.opts.extend(["--pin-names".into(), n.into()]);
            }
            match args.get("pull").and_then(|v| v.as_str()).unwrap_or("none") {
                "none" => {}
                "up" => p.opts.push("--pull-ups".into()),
                "down" => p.opts.push("--pull-downs".into()),
                o => return Err(format!("pull {o:?} must be none, up or down")),
            }
            let d = number(args, "duration_s", 0.1, cfg.max_capture_s, "the config's max_capture_s", Some(2.0f64.min(cfg.max_capture_s)))?;
            let file = ctx.out_dir.join(format!("glasgow_logic_{}.vcd", stamp()));
            p.stop_after = Some(Duration::from_secs_f64(d));
            p.timeout = Duration::from_secs_f64(d) + op_timeout;
            p.watch = Some((file.clone(), cfg.max_capture_mb * 1_000_000));
            p.tail = vec![file.display().to_string()];
        }
        "bench_glasgow_gpio" | "bench_glasgow_pattern" => {
            p.tool = if name.ends_with("gpio") { "gpio" } else { "pattern" };
            p.applet = "control-gpio";
            let spec = args.get("pins").and_then(|v| v.as_str()).ok_or("pins is required, e.g. A0:3")?;
            let pins = parse::pins(spec)?;
            p.ports.extend(pins.iter().map(|x| x.0));
            p.opts.extend(["--pins".into(), spec.into()]);
            if p.tool == "gpio" {
                p.needs_hs = false;
                let acts = args.get("actions").and_then(|v| v.as_array()).ok_or("actions is required: a list such as [\"A0=1\", \"A2\"]")?;
                if acts.is_empty() || acts.len() > 32 {
                    return Err("actions must hold 1 to 32 entries".into());
                }
                for a in acts {
                    let a = a.as_str().ok_or("each action is a string such as A0=1")?;
                    let b = a.as_bytes();
                    let ok = b.len() >= 2 && matches!(b[0], b'A' | b'B') && (b'0'..=b'7').contains(&b[1]) && (b.len() == 2 || (b.len() == 4 && b[2] == b'=' && matches!(b[3], b'0' | b'1' | b'H' | b'L' | b'Z')));
                    if !ok || !pins.contains(&(b[0] as char, b[1] - b'0')) {
                        return Err(format!("action {a:?} is not a pin of {spec:?} with =0, =1, =H, =L, =Z or nothing"));
                    }
                    p.tail.push(a.to_string());
                }
                let hold = number(args, "hold_s", 0.0, 30.0f64.min(cfg.max_capture_s), "the longest hold", Some(0.0))?;
                p.hold = Duration::from_secs_f64(hold);
            } else {
                if pins.len() > 8 {
                    return Err("a pattern drives at most 8 pins".into());
                }
                let steps = args.get("steps").and_then(|v| v.as_array()).ok_or("steps is required: a list of {value, hold_ms}")?;
                if steps.is_empty() || steps.len() > 256 {
                    return Err("steps must hold 1 to 256 entries".into());
                }
                let repeat = number(args, "repeat", 1.0, 100_000.0, "repeats", Some(1.0))? as u64;
                let mut list = vec![];
                let mut per_round = 0.0;
                for s in steps {
                    let v = number(s, "value", 0.0, (1u32 << pins.len()) as f64 - 1.0, "a bit mask over the pins", None)? as u64;
                    let h = number(s, "hold_ms", 0.5, 5000.0, "the host timer's range", None)?;
                    per_round += h;
                    list.push(json!([v, h]));
                }
                let total = per_round * repeat as f64 / 1000.0;
                if total > cfg.max_capture_s {
                    return Err(format!("the pattern would run {total:.1} s; the config's max_capture_s is {} s", cfg.max_capture_s));
                }
                p.timeout = Duration::from_secs_f64(total) + op_timeout;
                p.needs_hs = false;
                let first = list[0][0].as_u64().unwrap_or(0);
                p.script = Some(("gpio_pattern", json!({"first": first, "steps": list, "repeat": repeat, "max_s": total + 2.0})));
            }
        }
        "bench_glasgow_jtag_scan" => {
            p.tool = "jtag";
            p.applet = "jtag-probe";
            for (f, k, d) in [("--tck", "tck", "A0"), ("--tms", "tms", "A1"), ("--tdi", "tdi", "A2"), ("--tdo", "tdo", "A3")] {
                role(&mut p, f, k, Some(d))?;
            }
            distinct(&p.pins.iter().collect::<Vec<_>>())?;
            p.opts.extend(["-f".into(), freq(args, 10.0, 4000.0, 100.0)?]);
            p.tail = vec!["scan".into()];
        }
        "bench_glasgow_swd_idcode" => {
            p.tool = "swd";
            p.applet = "swd-probe";
            role(&mut p, "--swclk", "swclk", Some("A0"))?;
            role(&mut p, "--swdio", "swdio", Some("A1"))?;
            distinct(&p.pins.iter().collect::<Vec<_>>())?;
            p.opts.extend(["-f".into(), freq(args, 10.0, 4000.0, 100.0)?]);
            p.script = Some(("swd_idcode", json!({})));
        }
        _ => return Err(format!("{name} does not apply to {:?}", inst.name)),
    }
    Ok(p)
}

fn stamp() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// `all inputs` for one port: loads the control-gpio gateware with no pin driven. With `release_pulls` (the rail is on) every pin's pull is
/// released too (`P0=Z` to `P7=Z`); with the rail off pulls cannot be configured and none is.
pub(crate) fn park(ctx: &Ctx, inst: &Instrument, port: char, release_pulls: bool) -> Result<(), String> {
    // [@ANCHOR: glasgow_park]
    let mut a = base_args(inst);
    a.extend(["run".into(), "control-gpio".into(), "--pins".into(), format!("{port}0:7")]);
    if release_pulls {
        a.extend((0..8).map(|i| format!("{port}{i}=Z")));
    }
    let mut r = Req { args: a, timeout: Duration::from_secs(inst.glasgow.op_timeout_s), stop_after: None, watch: None, nice: false };
    r.timeout = r.timeout.max(Duration::from_secs(60));
    let out = run(ctx, r, "parking the pins")?;
    if out.code != Some(0) {
        return Err(next_step(&format!("parking port {port} failed (glasgow exited {:?}: {})", out.code, tail(&out.stderr, 3))));
    }
    Ok(())
}

fn volt_spec(inst: &Instrument, ports: &BTreeSet<char>) -> Result<String, String> {
    let rec = with_session(|s| s.get(&inst.name).map(|x| x.ports.clone()));
    let rec = rec.unwrap_or_default();
    let now = Instant::now();
    let mut parts = vec![];
    for p in ports {
        let r = rec.get(p).ok_or_else(|| format!("port {p} has no voltage set by this server. Call bench_glasgow_port_set (port {p}, volts_v, confirm: true) first; a tool never takes the I/O voltage from its own arguments"))?;
        if r.deadline <= now {
            return Err(format!("EXPIRED:{p}"));
        }
        parts.push(format!("{p}={:.2}", r.volts));
    }
    Ok(parts.join(","))
}

/// The one path of every applet tool. Returns what the command printed and the argument list it ran.
fn execute(ctx: &Ctx, inst: &Instrument, p: &Plan, dry: bool) -> Result<Result<(super::runner::Out, Vec<String>), Value>, String> {
    // [@ANCHOR: glasgow_execute]
    let build_args = |volts: &str| -> Vec<String> {
        let mut a = base_args(inst);
        match &p.script {
            Some((op, payload)) => {
                a.extend(["script".into(), "<helper>".into(), p.applet.into(), "-V".into(), volts.into()]);
                a.extend(p.opts.clone());
                a.extend(["--".into(), (*op).into(), payload.to_string()]);
            }
            None => {
                a.extend(["run".into(), p.applet.into(), "-V".into(), volts.into()]);
                a.extend(p.opts.clone());
                a.extend(p.tail.clone());
            }
        }
        a
    };
    if dry {
        let volts = volt_spec(inst, &p.ports);
        let ready = volts.is_ok();
        let spec = volts.unwrap_or_else(|_| p.ports.iter().map(|q| format!("{q}=<not set>")).collect::<Vec<_>>().join(","));
        return Ok(Err(json!({"dry_run": true, "sent": [], "would_run": format!("glasgow {}", build_args(&spec).join(" ")), "ports_needed": p.ports.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            "ports_set_by_this_server": ready, "then": "every used port is parked (pins inputs, pulls released)", "note": "nothing was sent to the device"})));
    }
    let _lock = super::device_lock(&ctx.lock_path)?;
    let volts = match volt_spec(inst, &p.ports) {
        Ok(v) => v,
        Err(e) if e.starts_with("EXPIRED:") => return Err(fail_closed(ctx, inst, format!("port {} passed its max_on_s of {} s", &e[8..], inst.max_on_s))),
        Err(e) => return Err(e),
    };
    // The device must still be as this server left it.
    let (rows, _) = read_ports(ctx, inst).map_err(|e| fail_closed(ctx, inst, format!("the readback before the call failed ({e})")))?;
    let recorded = with_session(|s| s.get(&inst.name).map(|x| x.ports.clone())).unwrap_or_default();
    for port in &p.ports {
        let want = recorded.get(port).map(|r| r.volts).unwrap_or(0.0);
        let row = rows.iter().find(|r| r.port == port.to_string());
        let ok = row.is_some_and(|r| (r.vsupply_v - want).abs() <= VOLT_TOLERANCE_V.max(want * 0.05) && r.vlimit_v <= want + 0.05 && !r.fault && !r.short);
        if !ok {
            let seen = row.map(|r| format!("supply {:.3} V, limit {:.3} V", r.vsupply_v, r.vlimit_v)).unwrap_or_else(|| "no such port".into());
            return Err(fail_closed(ctx, inst, format!("port {port} is not as it was set ({want} V recorded; the device reports {seen})")));
        }
    }
    if p.needs_hs {
        if let Some(u) = parse::usb_device(&ctx.sysfs) {
            if u.get("high_speed").and_then(|h| h.as_bool()) == Some(false) {
                return Err(format!("the Glasgow is linked at {} Mb/s (full speed): this tool streams data over bulk endpoints that need high speed (480 Mb/s), so it would fail and make the device re-enumerate. Nothing was run; the ports stay as set. Next: a short data-rated USB 2.0 cable and another port (manual instruments/glasgow/troubleshooting), or bench_glasgow_safe_state to power the ports down.", u.get("link_speed_mbps").and_then(|s| s.as_f64()).unwrap_or(0.0)));
            }
        }
    }
    let helper = ctx.out_dir.join("glasgow_ops.py");
    if p.script.is_some() {
        let _ = std::fs::create_dir_all(&ctx.out_dir);
        if std::fs::read_to_string(&helper).ok().as_deref() != Some(HELPER) {
            std::fs::write(&helper, HELPER).map_err(|e| format!("{}: {e}", helper.display()))?;
        }
    }
    let mut args = build_args(&volts);
    if let Some(i) = args.iter().position(|a| a == "<helper>") {
        args[i] = helper.display().to_string();
    }
    let ran = run(ctx, Req { args: args.clone(), timeout: p.timeout, stop_after: p.stop_after, watch: p.watch.clone(), nice: false }, &format!("the {} applet", p.applet));
    if !p.hold.is_zero() && ran.is_ok() {
        std::thread::sleep(p.hold);
    }
    let mut parked = Ok(());
    for port in &p.ports {
        parked = parked.and(park(ctx, inst, *port, true));
    }
    let out = match (ran, parked) {
        (Ok(o), Ok(())) => o,
        (Err(e), Ok(())) => return Err(format!("{e} The pins were parked (inputs).")),
        (_, Err(e)) => return Err(fail_closed(ctx, inst, format!("the pins could not be parked after the call ({e})"))),
    };
    Ok(Ok((out, args)))
}

fn done(mut v: Value, p: &Plan, started: Instant) -> Value {
    if let Value::Object(m) = &mut v {
        m.insert("applet".into(), p.applet.into());
        m.insert("pins".into(), json!(p.pins));
        m.insert("pins_state".into(), "parked: all inputs, pulls released".into());
        m.insert("outputs_enabled".into(), true.into());
        m.insert("rails_note".into(), "the port voltage you set is still on until bench_glasgow_safe_state or max_on_s".into());
        m.insert("call_s".into(), json!((started.elapsed().as_secs_f64() * 1000.0).round() / 1000.0));
    }
    v
}

/// Every applet tool: arguments checked, the plan run through `execute`, the output turned into numbers.
pub fn call(ctx: &Ctx, inst: &Instrument, name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: glasgow_ops_call]
    let p = plan(ctx, inst, name, args)?;
    let dry = flag(args, "dry_run")?;
    let confirm = flag(args, "confirm")?;
    let drives = match name {
        "bench_glasgow_i2c_write" | "bench_glasgow_pattern" => true,
        "bench_glasgow_uart_transact" => p.script.as_ref().and_then(|(_, v)| v["send"].as_array()).is_some_and(|a| !a.is_empty()),
        "bench_glasgow_gpio" => p.tail.iter().any(|a| a.ends_with("=0") || a.ends_with("=1")),
        _ => false,
    };
    if drives && !confirm && !dry {
        return Err(format!("{name} drives pins or writes to the target; call it with confirm: true only when the person at the bench has agreed (dry_run: true shows the plan)"));
    }
    let started = Instant::now();
    let (out, _args) = match execute(ctx, inst, &p, dry)? {
        Err(plan) => return Ok(plan),
        Ok(x) => x,
    };
    let problems = parse::log_problems(&out.stderr);
    if !out.stopped && out.code != Some(0) {
        return Err(format!("the {} applet exited {:?}: {} The pins were parked.", p.applet, out.code, tail(&out.stderr, 4)));
    }
    let result = match name {
        n if p.script.is_some() => {
            let r = parse::result_line(&out.stdout).map_err(|e| format!("{e} ({}). The pins were parked.", tail(&out.stderr, 3)))?;
            let _ = n;
            r
        }
        "bench_glasgow_spi_jedec_id" => {
            let b = parse::hex_bytes(out.stdout.trim())?;
            if b.len() != 4 {
                return Err(format!("the SPI exchange returned {} bytes, expected 4: {:?}", b.len(), out.stdout.trim()));
            }
            json!({"reply_hex": out.stdout.trim(), "manufacturer_id": format!("0x{:02x}", b[1]), "memory_type": format!("0x{:02x}", b[2]), "capacity_id": format!("0x{:02x}", b[3]),
                "all_ones": b[1..].iter().all(|x| *x == 0xff), "all_zeros": b[1..].iter().all(|x| *x == 0)})
        }
        "bench_glasgow_jtag_scan" => {
            let taps = parse::jtag_taps(&out.stderr);
            json!({"taps": taps, "tap_count": taps.len()})
        }
        "bench_glasgow_gpio" => {
            let reads: serde_json::Map<String, Value> = out.stdout.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), json!(v.trim() == "1"))).collect();
            json!({"reads": reads, "held_s": p.hold.as_secs_f64(), "released": true})
        }
        "bench_glasgow_logic_capture" | "bench_glasgow_uart_sniff" => {
            let file = p.tail.last().cloned().unwrap_or_default();
            let text = std::fs::read_to_string(&file).map_err(|e| format!("the capture file {file} was not written ({e}): {}", tail(&out.stderr, 3)))?;
            let size = text.len();
            let summary = if name.ends_with("capture") {
                let s = parse::vcd_summary(&text)?;
                let overrun = s["pins"].as_object().is_some_and(|m| m.values().any(|v| v["unknown_samples"].as_u64().unwrap_or(0) > 0));
                json!({"file": file, "file_bytes": size, "summary": s, "overrun": overrun, "stopped_by": if out.stopped { "duration or size limit" } else { "the applet" }})
            } else {
                json!({"file": file, "file_bytes": size, "channels": parse::uart_csv_summary(&text, 4096)?, "stopped_by": if out.stopped { "duration or size limit" } else { "the applet" }})
            };
            summary
        }
        _ => return Err(format!("{name}: no result parser")),
    };
    let mut v = done(result, &p, started);
    if let Value::Object(m) = &mut v {
        m.insert("log_problems".into(), json!(problems));
    }
    Ok(v)
}

/// The tool `bench_glasgow_prepare`: builds the gateware of a tool's applet into the cache. Needs no device.
pub fn prepare(ctx: &Ctx, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: glasgow_prepare]
    let tool = args.get("tool").and_then(|t| t.as_str()).ok_or("tool is required")?;
    let name = match tool {
        "i2c" => "bench_glasgow_i2c_scan",
        "uart" => "bench_glasgow_uart_transact",
        "uart_sniff" => "bench_glasgow_uart_sniff",
        "spi" => "bench_glasgow_spi_jedec_id",
        "logic" => "bench_glasgow_logic_capture",
        "gpio" => "bench_glasgow_gpio",
        "pattern" => "bench_glasgow_pattern",
        "jtag" => "bench_glasgow_jtag_scan",
        "swd" => "bench_glasgow_swd_idcode",
        o => return Err(format!("tool {o:?} is not one of i2c, uart, uart_sniff, spi, logic, gpio, pattern, jtag, swd")),
    };
    let mut a = args.clone();
    if let Value::Object(m) = &mut a {
        if tool == "gpio" {
            m.entry("actions").or_insert(json!(["A0"]));
        }
        if tool == "pattern" {
            m.entry("steps").or_insert(json!([{"value": 0, "hold_ms": 1}]));
        }
        m.entry("pins").or_insert(json!("A0:3"));
    }
    let p = plan(ctx, inst, name, &a)?;
    let rev = match args.get("revision").and_then(|r| r.as_str()) {
        Some(r) if ["A0", "B0", "C0", "C1", "C2", "C3", "D0"].contains(&r) => r.to_string(),
        Some(r) => return Err(format!("revision {r:?} must be one of A0, B0, C0, C1, C2, C3, D0")),
        None => super::revision(ctx, inst)?.0,
    };
    // The gateware does not depend on the I/O voltage or on runtime settings; the build takes the pin options and the build-time options only.
    let skip_with_value = ["-f", "-b", "-m"];
    let mut build_opts = vec![];
    let mut it = p.opts.iter();
    while let Some(o) = it.next() {
        if skip_with_value.contains(&o.as_str()) {
            it.next();
        } else {
            build_opts.push(o.clone());
        }
    }
    let scratch = std::path::Path::new("/dev/shm").join(format!("bench_glasgow_build_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&scratch);
    let file = scratch.join("out.bin");
    let mut cmd = vec!["build".to_string(), "--rev".into(), rev.clone(), "-t".into(), "bitstream".into(), "-f".into(), file.display().to_string(), p.applet.into()];
    cmd.extend(build_opts);
    if p.applet == "analyzer" {
        // the analyzer takes its pins as `--i`
    }
    let started = Instant::now();
    let out = run(ctx, Req { args: cmd.clone(), timeout: Duration::from_secs(inst.glasgow.build_timeout_s), stop_after: None, watch: None, nice: true }, "the gateware build");
    let _ = std::fs::remove_dir_all(&scratch);
    let out = out?;
    if out.code != Some(0) {
        return Err(next_step(&format!("the build exited {:?}: {}", out.code, tail(&out.stderr, 4))));
    }
    let cache = super::install_dir(&inst.glasgow).join("cache/GlasgowEmbedded/bitstreams");
    let bytes: u64 = std::fs::read_dir(&cache).map(|d| d.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0);
    Ok(json!({"built": true, "applet": p.applet, "revision": rev, "build_s": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0, "cache_bytes": bytes,
        "note": "the cache key covers the applet, its pins and the revision: call the tool with the same pins to use it"}))
}

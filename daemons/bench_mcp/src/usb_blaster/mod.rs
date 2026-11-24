// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Altera USB-Blaster JTAG cable, driven through the `openFPGALoader` command line (no vendor software).
//!
//! Three tools, three safety levels:
//!
//! * `bench_jtag_scan` (read_config): which cable is plugged in and which devices answer on its JTAG chain (IDCODE, family, model).
//!   It only reads identification registers.
//! * `bench_jtag_sram_load` (full): a `.sof` or `.svf` into the FPGA's SRAM. Volatile: a power cycle of the board clears it.
//!   Needs `confirm_sram_volatile: true`; `dry_run` shows the command.
//! * `bench_jtag_flash_write` (full): the board's configuration flash (EPCS and similar). Never listed and never run unless the
//!   config entry says `"allow_flash": true`, and then each call needs the owner's yes for that one action
//!   (`owner_approval` quotes it, `confirm_flash` repeats the file name). Nothing else in this server writes a flash.
//!
//! Entry: `{"driver": "usb_blaster", "tier": "read_config"}`. Optional: `program` (default `openFPGALoader`), `cable`
//! (`usb-blaster` or `usb-blasterII`, default `usb-blaster`), `allow_flash` (default false).

use crate::config::{Instrument, Kind, Tier};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Time limit of a scan: it only reads IDCODEs.
const SCAN_TIMEOUT_S: u64 = 20;
/// Time limit of a load or a flash write (a large EPCS write takes a few minutes).
const LOAD_TIMEOUT_S: u64 = 600;
/// A bitstream file larger than this is not a bitstream for these parts.
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const CABLES: &[&str] = &["usb-blaster", "usb-blasterII"];
const SRAM_EXTS: &[&str] = &["sof", "svf"];
const FLASH_EXTS: &[&str] = &["jic", "rpd", "pof", "rbf", "bin"];

// [@ANCHOR: bench_mcp_usb_blaster_types]
/// The `usb_blaster` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub program: String,
    pub cable: String,
    pub allow_flash: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            program: "openFPGALoader".into(),
            cable: "usb-blaster".into(),
            allow_flash: false,
        }
    }
}

/// Reads `program`, `cable` and `allow_flash`; a bad value is refused when the config file is read.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: usb_blaster_config]
    let mut cfg = Config::default();
    if let Some(v) = c.get("program") {
        let p = v
            .as_str()
            .filter(|p| !p.is_empty())
            .ok_or("program must be a non-empty string")?;
        cfg.program = p.into();
    }
    if let Some(v) = c.get("cable") {
        let p = v
            .as_str()
            .filter(|p| CABLES.contains(p))
            .ok_or(format!("cable must be one of {CABLES:?}"))?;
        cfg.cable = p.into();
    }
    if let Some(v) = c.get("allow_flash") {
        cfg.allow_flash = v.as_bool().ok_or("allow_flash must be true or false")?;
    }
    Ok(cfg)
}

/// What a command line run gave back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub code: Option<i32>,
    pub text: String,
    pub timed_out: bool,
}

/// How a call reaches `openFPGALoader`: production starts the program, tests answer from a fake cable.
pub trait Runner {
    fn run(&self, args: &[String], timeout: Duration) -> Result<Out, String>;
}

/// Starts the program as a child of this server and ends it (that one child only) when the time limit passes.
pub struct RealRunner {
    pub program: String,
}

impl Runner for RealRunner {
    fn run(&self, args: &[String], timeout: Duration) -> Result<Out, String> {
        // [@ANCHOR: usb_blaster_real_runner]
        let mut child = Command::new(&self.program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}. Next: install it (Debian: sudo apt install openfpgaloader) or set `program` in the config entry.", self.program))?;
        let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let a = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = out.read_to_string(&mut s);
            s
        });
        let b = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            s
        });
        let start = Instant::now();
        let mut timed_out = false;
        let code = loop {
            if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
                break st.code();
            }
            if start.elapsed() > timeout {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok().and_then(|s| s.code());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let text = format!(
            "{}{}",
            a.join().unwrap_or_default(),
            b.join().unwrap_or_default()
        );
        Ok(Out {
            code,
            text,
            timed_out,
        })
    }
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

/// Every tool of this instrument.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_jtag_scan",
        tier: Tier::ReadConfig,
        desc: "USB-Blaster JTAG cable: lists the cables on the USB bus and scans the JTAG chain of the cable in the config, returning each device's IDCODE, manufacturer, family, model and instruction register length. Reads identification registers only; loads and writes nothing. An empty or failed chain (board unpowered or not connected) is reported with the cause and the next step.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_jtag_sram_load",
        tier: Tier::Full,
        desc: "USB-Blaster JTAG cable, CHANGES THE TARGET: loads a `.sof` or `.svf` file (absolute path) into the FPGA's SRAM only. Volatile: a power cycle of the board clears it and the flash is never touched. The running design on the board is replaced and its pins change. Needs `confirm_sram_volatile: true`; `dry_run: true` shows the command and sends nothing. Refuses when the chain scan finds no device.",
        props: props!(r#""instrument":{"type":"string"},"file":{"type":"string","description":"absolute path of a .sof or .svf file"},"confirm_sram_volatile":{"type":"boolean","description":"true: the person at the bench knows this replaces the running design until the next power cycle"},"dry_run":{"type":"boolean"}"#),
        required: &["file"],
    },
    Spec {
        name: "bench_jtag_flash_write",
        tier: Tier::Full,
        desc: "USB-Blaster JTAG cable, WRITES THE BOARD'S CONFIGURATION FLASH (EPCS and similar). Listed only when the config entry has allow_flash true. EVERY CALL NEEDS THE OWNER'S EXPLICIT YES FOR THAT ONE ACTION: ask first, then pass `owner_approval` (the owner's words) and `confirm_flash` set to `WRITE FLASH <file name>`. A bad image can leave a board that does not start. `dry_run: true` shows the command and sends nothing.",
        props: props!(r#""instrument":{"type":"string"},"file":{"type":"string","description":"absolute path of a .jic, .rpd, .pof, .rbf or .bin file"},"owner_approval":{"type":"string","description":"the owner's yes for this write, quoted"},"confirm_flash":{"type":"string","description":"WRITE FLASH <file name>"},"dry_run":{"type":"boolean"}"#),
        required: &["file"],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: bench_mcp_usb_blaster_instrument]
    let ours: Vec<&Instrument> = insts
        .iter()
        .filter(|i| i.kind == Kind::UsbBlaster)
        .collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no usb_blaster instrument named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no usb_blaster instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: a tool appears only when a usb_blaster instrument is configured at or above its tier; the flash tool
/// also needs `allow_flash`.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    // [@ANCHOR: bench_mcp_usb_blaster_tool_defs]
    SPECS
        .iter()
        .filter(|s| {
            insts.iter().any(|i| {
                i.kind == Kind::UsbBlaster
                    && i.tier >= s.tier
                    && (s.name != "bench_jtag_flash_write" || i.usb_blaster.allow_flash)
            })
        })
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// A boolean argument that may be absent (false) but must not be anything else.
fn flag(args: &Value, key: &str) -> Result<bool, String> {
    // [@ANCHOR: bench_mcp_usb_blaster_flag]
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// The file argument: absolute, an existing regular file of an allowed extension and a sane size.
fn bitstream(args: &Value, exts: &[&str]) -> Result<String, String> {
    // [@ANCHOR: usb_blaster_file]
    let f = args
        .get("file")
        .and_then(|v| v.as_str())
        .ok_or("file is required (an absolute path)")?;
    let p = Path::new(f);
    if !p.is_absolute() {
        return Err(format!("file {f:?} must be an absolute path"));
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !exts.contains(&ext.as_str()) {
        return Err(format!(
            "file {f:?}: this tool takes {exts:?} files, not .{ext}"
        ));
    }
    let md = std::fs::metadata(p).map_err(|e| format!("file {f:?}: {e}"))?;
    if !md.is_file() || md.len() == 0 || md.len() > MAX_FILE_BYTES {
        return Err(format!(
            "file {f:?} is not a non-empty regular file of at most {MAX_FILE_BYTES} bytes"
        ));
    }
    Ok(f.to_string())
}

/// One device of a chain listing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub idcode: String,
    pub manufacturer: String,
    pub family: String,
    pub model: String,
    pub irlength: Option<u32>,
}

/// Reads openFPGALoader's `--detect` listing (`index N:` followed by `idcode`, `manufacturer`, `family`, `model`, `irlength`).
pub fn parse_chain(text: &str) -> Vec<Device> {
    // [@ANCHOR: usb_blaster_parse_chain]
    let mut out: Vec<Device> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("index ") && l.ends_with(':') {
            out.push(Device::default());
            continue;
        }
        let Some(d) = out.last_mut() else { continue };
        let (key, val) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
        let val = val.trim().to_string();
        match key {
            "idcode" => d.idcode = val,
            "manufacturer" => d.manufacturer = val,
            "family" => d.family = val,
            "model" => d.model = val,
            "irlength" => d.irlength = val.parse().ok(),
            _ => {}
        }
    }
    out
}

/// Cables on the USB bus: the rows of `--scan-usb` after its header, as vid:pid, probe type and manufacturer.
pub fn parse_cables(text: &str) -> Vec<Value> {
    // [@ANCHOR: usb_blaster_parse_cables]
    text.lines()
        .filter(|l| l.contains("0x") && l.contains(':'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 5).then(|| json!({"bus": f[0], "device": f[1], "vid_pid": f[2], "probe": f[3], "manufacturer": f[4]}))
        })
        .collect()
}

fn next_step(what: &str) -> String {
    format!("{what}. Next: check the USB cable, that the board is powered and its JTAG header is connected, and that this user may open the USB device (udev rule or sudo).")
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// The chain scan of the cable in the config; a failed scan is an error that carries the program's own message.
fn chain(r: &dyn Runner, cfg: &Config) -> Result<(Vec<Device>, String), String> {
    // [@ANCHOR: usb_blaster_chain]
    let out = r.run(
        &s(&["-c", &cfg.cable, "--detect"]),
        Duration::from_secs(SCAN_TIMEOUT_S),
    )?;
    if out.timed_out {
        return Err(next_step(&format!(
            "the chain scan did not finish in {SCAN_TIMEOUT_S} s"
        )));
    }
    let devs = parse_chain(&out.text);
    if out.code != Some(0) || devs.is_empty() {
        let msg = out
            .text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no output")
            .trim()
            .to_string();
        return Err(next_step(&format!(
            "no device answered on the JTAG chain (exit {:?}: {msg})",
            out.code
        )));
    }
    Ok((devs, out.text))
}

pub fn scan(r: &dyn Runner, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_usb_blaster_scan]
    let cfg = &inst.usb_blaster;
    let cables = r.run(&s(&["--scan-usb"]), Duration::from_secs(SCAN_TIMEOUT_S))?;
    let cables = parse_cables(&cables.text);
    if cables.is_empty() {
        return Err(next_step(
            "no USB-Blaster cable is on the USB bus (or this user may not open it)",
        ));
    }
    let (devs, _) = chain(r, cfg)?;
    let devices: Vec<Value> = devs
        .iter()
        .map(|d| json!({"idcode": d.idcode, "manufacturer": d.manufacturer, "family": d.family, "model": d.model, "irlength": d.irlength}))
        .collect();
    Ok(json!({"cables": cables, "cable": cfg.cable, "devices": devices}))
}

pub fn sram_load(r: &dyn Runner, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_usb_blaster_sram_load]
    let cfg = &inst.usb_blaster;
    let file = bitstream(args, SRAM_EXTS)?;
    let cmd = s(&["-c", &cfg.cable, "-m", &file]);
    if flag(args, "dry_run")? {
        return Ok(
            json!({"dry_run": true, "sent": [], "would": [format!("{} --detect", cfg.cable), format!("{} {}", cfg.program, cmd.join(" "))],
            "note": "nothing was sent to the cable; SRAM only, flash is never touched"}),
        );
    }
    if !flag(args, "confirm_sram_volatile")? {
        return Err(format!("bench_jtag_sram_load replaces the running design on the board with {file} until its next power cycle; call it with confirm_sram_volatile: true only when the person at the bench has said so (dry_run: true shows the command)"));
    }
    let (devs, _) = chain(r, cfg)?;
    let out = r.run(&cmd, Duration::from_secs(LOAD_TIMEOUT_S))?;
    if out.timed_out || out.code != Some(0) {
        return Err(next_step(&format!(
            "the SRAM load failed (exit {:?}, timed out: {}): {}",
            out.code,
            out.timed_out,
            tail(&out.text)
        )));
    }
    Ok(
        json!({"loaded": "sram", "file": file, "devices": devs.len(), "output_tail": tail(&out.text), "note": "volatile: a power cycle of the board clears it"}),
    )
}

/// The last lines of the program's output, enough to see what it said without returning a long log.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(6)..].join("\n")
}

pub fn flash_write(r: &dyn Runner, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_usb_blaster_flash_write]
    let cfg = &inst.usb_blaster;
    if !cfg.allow_flash {
        return Err("flash writes are off: the config entry of this cable has no \"allow_flash\": true. Only the owner turns that on.".into());
    }
    let file = bitstream(args, FLASH_EXTS)?;
    let base = Path::new(&file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let cmd = s(&["-c", &cfg.cable, "-f", &file]);
    if flag(args, "dry_run")? {
        return Ok(
            json!({"dry_run": true, "sent": [], "would": [format!("{} {}", cfg.program, cmd.join(" "))],
            "note": "nothing was sent; a real call also needs owner_approval and confirm_flash"}),
        );
    }
    let approval = args
        .get("owner_approval")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if approval.len() < 3 {
        return Err("bench_jtag_flash_write needs `owner_approval`: the owner's own yes for THIS write, quoted. Ask the owner for this action now; an earlier yes for another write does not count.".into());
    }
    let want = format!("WRITE FLASH {base}");
    if args.get("confirm_flash").and_then(|v| v.as_str()) != Some(want.as_str()) {
        return Err(format!("confirm_flash must be exactly {want:?}"));
    }
    chain(r, cfg)?;
    let out = r.run(&cmd, Duration::from_secs(LOAD_TIMEOUT_S))?;
    if out.timed_out || out.code != Some(0) {
        return Err(format!("the flash write FAILED (exit {:?}, timed out: {}): {}. The flash may be half written: do not power-cycle the board; retry the same write or recover over JTAG with the original image.", out.code, out.timed_out, tail(&out.text)));
    }
    Ok(json!({"written": "flash", "file": file, "output_tail": tail(&out.text)}))
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_usb_blaster_call]
    let inst = instrument(insts, args)?;
    let r = RealRunner {
        program: inst.usb_blaster.program.clone(),
    };
    call_with(&r, insts, name, args)
}

/// `call` with the way to reach the program given, so the tests run the whole path (tier check, argument checks, the
/// gates) against a fake cable.
pub fn call_with(
    r: &dyn Runner,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_usb_blaster_call_with]
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
    match name {
        "bench_jtag_scan" => scan(r, inst),
        "bench_jtag_sram_load" => sram_load(r, inst, args),
        "bench_jtag_flash_write" => flash_write(r, inst, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

#[cfg(test)]
mod tests;

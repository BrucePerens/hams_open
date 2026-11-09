// SPDX-License-Identifier: AGPL-3.0-or-later
//! The RX-888 MkII direct-sampling HF receiver, driven through ka9q-radio's `radiod` (driver `rx888`).
//!
//! The RX-888 is a Cypress FX3 USB 3 device with a 16-bit ADC that samples 0 to 30 MHz directly. It
//! appears unbooted as `04b4:00f3` (the FX3 boot loader) and, after its firmware is loaded into RAM,
//! as `04b4:00f1`. This module does not talk to the USB device itself: ka9q-radio's `rx888_boot` loads
//! the firmware and ka9q-radio's `radiod` (GPL-3.0, run unmodified as a separate process) runs the
//! receiver. The tools then talk to `radiod` through the `radiod_client` crate's closed command schema
//! on loopback multicast with TTL 0, so nothing leaves the host. Receive only: nothing here can
//! transmit, and no tool needs a safety tier above `read_config`.
//!
//! | Tool | Tier | What |
//! |---|---|---|
//! | `bench_rx888_status` | read_only | USB presence and boot state, the `radiod` this server started, the front end |
//! | `bench_rx888_frontend` | read_only | front end: sample rate, A/D overrange count, RF gain, attenuation, AGC, IF power |
//! | `bench_rx888_boot` | read_config | load the FX3 firmware with `rx888_boot` (the device re-enumerates) |
//! | `bench_rx888_radiod_start`, `_radiod_stop` | read_config | run or stop a `radiod` for the RX-888 on loopback |
//! | `bench_rx888_tune` | read_config | a channel at a frequency: level, noise density, SNR, measured input rate |
//! | `bench_rx888_spectrum` | read_config | a spectrum snapshot (bins over a span) as a CSV with a summary |
//! | `bench_rx888_capture` | read_config | a short I/Q or audio recording to a WAV file |
//!
//! The config entry (paths are where `make install` of ka9q-radio puts them; all must be absolute):
//! `{"driver": "rx888", "tier": "read_config", "radiod": "/usr/local/sbin/radiod",
//!   "driver_library": "/usr/local/lib/ka9q-radio/rx888.so", "boot": "/usr/local/sbin/rx888_boot",
//!   "firmware": "/usr/local/share/ka9q-radio/W1EUJ_0.1.0_FX3.img",
//!   "presets": "/usr/local/share/ka9q-radio/presets.conf",
//!   "status_group": "239.88.8.1", "data_group": "239.88.8.2"}`.

pub mod radio;
#[cfg(test)]
pub mod testkit;

use crate::config::{Instrument, Kind, Tier};
use radiod_client::net::Control;
use radiod_client::status::ChannelStatus;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cypress's USB vendor id.
pub const VID: &str = "04b4";
/// The FX3 boot loader: an RX-888 with no firmware loaded.
pub const PID_UNBOOTED: &str = "00f3";
/// The RX-888 running its firmware.
pub const PID_BOOTED: &str = "00f1";
/// Where the kernel lists USB devices.
pub const USB_SYSFS: &str = "/sys/bus/usb/devices";
/// The sample rates a `radiod_start` may ask for: half and full rate. Half rate covers 0 to 30 MHz
/// and was measured at 64,798,620 S/s on bruce16 (2026-11-05); full rate needs a faster host.
pub const SAMPLE_RATES_HZ: [u64; 2] = [64_800_000, 129_600_000];
/// How long `radiod_start` waits for the front end to answer.
const START_WAIT: Duration = Duration::from_secs(15);
/// How long `boot` waits for the booted device to appear.
const BOOT_WAIT: Duration = Duration::from_secs(20);

/// The `rx888` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The `radiod` binary.
    pub radiod: PathBuf,
    /// ka9q-radio's RX-888 driver, loaded by `radiod` with dlopen (`library =`).
    pub driver_library: PathBuf,
    /// ka9q-radio's firmware loader.
    pub boot: PathBuf,
    /// The FX3 firmware image (ka9q-radio's `share/W1EUJ_0.1.0_FX3.img`).
    pub firmware: PathBuf,
    /// `radiod`'s presets file (its `[global] mode` names one).
    pub presets: PathBuf,
    /// `radiod`'s status (control) multicast group.
    pub status_group: Ipv4Addr,
    /// `radiod`'s data (RTP) multicast group.
    pub data_group: Ipv4Addr,
    /// Status port.
    pub status_port: u16,
    /// Data port.
    pub data_port: u16,
    /// First SSRC the tools use (tune, spectrum, capture use `ssrc_base` to `ssrc_base + 2`).
    pub ssrc_base: u32,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            radiod: PathBuf::from("/usr/local/sbin/radiod"),
            driver_library: PathBuf::from("/usr/local/lib/ka9q-radio/rx888.so"),
            boot: PathBuf::from("/usr/local/sbin/rx888_boot"),
            firmware: PathBuf::from("/usr/local/share/ka9q-radio/W1EUJ_0.1.0_FX3.img"),
            presets: PathBuf::from("/usr/local/share/ka9q-radio/presets.conf"),
            status_group: Ipv4Addr::new(239, 88, 8, 1),
            data_group: Ipv4Addr::new(239, 88, 8, 2),
            status_port: 5006,
            data_port: 5004,
            ssrc_base: 88_000,
        }
    }
}

/// Reads the `rx888` keys. Every path must be absolute (`rx888_boot` prefixes its own install directory
/// to a relative firmware name, seen on 2026-11-05), the groups must be multicast, the ports non-zero.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: bench_mcp_rx888_config]
    let mut cfg = Config::default();
    let path = |key: &str, slot: &mut PathBuf| -> Result<(), String> {
        if let Some(v) = c.get(key) {
            let p = PathBuf::from(v.as_str().ok_or(format!("{key} must be a path"))?);
            if !p.is_absolute() {
                return Err(format!("{key} {p:?} must be an absolute path"));
            }
            *slot = p;
        }
        Ok(())
    };
    path("radiod", &mut cfg.radiod)?;
    path("driver_library", &mut cfg.driver_library)?;
    path("boot", &mut cfg.boot)?;
    path("firmware", &mut cfg.firmware)?;
    path("presets", &mut cfg.presets)?;
    let group = |key: &str, slot: &mut Ipv4Addr| -> Result<(), String> {
        if let Some(v) = c.get(key) {
            let text = v.as_str().ok_or(format!("{key} must be a string"))?;
            match text.parse::<Ipv4Addr>() {
                Ok(ip) if ip.is_multicast() => *slot = ip,
                _ => return Err(format!("{key} {text:?} is not an IPv4 multicast group")),
            }
        }
        Ok(())
    };
    group("status_group", &mut cfg.status_group)?;
    group("data_group", &mut cfg.data_group)?;
    let port = |key: &str, slot: &mut u16| -> Result<(), String> {
        if let Some(v) = c.get(key) {
            *slot = v
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .filter(|p| *p > 0)
                .ok_or(format!("{key} must be 1 to 65535"))?;
        }
        Ok(())
    };
    port("status_port", &mut cfg.status_port)?;
    port("data_port", &mut cfg.data_port)?;
    if let Some(v) = c.get("ssrc_base") {
        cfg.ssrc_base = v
            .as_u64()
            .and_then(|s| u32::try_from(s).ok())
            .filter(|s| (1..=0xffff_fff0).contains(s))
            .ok_or("ssrc_base must be 1 to 4294967280")?;
    }
    if cfg.status_group == cfg.data_group && cfg.status_port == cfg.data_port {
        return Err("status and data must differ in group or port".into());
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

pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_rx888_status",
        tier: Tier::ReadOnly,
        desc: "RX-888 MkII: whether it is on the USB bus and booted (04b4:00f3 is the unbooted FX3 loader, 04b4:00f1 runs its firmware), its sysfs path and link speed (5000 Mb/s is USB 3; the full rate needs it), whether the radiod this server started is running (pid, config, log), and, if radiod answers, the front end (sample rate, overrange count, RF gain). Changes nothing.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rx888_frontend",
        tier: Tier::ReadOnly,
        desc: "RX-888 through radiod: the front end's own status: input sample rate, A/D overrange count (and samples since the last overrange), RF gain, attenuation and whether the RF AGC is on, IF power in dBFS (-inf while no channel exists, the front end then idles), level calibration, usable edges, A/D bits. Needs radiod running. Changes nothing.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rx888_boot",
        tier: Tier::ReadConfig,
        desc: "RX-888: loads the FX3 firmware into the device's RAM with ka9q-radio's rx888_boot (the firmware path from the config, absolute), then waits up to 20 s for the device to come back booted (04b4:00f1) and reports its link speed. The device leaves the USB bus and re-enumerates; nothing is written to flash. Refused when the device is already booted or not present, or while radiod runs. If the device does not come back, power-cycle its hub port (bench_usb_port_power) and try once more.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rx888_radiod_start",
        tier: Tier::ReadConfig,
        desc: "RX-888: starts ka9q-radio radiod for the booted RX-888 on loopback (iface lo, ttl 0, no Avahi), niced to 10, with a config written to BENCH_MCP_OUT, and waits for the front end to answer. sample_rate_hz 64800000 (default, covers 0 to 30 MHz) or 129600000. Optional rf_gain_db (-8 to 34, the VGA in high-gain mode) and rf_atten_db (0 to 31.5 in 0.5 dB steps); omitted keeps the driver's defaults (RF AGC on). Refused if this server already runs one, if the RX-888 is not booted, or if loopback multicast is off (the error gives the one command a person must run).",
        props: props!(r#""instrument":{"type":"string"},"sample_rate_hz":{"type":"integer","description":"64800000 (default) or 129600000"},"rf_gain_db":{"type":"number","description":"VGA gain, -8 to 34 dB; omit for the driver default"},"rf_atten_db":{"type":"number","description":"attenuator, 0 to 31.5 dB in 0.5 dB steps; omit for 0"},"fft_threads":{"type":"integer","description":"1 to 8, default 2"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rx888_radiod_stop",
        tier: Tier::ReadConfig,
        desc: "RX-888: stops the radiod this server started (by its own recorded pid only, after checking the process is that radiod with that config): SIGTERM, then SIGKILL after 5 s. Returns the exit and the log's last lines.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rx888_tune",
        tier: Tier::ReadConfig,
        desc: "RX-888 through radiod: opens a receive channel at freq_hz (10 kHz to 30 MHz) in mode usb, lsb, cw, am (default) or fm, optionally with bandwidth_hz, lets it settle, then averages `readings` status reads (1 to 50, default 10, 100 ms apart): baseband power (dB re full scale, referred to the input, not dBm), noise density (dB/Hz), the passband, SNR estimate in dB, the channel's AGC gain and output level, and the input sample rate measured over the readings. The channel is removed afterwards. Receive only.",
        props: props!(r#""instrument":{"type":"string"},"freq_hz":{"type":"number","description":"carrier (dial) frequency, 10000 to 30000000 Hz"},"mode":{"type":"string","enum":["usb","lsb","cw","am","fm"]},"bandwidth_hz":{"type":"number","description":"passband width, 100 to 24000 Hz; omit for the mode's own"},"readings":{"type":"integer","description":"1 to 50, default 10"}"#),
        required: &["freq_hz"],
    },
    Spec {
        name: "bench_rx888_spectrum",
        tier: Tier::ReadConfig,
        desc: "RX-888 through radiod: one spectrum snapshot of `bins` bins (64 to 16000, default 1024) over span_hz around center_hz (defaults: the whole 0 to 30 MHz range). The span must be at most half the sample rate (32.4 MHz at 64.8 Msps: wider spans come back with half the bins zero). averaging 1 to 64 FFTs (default 10). Saves a CSV (freq_hz, level_db) in BENCH_MCP_OUT and returns the floor (median), min, max, the strongest peaks above the floor and the number of zero bins. Levels are dB re full scale per bin, referred to the input; not dBm.",
        props: props!(r#""instrument":{"type":"string"},"center_hz":{"type":"number","description":"centre, Hz; default 16200000"},"span_hz":{"type":"number","description":"span, Hz; default 32400000"},"bins":{"type":"integer","description":"64 to 16000, default 1024"},"averaging":{"type":"integer","description":"FFTs averaged, 1 to 64, default 10"},"peaks":{"type":"integer","description":"peaks to list, 0 to 50, default 10"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rx888_capture",
        tier: Tier::ReadConfig,
        desc: "RX-888 through radiod: records seconds (0.1 to 60, default 5) of a channel at freq_hz to a 16-bit WAV in BENCH_MCP_OUT. kind iq (default): two channels (I, Q), 12 kHz, +-5 kHz around freq_hz, at a fixed gain (gain_db -20 to 160, default chosen so the noise sits near -30 dBFS). kind audio: one channel, demodulated in mode (usb, lsb, cw, am default, fm), AGC on, 12 kHz (24 kHz for FM). Returns the file, samples, RTP packets, losses and the RMS level. The channel is removed afterwards.",
        props: props!(r#""instrument":{"type":"string"},"freq_hz":{"type":"number","description":"10000 to 30000000 Hz"},"kind":{"type":"string","enum":["iq","audio"]},"mode":{"type":"string","enum":["usb","lsb","cw","am","fm"]},"seconds":{"type":"number","description":"0.1 to 60, default 5"},"gain_db":{"type":"number","description":"iq only: fixed gain, -20 to 160 dB"}"#),
        required: &["freq_hz"],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: bench_mcp_rx888_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Rx888).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no rx888 instrument named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no rx888 instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    // [@ANCHOR: bench_mcp_rx888_tool_defs]
    SPECS
        .iter()
        .filter(|s| {
            insts
                .iter()
                .any(|i| i.kind == Kind::Rx888 && i.tier >= s.tier)
        })
        .filter_map(|s| {
            let props = serde_json::from_str::<Value>(s.props).ok()?;
            let Value::Object(schema) =
                json!({"type": "object", "properties": props, "required": s.required})
            else {
                return None;
            };
            Some(Tool::new(
                s.name,
                format!("[{:?}] {}", s.tier, s.desc),
                Arc::new(schema),
            ))
        })
        .collect()
}

/// A number argument: absent gives `default`; present must be finite and in `lo..=hi`.
pub(crate) fn number(
    args: &Value,
    key: &str,
    default: Option<f64>,
    lo: f64,
    hi: f64,
) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_rx888_number]
    match args.get(key) {
        None | Some(Value::Null) => default.ok_or_else(|| format!("{key} is required")),
        Some(v) => {
            let x = v
                .as_f64()
                .ok_or_else(|| format!("{key} must be a number"))?;
            if x.is_finite() && (lo..=hi).contains(&x) {
                Ok(x)
            } else {
                Err(format!("{key} {x} is outside {lo}..={hi}"))
            }
        }
    }
}

/// One RX-888 found in sysfs.
#[derive(Clone, Debug, PartialEq)]
pub struct UsbSeen {
    /// The sysfs device name (`2-3.2`).
    pub path: String,
    /// `00f3` (unbooted) or `00f1` (booted).
    pub product_id: String,
    /// Link speed in Mb/s as the kernel reports it (`480`, `5000`).
    pub speed_mbps: Option<u32>,
    /// The product string, when the device has one.
    pub product: Option<String>,
}

/// The RX-888s under `root` (production: [`USB_SYSFS`]): every device directory whose `idVendor` is
/// Cypress and whose `idProduct` is the FX3 loader or the booted RX-888, sorted by path.
pub fn find_usb(root: &Path) -> Vec<UsbSeen> {
    // [@ANCHOR: bench_mcp_rx888_find_usb]
    let read = |dir: &Path, f: &str| {
        std::fs::read_to_string(dir.join(f))
            .ok()
            .map(|s| s.trim().to_string())
    };
    let mut out: Vec<UsbSeen> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            if read(&dir, "idVendor").as_deref() != Some(VID) {
                return None;
            }
            let pid = read(&dir, "idProduct")?;
            if pid != PID_BOOTED && pid != PID_UNBOOTED {
                return None;
            }
            Some(UsbSeen {
                path: e.file_name().to_string_lossy().into_owned(),
                product_id: pid,
                speed_mbps: read(&dir, "speed")
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|s| s as u32),
                product: read(&dir, "product"),
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// What the USB scan says, as JSON.
fn usb_json(seen: &[UsbSeen]) -> Value {
    // [@ANCHOR: bench_mcp_rx888_usb_json]
    let state = match (
        seen.iter().any(|s| s.product_id == PID_BOOTED),
        seen.iter().any(|s| s.product_id == PID_UNBOOTED),
    ) {
        (true, _) => "booted",
        (false, true) => "unbooted (FX3 loader): call bench_rx888_boot",
        (false, false) => "not on the USB bus",
    };
    json!({
        "state": state,
        "devices": seen.iter().map(|s| json!({
            "sysfs": s.path, "usb_id": format!("{VID}:{}", s.product_id),
            "speed_mbps": s.speed_mbps, "product": s.product,
            "superspeed": s.speed_mbps.map(|v| v >= 5000),
        })).collect::<Vec<_>>(),
    })
}

/// A `radiod` this server started.
struct Running {
    child: Child,
    config: PathBuf,
    log: PathBuf,
    started: Instant,
}

/// The `radiod`s started by this process, by instrument name.
fn running() -> &'static Mutex<HashMap<String, Running>> {
    // [@ANCHOR: bench_mcp_rx888_running]
    static RUNNING: std::sync::OnceLock<Mutex<HashMap<String, Running>>> =
        std::sync::OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The pid of the `radiod` this server runs for `name`, reaping one that has exited.
fn running_pid(name: &str) -> Option<u32> {
    // [@ANCHOR: bench_mcp_rx888_running_pid]
    let mut map = running().lock().unwrap_or_else(|p| p.into_inner());
    let alive = map
        .get_mut(name)
        .map(|r| matches!(r.child.try_wait(), Ok(None)));
    match alive {
        Some(true) => map.get(name).map(|r| r.child.id()),
        Some(false) => {
            map.remove(name);
            None
        }
        None => None,
    }
}

/// The last `n` lines of a file, for error messages.
fn tail(path: &Path, n: usize) -> Vec<String> {
    // [@ANCHOR: bench_mcp_rx888_tail]
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// True when the loopback interface has multicast on (`IFF_MULTICAST`, 0x1000, in sysfs `flags`).
pub fn loopback_multicast(sys_class_net: &Path) -> bool {
    // [@ANCHOR: bench_mcp_rx888_loopback_multicast]
    std::fs::read_to_string(sys_class_net.join("lo").join("flags"))
        .ok()
        .and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .is_some_and(|f| f & 0x1000 != 0)
}

/// The `radiod` configuration text for the RX-888: loopback, TTL 0, no Avahi, the driver by path.
pub fn radiod_config_text(
    cfg: &Config,
    description: &str,
    sample_rate_hz: u64,
    rf_gain_db: Option<f64>,
    rf_atten_db: Option<f64>,
    fft_threads: u32,
) -> String {
    // [@ANCHOR: bench_mcp_rx888_radiod_config_text]
    let mut s = format!(
        "# Written by bench_mcp (bench_rx888_radiod_start); receive only, loopback only.\n\
         [global]\nhardware = rx888\nstatus = {}\ndata = {}\niface = lo\nttl = 0\nmode = usb\n\
         advertise = no\npresets-file = {}\nfft-threads = {fft_threads}\nverbose = 1\n\n\
         [rx888]\ndevice = \"rx888\"\nlibrary = {}\ndescription = \"{}\"\nsamprate = {sample_rate_hz}\n",
        cfg.status_group,
        cfg.data_group,
        cfg.presets.display(),
        cfg.driver_library.display(),
        description.replace(['"', '\n'], " "),
    );
    if let Some(g) = rf_gain_db {
        s.push_str(&format!("gain = {g}\n"));
    }
    if let Some(a) = rf_atten_db {
        s.push_str(&format!("att = {a}\n"));
    }
    s
}

/// A current-thread runtime for one tool call (calls run on blocking threads, outside any runtime).
pub(crate) fn runtime() -> Result<tokio::runtime::Runtime, String> {
    // [@ANCHOR: bench_mcp_rx888_runtime]
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start a runtime: {e}"))
}

/// Opens the control socket to `radiod`'s status group on loopback.
pub(crate) fn control(cfg: &Config, rt: &tokio::runtime::Runtime) -> Result<Control, String> {
    // [@ANCHOR: bench_mcp_rx888_control]
    Control::open_on_port(
        rt.handle(),
        cfg.status_group,
        cfg.status_port,
        Ipv4Addr::LOCALHOST,
        radiod_client::command::Policy::default(),
    )
    .map_err(|e| {
        format!(
            "cannot join radiod's status group {}:{} on loopback ({e}). Next: is loopback multicast on (bench_rx888_status)?",
            cfg.status_group, cfg.status_port
        )
    })
}

/// The front end's status, or an error that says radiod is not answering.
pub(crate) fn front_end(
    cfg: &Config,
    rt: &tokio::runtime::Runtime,
    wait: Duration,
) -> Result<ChannelStatus, String> {
    // [@ANCHOR: bench_mcp_rx888_front_end]
    let mut c = control(cfg, rt)?;
    rt.block_on(c.front_end(wait))
        .map_err(|e| format!("radiod front-end read failed: {e}"))?
        .ok_or_else(|| {
            format!(
                "radiod does not answer on {}:{} within {:?}. Next: bench_rx888_status, then bench_rx888_radiod_start.",
                cfg.status_group, cfg.status_port, wait
            )
        })
}

/// The front end as JSON (finite numbers only; `-inf` IF power is reported as idle).
pub(crate) fn front_end_json(fe: &ChannelStatus) -> Value {
    // [@ANCHOR: bench_mcp_rx888_front_end_json]
    let finite = |v: Option<f32>| v.filter(|x| x.is_finite()).map(f64::from);
    json!({
        "description": fe.description,
        "input_sample_rate_hz": fe.input_samprate,
        "real_samples": fe.fe_is_real,
        "ad_bits": fe.ad_bits,
        "ad_overrange_count": fe.ad_over,
        "samples_since_overrange": fe.samples_since_over,
        "rf_gain_db": finite(fe.rf_gain_db),
        "rf_atten_db": finite(fe.rf_atten_db),
        "rf_agc": fe.rf_agc,
        "if_power_dbfs": finite(fe.if_power_db),
        "front_end_running": fe.if_power_db.is_some_and(|p| p.is_finite()),
        "rf_level_cal_db": finite(fe.rf_level_cal_db),
        "usable_low_hz": finite(fe.fe_low_edge_hz),
        "usable_high_hz": finite(fe.fe_high_edge_hz),
    })
}

pub fn status(inst: &Instrument, usb_root: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_status]
    let cfg = &inst.rx888;
    let seen = find_usb(usb_root);
    let pid = running_pid(&inst.name);
    let radiod = match pid {
        Some(pid) => {
            let map = running().lock().unwrap_or_else(|p| p.into_inner());
            let r = map.get(&inst.name);
            json!({"running": true, "pid": pid,
                "config": r.map(|r| r.config.display().to_string()),
                "log": r.map(|r| r.log.display().to_string()),
                "uptime_s": r.map(|r| r.started.elapsed().as_secs())})
        }
        None => {
            json!({"running": false, "note": "no radiod started by this server; another one may still answer"})
        }
    };
    let rt = runtime()?;
    let fe = match front_end(cfg, &rt, Duration::from_millis(800)) {
        Ok(fe) => front_end_json(&fe),
        Err(e) => json!({"answering": false, "why": e}),
    };
    Ok(json!({
        "usb": usb_json(&seen),
        "radiod": radiod,
        "front_end": fe,
        "loopback_multicast": loopback_multicast(Path::new("/sys/class/net")),
        "groups": {"status": format!("{}:{}", cfg.status_group, cfg.status_port),
                   "data": format!("{}:{}", cfg.data_group, cfg.data_port)},
    }))
}

pub fn frontend(inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_frontend]
    let rt = runtime()?;
    let fe = front_end(&inst.rx888, &rt, Duration::from_secs(2))?;
    Ok(front_end_json(&fe))
}

pub fn boot(inst: &Instrument, usb_root: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_boot]
    let cfg = &inst.rx888;
    if running_pid(&inst.name).is_some() {
        return Err(
            "radiod is running for this RX-888; stop it first (bench_rx888_radiod_stop)".into(),
        );
    }
    let seen = find_usb(usb_root);
    if seen.iter().any(|s| s.product_id == PID_BOOTED) {
        return Ok(json!({"already_booted": true, "usb": usb_json(&seen)}));
    }
    if !seen.iter().any(|s| s.product_id == PID_UNBOOTED) {
        return Err("no RX-888 on the USB bus (neither 04b4:00f3 nor 04b4:00f1). Next: check the cable; if it is on a switchable hub port, bench_usb_port_power cycle it.".into());
    }
    if !cfg.firmware.is_file() {
        return Err(format!("the firmware {} is not a file. Next: set \"firmware\" in the bench config to ka9q-radio's share/W1EUJ_0.1.0_FX3.img", cfg.firmware.display()));
    }
    let out = std::process::Command::new(&cfg.boot)
        .arg(&cfg.firmware)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", cfg.boot.display()))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let deadline = Instant::now() + BOOT_WAIT;
    let mut now_seen = find_usb(usb_root);
    while !now_seen.iter().any(|s| s.product_id == PID_BOOTED) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
        now_seen = find_usb(usb_root);
    }
    let booted = now_seen.iter().find(|s| s.product_id == PID_BOOTED);
    match booted {
        Some(b) => Ok(json!({
            "booted": true,
            "sysfs": b.path,
            "speed_mbps": b.speed_mbps,
            "superspeed": b.speed_mbps.map(|v| v >= 5000),
            "note": if b.speed_mbps.is_some_and(|v| v >= 5000) { "USB 3 link: the full rate is possible" } else { "not a USB 3 link: run radiod at a rate USB 2 can carry, or move the cable to a SuperSpeed port" },
            "loader_output": text.lines().map(str::to_string).collect::<Vec<_>>(),
        })),
        None => Err(format!(
            "the RX-888 did not come back booted within {:?}. Loader said: {:?}. Next: power-cycle its hub port (bench_usb_port_power, off 5 s, on) and call bench_rx888_boot once more; do not unbind the USB controller.",
            BOOT_WAIT,
            text.trim()
        )),
    }
}

pub fn radiod_start(inst: &Instrument, args: &Value, usb_root: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_radiod_start]
    let cfg = &inst.rx888;
    if let Some(pid) = running_pid(&inst.name) {
        return Err(format!("radiod already runs for {} (pid {pid})", inst.name));
    }
    let dir = crate::out_dir();
    let conf = dir.join(format!("rx888-{}.conf", inst.name));
    if let Some(pid) = std::fs::read_to_string(dir.join(format!("rx888-{}.pid", inst.name)))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|pid| is_our_radiod(*pid, &cfg.radiod, &conf))
    {
        return Err(format!(
            "a radiod started earlier for {} still runs (pid {pid}, started by a previous server); stop it with bench_rx888_radiod_stop",
            inst.name
        ));
    }
    let rate = match args.get("sample_rate_hz") {
        None | Some(Value::Null) => SAMPLE_RATES_HZ[0],
        Some(v) => v
            .as_u64()
            .filter(|r| SAMPLE_RATES_HZ.contains(r))
            .ok_or("sample_rate_hz must be 64800000 or 129600000")?,
    };
    let gain = match args.get("rf_gain_db") {
        None | Some(Value::Null) => None,
        Some(_) => Some(number(args, "rf_gain_db", None, -8.0, 34.0)?),
    };
    let atten = match args.get("rf_atten_db") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let a = number(args, "rf_atten_db", None, 0.0, 31.5)?;
            if (a * 2.0).fract() != 0.0 {
                return Err(format!("rf_atten_db {a} is not a 0.5 dB step"));
            }
            Some(a)
        }
    };
    let threads = number(args, "fft_threads", Some(2.0), 1.0, 8.0)? as u32;
    let seen = find_usb(usb_root);
    if !seen.iter().any(|s| s.product_id == PID_BOOTED) {
        return Err(format!(
            "the RX-888 is not booted ({}). Next: bench_rx888_boot.",
            usb_json(&seen)["state"]
        ));
    }
    if !loopback_multicast(Path::new("/sys/class/net")) {
        return Err("multicast is off on the loopback interface, and radiod runs on loopback only. A person must run: sudo ip link set dev lo multicast on (it lasts until reboot).".into());
    }
    for (what, p) in [
        ("radiod", &cfg.radiod),
        ("driver_library", &cfg.driver_library),
        ("presets", &cfg.presets),
    ] {
        if !p.is_file() {
            return Err(format!(
                "{what} {} is not a file; set it in the bench config",
                p.display()
            ));
        }
    }
    let log = dir.join(format!("rx888-{}.log", inst.name));
    let text = radiod_config_text(
        cfg,
        &format!("bench_mcp {}", inst.name),
        rate,
        gain,
        atten,
        threads,
    );
    std::fs::write(&conf, text).map_err(|e| format!("cannot write {}: {e}", conf.display()))?;
    let logf =
        std::fs::File::create(&log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
    let logf2 = logf.try_clone().map_err(|e| e.to_string())?;
    let child = std::process::Command::new("nice")
        .args(["-n", "10"])
        .arg(&cfg.radiod)
        .arg("-v")
        .arg(&conf)
        .stdin(std::process::Stdio::null())
        .stdout(logf)
        .stderr(logf2)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", cfg.radiod.display()))?;
    let pid = child.id();
    std::fs::write(
        dir.join(format!("rx888-{}.pid", inst.name)),
        format!("{pid}\n"),
    )
    .map_err(|e| e.to_string())?;
    running().lock().unwrap_or_else(|p| p.into_inner()).insert(
        inst.name.clone(),
        Running {
            child,
            config: conf.clone(),
            log: log.clone(),
            started: Instant::now(),
        },
    );
    let rt = runtime()?;
    let deadline = Instant::now() + START_WAIT;
    loop {
        if running_pid(&inst.name).is_none() {
            return Err(format!("radiod exited at start. Log: {:?}", tail(&log, 15)));
        }
        if let Ok(fe) = front_end(cfg, &rt, Duration::from_millis(500)) {
            return Ok(
                json!({"started": true, "pid": pid, "config": conf.display().to_string(),
                "log": log.display().to_string(), "front_end": front_end_json(&fe)}),
            );
        }
        if Instant::now() > deadline {
            let _ = stop_pid(inst);
            return Err(format!(
                "radiod did not answer within {START_WAIT:?}; stopped it. Log: {:?}",
                tail(&log, 15)
            ));
        }
    }
}

/// True when `/proc/<pid>/cmdline` is `radiod` with this config (so a stale pid file never names
/// another process).
fn is_our_radiod(pid: u32, radiod: &Path, config: &Path) -> bool {
    // [@ANCHOR: bench_mcp_rx888_is_our_radiod]
    let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args: Vec<String> = raw
        .split(|b| *b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let radiod = radiod.display().to_string();
    let config = config.display().to_string();
    args.first().is_some_and(|a| *a == radiod) && args.contains(&config)
}

/// Stops the `radiod` started for `inst`: the child this process holds, or else the pid in its pid file
/// when `/proc` shows that pid is `radiod` with this server's config for it (a restarted server can
/// stop what it started, and a stale pid file never names another process). SIGTERM, then SIGKILL
/// after 5 s. Never by name or pattern.
fn stop_pid(inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_stop_pid]
    let name = &inst.name;
    let dir = crate::out_dir();
    let pid_file = dir.join(format!("rx888-{name}.pid"));
    let conf = dir.join(format!("rx888-{name}.conf"));
    let held = running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(name);
    let (pid, log, mut child) = match held {
        Some(r) => (r.child.id(), r.log, Some(r.child)),
        None => {
            let pid = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .ok_or("no radiod was started by this server")?;
            if !is_our_radiod(pid, &inst.rx888.radiod, &conf) {
                let _ = std::fs::remove_file(&pid_file);
                return Err(format!(
                    "pid {pid} in {} is not radiod with {}; nothing was stopped and the stale pid file was removed",
                    pid_file.display(),
                    conf.display()
                ));
            }
            (pid, dir.join(format!("rx888-{name}.log")), None)
        }
    };
    let pid_t = libc::pid_t::try_from(pid).map_err(|_| format!("pid {pid} out of range"))?;
    // SAFETY: kill(2) with a pid this server started (held child) or verified through /proc above.
    let signal = |sig: libc::c_int| unsafe { libc::kill(pid_t, sig) };
    let gone = |child: &mut Option<Child>| match child.as_mut() {
        Some(c) => c.try_wait().ok().flatten().map(|s| s.to_string()),
        None => (!Path::new(&format!("/proc/{pid}")).exists()).then(|| "exited".to_string()),
    };
    signal(libc::SIGTERM);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut exit = gone(&mut child);
    while exit.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        exit = gone(&mut child);
    }
    let mut killed = false;
    if exit.is_none() {
        signal(libc::SIGKILL);
        killed = true;
        exit = match child.as_mut() {
            Some(c) => c.wait().ok().map(|s| s.to_string()),
            None => Some("killed".to_string()),
        };
    }
    let _ = std::fs::remove_file(&pid_file);
    Ok(
        json!({"stopped": true, "pid": pid, "exit": exit, "sigkill_needed": killed, "log_tail": tail(&log, 5)}),
    )
}

pub fn radiod_stop(inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_radiod_stop]
    stop_pid(inst)
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_call]
    call_with(Path::new(USB_SYSFS), insts, name, args)
}

/// `call` with the sysfs USB root given (production: [`USB_SYSFS`]).
pub fn call_with(
    usb_root: &Path,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_rx888_call_with]
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
        "bench_rx888_status" => status(inst, usb_root),
        "bench_rx888_frontend" => frontend(inst),
        "bench_rx888_boot" => boot(inst, usb_root),
        "bench_rx888_radiod_start" => radiod_start(inst, args, usb_root),
        "bench_rx888_radiod_stop" => radiod_stop(inst),
        "bench_rx888_tune" => radio::tune(inst, args),
        "bench_rx888_spectrum" => radio::spectrum(inst, args),
        "bench_rx888_capture" => radio::capture(inst, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    fn bench(entry: &str) -> Vec<Instrument> {
        crate::config::parse(&format!(r#"{{"instruments":{{"rx888":{{{entry}}}}}}}"#)).unwrap()
    }

    /// A sysfs USB tree with the given (name, vid, pid, speed) devices.
    fn sysfs(devs: &[(&str, &str, &str, &str)]) -> tempdir::Dir {
        let d = tempdir::Dir::new();
        for (name, vid, pid, speed) in devs {
            let p = d.0.join(name);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("idVendor"), format!("{vid}\n")).unwrap();
            std::fs::write(p.join("idProduct"), format!("{pid}\n")).unwrap();
            std::fs::write(p.join("speed"), format!("{speed}\n")).unwrap();
            std::fs::write(p.join("product"), "RX888mk2\n").unwrap();
        }
        d
    }

    mod tempdir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Dir {
                static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let p = crate::testdir::scratch(&format!("bench_rx888__{n}"));
                Dir(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn json_of(v: Result<Value, String>) -> Value {
        v.unwrap_or_else(|e| panic!("tool failed: {e}"))
    }

    // Tests [@ANCHOR: bench_mcp_rx888_config]
    #[test]
    fn config_has_install_defaults_and_refuses_relative_paths_and_unicast_groups() {
        let c = parse_config(&json!({})).unwrap();
        assert_eq!(c, Config::default());
        assert!(c.radiod.is_absolute() && c.firmware.ends_with("W1EUJ_0.1.0_FX3.img"));
        let c = parse_config(&json!({"radiod": "/opt/k/radiod", "status_group": "239.1.2.3", "data_port": 6004, "ssrc_base": 5})).unwrap();
        assert_eq!(
            (c.radiod, c.status_group, c.data_port, c.ssrc_base),
            (
                PathBuf::from("/opt/k/radiod"),
                Ipv4Addr::new(239, 1, 2, 3),
                6004,
                5
            )
        );
        for bad in [
            json!({"firmware": "W1EUJ_0.1.0_FX3.img"}),
            json!({"radiod": 7}),
            json!({"status_group": "192.0.2.1"}),
            json!({"data_group": "hf.local"}),
            json!({"status_port": 0}),
            json!({"data_port": 70000}),
            json!({"ssrc_base": 0}),
            json!({"status_group": "239.9.9.9", "data_group": "239.9.9.9", "status_port": 5006, "data_port": 5006}),
        ] {
            assert!(parse_config(&bad).is_err(), "{bad}");
        }
        assert!(crate::config::parse(
            r#"{"instruments":{"r":{"driver":"rx888","firmware":"rel.img"}}}"#
        )
        .is_err());
    }

    // Tests [@ANCHOR: bench_mcp_rx888_find_usb]
    // Tests [@ANCHOR: bench_mcp_rx888_usb_json]
    #[test]
    fn usb_scan_finds_the_loader_and_the_booted_device_only() {
        let d = sysfs(&[
            ("3-2.2", "04b4", "00f3", "480"),
            ("1-1", "046d", "c52b", "12"),
            ("1-2", "04b4", "1234", "12"),
        ]);
        let seen = find_usb(&d.0);
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (
                seen[0].path.as_str(),
                seen[0].product_id.as_str(),
                seen[0].speed_mbps
            ),
            ("3-2.2", "00f3", Some(480))
        );
        assert!(usb_json(&seen)["state"]
            .as_str()
            .unwrap()
            .starts_with("unbooted"));
        let d = sysfs(&[("2-3.2", "04b4", "00f1", "5000")]);
        let j = usb_json(&find_usb(&d.0));
        assert_eq!(
            (j["state"].as_str(), j["devices"][0]["superspeed"].as_bool()),
            (Some("booted"), Some(true))
        );
        assert_eq!(
            usb_json(&find_usb(Path::new("/nonexistent")))["state"],
            "not on the USB bus"
        );
    }

    // Tests [@ANCHOR: bench_mcp_rx888_status]
    // Tests [@ANCHOR: bench_mcp_rx888_frontend]
    // Tests [@ANCHOR: bench_mcp_rx888_front_end]
    // Tests [@ANCHOR: bench_mcp_rx888_front_end_json]
    // Tests [@ANCHOR: bench_mcp_rx888_control]
    // Tests [@ANCHOR: bench_mcp_rx888_runtime]
    // Tests [@ANCHOR: bench_mcp_rx888_call]
    // Tests [@ANCHOR: bench_mcp_rx888_call_with]
    // Tests [@ANCHOR: bench_mcp_rx888_instrument]
    // Tests [@ANCHOR: bench_mcp_rx888_running_pid]
    // Tests [@ANCHOR: bench_mcp_rx888_running]
    // Tests [@ANCHOR: bench_mcp_rx888_loopback_multicast]
    // Tests [@ANCHOR: bench_mcp_rx888_tool_defs]
    #[test]
    fn status_and_frontend_read_the_usb_tree_and_the_front_end() {
        let fake = FakeRadiod::start();
        let insts = bench(&fake.entry("read_only"));
        let d = sysfs(&[("2-3.2", "04b4", "00f1", "5000")]);
        let s = json_of(call_with(&d.0, &insts, "bench_rx888_status", &json!({})));
        assert_eq!(s["usb"]["state"], "booted");
        assert_eq!(s["radiod"]["running"], false);
        assert_eq!(s["front_end"]["input_sample_rate_hz"], 64_800_000);
        let fe = json_of(call(&insts, "bench_rx888_frontend", &json!({})));
        assert_eq!(
            (
                fe["ad_overrange_count"].as_u64(),
                fe["rf_gain_db"].as_f64(),
                fe["rf_agc"].as_bool()
            ),
            (Some(3), Some(34.0), Some(true))
        );
        assert_eq!(
            (
                fe["rf_atten_db"].as_f64(),
                fe["front_end_running"].as_bool()
            ),
            (Some(0.0), Some(true))
        );
        assert!(
            fake.seen
                .lock()
                .unwrap()
                .iter()
                .all(|s| s.ssrc == 0 && s.lifetime.is_none()),
            "front-end polls only, no channel"
        );
        // A read_only instrument cannot tune, and an unknown name is refused.
        assert!(call(&insts, "bench_rx888_tune", &json!({"freq_hz": 1e7}))
            .unwrap_err()
            .contains("needs tier"));
        assert!(call(
            &insts,
            "bench_rx888_status",
            &json!({"instrument": "other"})
        )
        .is_err());
        let flags = tempdir::Dir::new();
        std::fs::create_dir_all(flags.0.join("lo")).unwrap();
        std::fs::write(flags.0.join("lo/flags"), "0x9\n").unwrap();
        assert!(!loopback_multicast(&flags.0));
        std::fs::write(flags.0.join("lo/flags"), "0x1009\n").unwrap();
        assert!(loopback_multicast(&flags.0));
        assert!(running_pid("nobody").is_none());
        let listed =
            |tier: &str| tool_defs(&bench(&format!(r#""driver":"rx888","tier":"{tier}""#))).len();
        assert_eq!(
            (listed("read_only"), listed("read_config"), listed("full")),
            (2, SPECS.len(), SPECS.len())
        );
    }

    // Tests [@ANCHOR: bench_mcp_rx888_tune]
    // Tests [@ANCHOR: bench_mcp_rx888_number]
    // Tests [@ANCHOR: bench_mcp_rx888_ask]
    // Tests [@ANCHOR: bench_mcp_rx888_remove]
    #[test]
    fn tune_reports_levels_snr_and_rate_and_removes_its_channel() {
        let fake = FakeRadiod::start();
        let insts = bench(&fake.entry("read_config"));
        let t = json_of(call(
            &insts,
            "bench_rx888_tune",
            &json!({"freq_hz": 7_074_000, "mode": "usb", "bandwidth_hz": 2400, "readings": 6}),
        ));
        assert_eq!(t["frequency_hz"].as_f64(), Some(7_074_000.0));
        assert_eq!(t["passband_hz"], json!([50.0, 2450.0]));
        assert!((t["baseband_power_db"].as_f64().unwrap() - f64::from(BASEBAND_DB)).abs() < 1e-3);
        let n = t["noise_in_passband_db"].as_f64().unwrap();
        assert!((n - (f64::from(NOISE_DENSITY_DB) + 10.0 * 2400f64.log10())).abs() < 1e-3);
        assert!(t["snr_db"].as_f64().is_some());
        let rate = t["input_sample_rate_measured_sps"].as_f64().unwrap();
        let res = t["rate_resolution_fraction"].as_f64().unwrap();
        assert!((rate / 64.8e6 - 1.0).abs() <= 1.5 * res, "{rate} {res}");
        let seen = fake.seen.lock().unwrap().clone();
        let ssrc = Config::default().ssrc_base;
        assert!(
            seen.iter()
                .filter(|s| s.ssrc == ssrc)
                .all(|s| s.lifetime.is_some_and(|l| l >= 1)),
            "{seen:?}"
        );
        assert_eq!(
            seen.last().map(|s| (s.ssrc, s.lifetime)),
            Some((ssrc, Some(1))),
            "removed at the end"
        );
        for bad in [
            json!({}),
            json!({"freq_hz": 5}),
            json!({"freq_hz": 1e7, "mode": "rtty"}),
            json!({"freq_hz": 1e7, "readings": 0}),
            json!({"freq_hz": 1e7, "bandwidth_hz": 50000}),
        ] {
            assert!(call(&insts, "bench_rx888_tune", &bad).is_err(), "{bad}");
        }
        assert_eq!(number(&json!({}), "x", Some(2.0), 0.0, 3.0), Ok(2.0));
        assert!(number(&json!({"x": f64::NAN}), "x", None, 0.0, 3.0).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_rx888_spectrum]
    #[test]
    fn spectrum_writes_a_csv_finds_the_tone_and_refuses_a_span_wider_than_half_the_rate() {
        let fake = FakeRadiod::start();
        let insts = bench(&fake.entry("read_config"));
        let s = json_of(call(
            &insts,
            "bench_rx888_spectrum",
            &json!({"center_hz": 10_000_000, "span_hz": 2_048_000, "bins": 1024}),
        ));
        assert_eq!(
            (s["bins"].as_u64(), s["zero_bins"].as_u64()),
            (Some(1024), Some(0))
        );
        assert!((s["floor_db"].as_f64().unwrap() - f64::from(FLOOR_DB)).abs() < 0.01);
        let p = &s["peaks"][0];
        assert!(
            (p["freq_hz"].as_f64().unwrap() - 11_000_000.0).abs() < 2_000.0,
            "{p}"
        );
        let csv = std::fs::read_to_string(s["csv"].as_str().unwrap()).unwrap();
        assert_eq!(
            csv.lines().filter(|l| !l.starts_with('#')).count(),
            1 + 1024
        );
        let err = call(
            &insts,
            "bench_rx888_spectrum",
            &json!({"span_hz": 32_768_000}),
        )
        .unwrap_err();
        assert!(err.contains("wider than the front end can fill"), "{err}");
        assert!(call(&insts, "bench_rx888_spectrum", &json!({"bins": 20_000})).is_err());
        let _ = std::fs::remove_file(s["csv"].as_str().unwrap());
    }

    // Tests [@ANCHOR: bench_mcp_rx888_capture]
    #[test]
    fn capture_writes_iq_and_audio_wavs() {
        let fake = FakeRadiod::start();
        let insts = bench(&fake.entry("read_config"));
        let iq = json_of(call(
            &insts,
            "bench_rx888_capture",
            &json!({"freq_hz": 10_000_000, "seconds": 0.5}),
        ));
        assert_eq!(
            (
                iq["channels"].as_u64(),
                iq["sample_rate_hz"].as_u64(),
                iq["frames"].as_u64()
            ),
            (Some(2), Some(12_000), Some(6000))
        );
        assert!(
            (iq["gain_db"].as_f64().unwrap() - (-30.0 - f64::from(BASEBAND_DB))).abs() < 0.01,
            "{iq}"
        );
        let wav = std::fs::read(iq["wav"].as_str().unwrap()).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 6000 * 2 * 2);
        let au = json_of(call(
            &insts,
            "bench_rx888_capture",
            &json!({"freq_hz": 10_000_000, "kind": "audio", "mode": "am", "seconds": 0.3}),
        ));
        assert_eq!(
            (
                au["channels"].as_u64(),
                au["frames"].as_u64(),
                au["gain_db"].is_null()
            ),
            (Some(1), Some(3600), true)
        );
        assert!(call(
            &insts,
            "bench_rx888_capture",
            &json!({"freq_hz": 1e7, "kind": "audio", "gain_db": 50})
        )
        .is_err());
        assert!(call(
            &insts,
            "bench_rx888_capture",
            &json!({"freq_hz": 1e7, "seconds": 61})
        )
        .is_err());
        for f in [&iq, &au] {
            let _ = std::fs::remove_file(f["wav"].as_str().unwrap());
        }
        let seen = fake.seen.lock().unwrap();
        assert!(seen.iter().any(|s| s.lifetime == Some(1)), "removed");
        assert!(seen
            .iter()
            .filter(|s| s.ssrc != 0)
            .all(|s| s.lifetime.is_some()));
    }

    // Tests [@ANCHOR: bench_mcp_rx888_boot]
    #[test]
    fn boot_runs_nothing_when_the_device_is_absent_or_already_booted() {
        let insts =
            bench(r#""driver":"rx888","tier":"read_config","boot":"/nonexistent/rx888_boot""#);
        let none = sysfs(&[]);
        assert!(call_with(&none.0, &insts, "bench_rx888_boot", &json!({}))
            .unwrap_err()
            .contains("no RX-888"));
        let booted = sysfs(&[("2-3.2", "04b4", "00f1", "5000")]);
        assert_eq!(
            json_of(call_with(&booted.0, &insts, "bench_rx888_boot", &json!({})))["already_booted"],
            true
        );
        let unbooted = sysfs(&[("3-2.2", "04b4", "00f3", "480")]);
        let missing =
            bench(r#""driver":"rx888","tier":"read_config","firmware":"/nonexistent/fw.img""#);
        assert!(
            call_with(&unbooted.0, &missing, "bench_rx888_boot", &json!({}))
                .unwrap_err()
                .contains("not a file")
        );
    }

    // Tests [@ANCHOR: bench_mcp_rx888_radiod_start]
    // Tests [@ANCHOR: bench_mcp_rx888_radiod_config_text]
    #[test]
    fn radiod_start_checks_its_arguments_and_the_device_and_writes_a_loopback_config() {
        let insts = bench(r#""driver":"rx888","tier":"read_config""#);
        let unbooted = sysfs(&[("3-2.2", "04b4", "00f3", "480")]);
        assert!(
            call_with(&unbooted.0, &insts, "bench_rx888_radiod_start", &json!({}))
                .unwrap_err()
                .contains("not booted")
        );
        for bad in [
            json!({"sample_rate_hz": 100}),
            json!({"rf_gain_db": 40}),
            json!({"rf_atten_db": 1.25}),
            json!({"rf_atten_db": 40}),
            json!({"fft_threads": 0}),
        ] {
            assert!(
                call_with(&unbooted.0, &insts, "bench_rx888_radiod_start", &bad).is_err(),
                "{bad}"
            );
        }
        let text = radiod_config_text(
            &Config::default(),
            "x\"y",
            64_800_000,
            Some(20.0),
            Some(10.5),
            2,
        );
        for want in [
            "hardware = rx888",
            "iface = lo",
            "ttl = 0",
            "advertise = no",
            "status = 239.88.8.1",
            "data = 239.88.8.2",
            "library = /usr/local/lib/ka9q-radio/rx888.so",
            "samprate = 64800000",
            "gain = 20",
            "att = 10.5",
            "fft-threads = 2",
            "description = \"x y\"",
        ] {
            assert!(text.contains(want), "{want} missing from\n{text}");
        }
        assert!(
            !radiod_config_text(&Config::default(), "d", 64_800_000, None, None, 2)
                .contains("gain =")
        );
    }

    // Tests [@ANCHOR: bench_mcp_rx888_is_our_radiod]
    // Tests [@ANCHOR: bench_mcp_rx888_stop_pid]
    // Tests [@ANCHOR: bench_mcp_rx888_radiod_stop]
    // Tests [@ANCHOR: bench_mcp_rx888_tail]
    #[test]
    fn stop_uses_only_a_pid_file_that_names_this_radiod_with_this_config() {
        let name = format!("stoptest{}", std::process::id());
        let dir = crate::out_dir();
        let conf = dir.join(format!("rx888-{name}.conf"));
        let pid_file = dir.join(format!("rx888-{name}.pid"));
        std::fs::write(dir.join(format!("rx888-{name}.log")), "a\nb\nlast line\n").unwrap();
        // A stand-in "radiod": /bin/sh whose argv carries the config path ($0), like radiod -v <config>.
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 5; :"])
            .arg(&conf)
            .spawn()
            .unwrap();
        let pid = child.id();
        // Right after spawn the child may not have exec'd yet; its cmdline is then the parent's.
        let exec_deadline = Instant::now() + Duration::from_secs(3);
        while !is_our_radiod(pid, Path::new("/bin/sh"), &conf) && Instant::now() < exec_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(is_our_radiod(pid, Path::new("/bin/sh"), &conf));
        assert!(!is_our_radiod(
            pid,
            Path::new("/bin/sh"),
            Path::new("/other.conf")
        ));
        assert!(!is_our_radiod(
            pid,
            Path::new("/usr/local/sbin/radiod"),
            &conf
        ));
        let entry = |radiod: &str| {
            crate::config::parse(&format!(r#"{{"instruments":{{"{name}":{{"driver":"rx888","tier":"read_config","radiod":"{radiod}"}}}}}}"#)).unwrap()
        };
        // A pid file that does not name this radiod is removed and nothing is signalled.
        std::fs::write(&pid_file, format!("{pid}\n")).unwrap();
        let err = call(
            &entry("/usr/local/sbin/radiod"),
            "bench_rx888_radiod_stop",
            &json!({}),
        )
        .unwrap_err();
        assert!(
            err.contains("nothing was stopped") && !pid_file.exists(),
            "{err}"
        );
        assert!(matches!(child.try_wait(), Ok(None)), "still running");
        // The right one is stopped.
        std::fs::write(&pid_file, format!("{pid}\n")).unwrap();
        let ok = json_of(call(
            &entry("/bin/sh"),
            "bench_rx888_radiod_stop",
            &json!({}),
        ));
        assert_eq!(
            (ok["stopped"].as_bool(), ok["pid"].as_u64()),
            (Some(true), Some(u64::from(pid)))
        );
        assert_eq!(
            ok["log_tail"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|v| v.as_str()),
            Some("last line")
        );
        let _ = child.wait();
        assert!(!pid_file.exists());
        assert!(
            call(&entry("/bin/sh"), "bench_rx888_radiod_stop", &json!({}))
                .unwrap_err()
                .contains("no radiod")
        );
        assert_eq!(tail(Path::new("/nonexistent"), 3), Vec::<String>::new());
    }
}

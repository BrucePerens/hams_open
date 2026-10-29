// SPDX-License-Identifier: AGPL-3.0-or-later
//! bench_mcp: one MCP server (rmcp, stdio) for the bench test equipment: a Siglent SDS1000X-E
//! oscilloscope over the LAN and an Abestop ATM6301 power supply on a USB-serial port. There is no
//! raw-SCPI tool. The instruments are listed in the JSON file named by `BENCH_MCP_CONFIG`, each with
//! its own permission tier (`read_only`, `read_config`, `full`); the server, not the model, refuses a
//! tool above an instrument's tier and does not list tools no instrument may use. `BENCH_MCP_OUT`
//! is the directory for captures and screenshots.

mod audio;
#[cfg(test)]
mod checklist;
mod config;
mod dsp;
mod example;
mod hamlib;
mod link;
mod manual;
mod psu;
mod rotator;
mod scope;
mod sdr;
mod spectrum;
mod usb_hub;
mod vna;
mod vna_tools;

use config::{Instrument, Kind, Tier};
use link::Link;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::stdio;
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct ToolSpec {
    name: &'static str,
    tier: Tier,
    /// Instrument kinds the tool applies to.
    kinds: &'static [Kind],
    desc: &'static str,
    props: &'static str,
    required: &'static [&'static str],
}

const ANY: &[Kind] = &[Kind::Scope, Kind::Psu];
const WITH_VNA: &[Kind] = &[Kind::Scope, Kind::Psu, Kind::Vna, Kind::Sdr];

const TOOLS: &[ToolSpec] = &[
    ToolSpec { name: "bench_list", tier: Tier::ReadOnly, kinds: ANY, desc: "List the configured instruments with their driver and permission tier.", props: "{}", required: &[] },
    ToolSpec { name: "bench_identify", tier: Tier::ReadOnly, kinds: WITH_VNA, desc: "Instrument identity (*IDN?).", props: r#"{"instrument":{"type":"string"}}"#, required: &[] },
    ToolSpec { name: "bench_status", tier: Tier::ReadOnly, kinds: WITH_VNA,
        desc: "Scope: running/stopped, timebase, sample rate, per-channel volts/div, offset, coupling, probe attenuation. Supply: output on/off, setpoints, protection limits, measured volts and amps. VNA: USB and codecs found, IF, time per point, RF level.",
        props: r#"{"instrument":{"type":"string"}}"#, required: &[] },
    ToolSpec { name: "bench_measure", tier: Tier::ReadOnly, kinds: ANY,
        desc: "Scope: the scope's own parameter measurements on a channel (values **** mean it could not measure them). Supply: measured voltage, current and power.",
        props: r#"{"instrument":{"type":"string"},"channel":{"type":"integer","enum":[1,2]},"items":{"type":"array","items":{"type":"string","enum":["PKPK","MAX","MIN","AMPL","TOP","BASE","CMEAN","MEAN","RMS","CRMS","OVSN","FPRE","OVSP","RPRE","PER","FREQ","PWID","NWID","RISE","FALL","WID","DUTY","NDUTY"]},"description":"scope only; default PKPK,MEAN,FREQ,DUTY"}}"#, required: &[] },
    ToolSpec { name: "bench_capture", tier: Tier::ReadOnly, kinds: &[Kind::Scope],
        desc: "Read the scope channel's waveform memory, scale to volts, save it as little-endian f32 and return a summary (min, max, mean, peak-to-peak, rising edges, estimated frequency, sample interval, file). Refuses a stopped scope unless allow_stopped is true, because a stopped scope returns a frozen old frame.",
        props: r#"{"instrument":{"type":"string"},"channel":{"type":"integer","enum":[1,2]},"allow_stopped":{"type":"boolean"}}"#, required: &["channel"] },
    ToolSpec { name: "bench_scope_spectrum", tier: Tier::ReadConfig, kinds: &[Kind::Scope],
        desc: "Scope: the spectrum of a channel, computed on the host from the waveform memory (so it does not depend on the scope's own FFT or its trigger): window (rect, hanning, hamming, blackman), points (power of two, default 131072; resolution = sample rate / points), optional timebase_s (sets the scope's time per division first), and the strongest peaks with frequency and level (dBV rms at the scope input and dBm into 50 ohms). Use it to check a generator's fundamental and harmonics; lines more than about 45 dB below the strongest are not reliable (8-bit samples).",
        props: r#"{"instrument":{"type":"string"},"channel":{"type":"integer","enum":[1,2]},"window":{"type":"string","enum":["rect","hanning","hamming","blackman"]},"points":{"type":"integer"},"memory_depth":{"type":"string","description":"scope memory depth, e.g. 14K, 140K, 1.4M, 14M"},"timebase_s":{"type":"number"},"peaks":{"type":"integer"},"range_db":{"type":"number"},"allow_stopped":{"type":"boolean"}}"#, required: &["channel"] },
    ToolSpec { name: "bench_screenshot", tier: Tier::ReadOnly, kinds: &[Kind::Scope], desc: "Save the scope screen as an image file and return its path.", props: r#"{"instrument":{"type":"string"}}"#, required: &[] },
    ToolSpec { name: "bench_set", tier: Tier::ReadConfig, kinds: ANY,
        desc: "Scope: timebase_s (seconds per division), vdiv_v, offset_v, coupling (A1M/D1M/GND), attenuation (probe ratio), trigger_mode (AUTO/NORM/SINGLE/STOP); channel selects which channel. Supply: voltage_v and current_a setpoints, each capped by the max_volts and max_amps in the server's config; does not change the output state.",
        props: r#"{"instrument":{"type":"string"},"channel":{"type":"integer","enum":[1,2]},"timebase_s":{"type":"number"},"vdiv_v":{"type":"number"},"offset_v":{"type":"number"},"coupling":{"type":"string","enum":["A1M","D1M","GND"]},"attenuation":{"type":"number"},"trigger_type":{"type":"string","enum":["EDGE","SLEW","GLIT","INTV","RUNT","TV"],"description":"trigger type on `channel`; the type's other parameters keep the scope's own values"},"trigger_mode":{"type":"string","enum":["AUTO","NORM","SINGLE","STOP"]},"voltage_v":{"type":"number"},"current_a":{"type":"number"}}"#, required: &[] },
    ToolSpec { name: "bench_psu_output", tier: Tier::Full, kinds: &[Kind::Psu],
        desc: "DRIVES A LOAD: switch the supply output on (or off). On is refused unless the setpoints stored in the supply are within the configured max_volts and max_amps, needs confirm_load_connected: true from the person at the bench, and this server switches the output off again after for_s seconds (default and ceiling: the config's max_on_s, 600 s unless set) because the supply has no output timer. dry_run: true reads the stored setpoints and returns the plan without switching anything. To put the supply in its safe state use bench_psu_safe_state.",
        props: r#"{"instrument":{"type":"string"},"on":{"type":"boolean"},"for_s":{"type":"number","description":"seconds until this server switches the output off again, 1 to the config's max_on_s"},"confirm_load_connected":{"type":"boolean","description":"true: the person at the bench says the load is connected and this output may be driven"},"dry_run":{"type":"boolean","description":"true: return the plan, switch nothing"}}"#, required: &["on"] },
    ToolSpec { name: "bench_psu_safe_state", tier: Tier::ReadConfig, kinds: &[Kind::Psu],
        desc: "Supply: switch the output off and read it back to prove it (up to three tries). Safe at any time; use it first when anything went wrong. If the supply will not confirm off, the error says what to do by hand.",
        props: r#"{"instrument":{"type":"string"}}"#, required: &[] },
    ToolSpec { name: "bench_vna_sweep", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA: a linear sweep of start_hz to stop_hz in `points` points (1 to 401, about 0.2 s each at 60 ms per point; the unit emits for six point times) with RF on the TX port, measuring S21 (RX channel) and S11 (reflection channel). The results are raw ratios, not calibrated: the cable, bridge and front-end are not removed. Saves a CSV (frequency, S21 and S11 as re/im, dB and degrees) and returns a summary, and the rows themselves when there are at most 51 points. if_hz (a divisor of 48 kHz such as 12000, 6000 or 4000), point_ms (0.2 to 100) and rf_level (0 to 16383) override the instrument's settings for this sweep.",
        props: r#"{"instrument":{"type":"string"},"start_hz":{"type":"number"},"stop_hz":{"type":"number"},"points":{"type":"integer"},"if_hz":{"type":"number"},"point_ms":{"type":"number"},"rf_level":{"type":"integer"},"calibration":{"type":"string","description":"a .cal file from bench_vna_calibrate_build"},"cable":{"type":"string","description":"required with calibration: the cable text given when the standards were measured"}}"#, required: &["start_hz"] },
    ToolSpec { name: "bench_vna_power_cycle", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA: cold-reset the unit by switching its power-switching hub port off, waiting, and on again (about 12 s in all), then wait for the unit and both codecs and, unless verify is false, check it with a one-point sweep at 10 MHz (RF at the unit's own level, about -17 dBm, on the TX port). The unit keeps no settings across it, so use it to clear the silent state (reference channels read about zero) or a unit that does not answer. Needs hub and hub_port in the instrument's config. bench_vna_sweep already does this by itself once when a point fails after its retries.",
        props: r#"{"instrument":{"type":"string"},"verify":{"type":"boolean","description":"default true"}}"#, required: &[] },
    ToolSpec { name: "bench_vna_generator_on", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA signal generator: CW on the TX port at freq_hz (up to 1.3e9) and level_db (0 to -50, a negative attenuation; 0 dB is about -17 dBm; steps verified on a scope from 0 to -40 dB within 0.3 dB). Stays on until bench_vna_generator_off. Do not run a sweep or the meter while it is on.",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number"},"level_db":{"type":"number"}}"#, required: &["freq_hz","level_db"] },
    ToolSpec { name: "bench_vna_generator_off", tier: Tier::ReadConfig, kinds: &[Kind::Vna], desc: "VNA signal generator: switch the TX output off. If the unit is modulating (AM or FM) it refuses the request; the tool then USB resets the unit's control chip, which ends the modulation (about 3 s, no power cycle), and only if that fails power cycles the unit through its hub (15 to 90 s, loses all settings; needs the hub in the config).", props: r#"{"instrument":{"type":"string"}}"#, required: &[] },
    ToolSpec { name: "bench_vna_generator_level", tier: Tier::ReadConfig, kinds: &[Kind::Vna], desc: "VNA signal generator: change the level (0 to -50 dB) of a running generator.", props: r#"{"instrument":{"type":"string"},"level_db":{"type":"number"}}"#, required: &["level_db"] },
    ToolSpec { name: "bench_vna_generator_am", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA signal generator with amplitude modulation: carrier freq_hz at level_db (default -20), modulated by a sine of mod_hz (1 to 3000, default 500) at depth_pct (1 to 100, default 100). Verified on a scope. The unit then modulates in real time and refuses the switch-off request: bench_vna_generator_off ends it with a USB reset of the unit's control chip (no power cycle).",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number"},"level_db":{"type":"number"},"mod_hz":{"type":"number"},"depth_pct":{"type":"number"}}"#, required: &["freq_hz"] },
    ToolSpec { name: "bench_vna_generator_fm", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA signal generator with frequency modulation: carrier freq_hz at level_db (default -20), modulated by a sine of mod_hz (1 to 3000, default 500) with a peak deviation of deviation_hz (100 to 300000). Verified on a scope. Like AM, it ends only with a USB reset of the control chip, which bench_vna_generator_off does.",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number"},"level_db":{"type":"number"},"mod_hz":{"type":"number"},"deviation_hz":{"type":"number"}}"#, required: &["freq_hz","deviation_hz"] },
    ToolSpec { name: "bench_vna_spectrum", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA as a rudimentary spectrum analyzer (RX port): the power in the audio band within band_hz (default 5000, at most 20000) of the unit's LO at each of `points` frequencies from start_hz to stop_hz, in UNCALIBRATED codec counts, with the noise floor and the peaks; point spacing must not exceed band_hz. Use below 100 MHz (unfiltered LO: ghost lines; direct conversion: subharmonic responses; both not characterised). tracking_db (0 to -50) turns the tracking generator on (TX 12 kHz below the LO: a loopback then shows a line at 12 kHz). Do not run it with a generator on.",
        props: r#"{"instrument":{"type":"string"},"start_hz":{"type":"number"},"stop_hz":{"type":"number"},"points":{"type":"integer"},"band_hz":{"type":"number"},"point_ms":{"type":"number"},"tracking_db":{"type":"number"}}"#, required: &["start_hz","stop_hz","points"] },
    ToolSpec { name: "bench_vna_meter", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA frequency meter: the frequency of the signal on the RX input within +/-25 Hz of center_hz (param freq_hz), by the phase of RX against the unit's own reference over about 1.7 s (point_ms 60 default; the TX DDS runs at the centre, nothing else may use the unit). Returns the offset in Hz with its standard error and the reading. A TX-to-RX loopback reads 0. The sign and absolute accuracy are not yet verified against an external standard.",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number","description":"centre frequency in Hz"},"point_ms":{"type":"number"}}"#, required: &["freq_hz"] },
    ToolSpec { name: "bench_vna_calibrate_measure", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA calibration, step 1: sweep one standard (open, short, load or thru) and store it under `session`, with the settings and the `cable` text (what is between the ports and the reference plane). Repeat for each standard with the same start_hz, stop_hz, points and cable.",
        props: r#"{"instrument":{"type":"string"},"session":{"type":"string"},"standard":{"type":"string","enum":["open","short","load","thru"]},"cable":{"type":"string"},"start_hz":{"type":"number"},"stop_hz":{"type":"number"},"points":{"type":"integer"},"if_hz":{"type":"number"},"point_ms":{"type":"number"},"rf_level":{"type":"integer"}}"#, required: &["session","standard","cable","start_hz","stop_hz","points"] },
    ToolSpec { name: "bench_vna_calibrate_build", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA calibration, step 2: build the calibration file of `session` from its stored standards (open+short+load give S11, thru gives S21), optionally with the real kit's published numbers from kit_file (see vnwa_open cal::KitDefinition text format). Apply it with bench_vna_sweep calibration=<file> cable=<same text>; it is refused at other IF, time per point, RF level or cable.",
        props: r#"{"instrument":{"type":"string"},"session":{"type":"string"},"kit_file":{"type":"string"},"created":{"type":"string"}}"#, required: &["session"] },
    ToolSpec { name: "bench_sdr_capture", tier: Tier::ReadConfig, kinds: &[Kind::Sdr],
        desc: "SDR (bladeRF, LimeSDR): tune to freq_hz, capture `samples` complex baseband samples (default 262144, up to 16777216) at sample_rate_hz (default 2 MHz) and gain_db (default 30), receive only. Saves the raw I/Q as little-endian f32 pairs and returns RMS level, peak, DC offset, clipped sample count, the noise floor and the strongest peaks (dBFS at the set gain, not calibrated power). A bladeRF below 237.5 MHz needs its XB-200 configured (\"xb200\" in the config). channel 1 is the LimeSDR's second receiver. antenna picks the LimeSDR receive input (LNAH, LNAL or LNAW, the H, L and W connectors of each channel); the default is LNAL, which is what the driver selects at every frequency. The bladeRF has one input.",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number"},"sample_rate_hz":{"type":"number"},"gain_db":{"type":"number"},"antenna":{"type":"string","enum":["LNAH","LNAL","LNAW"]},"samples":{"type":"integer"},"channel":{"type":"integer","enum":[0,1]}}"#, required: &["freq_hz"] },
    ToolSpec { name: "bench_sdr_survey", tier: Tier::ReadConfig, kinds: &[Kind::Sdr],
        desc: "SDR: a power survey from start_hz to stop_hz, stepping the tuning and stitching the spectra (70 % of each capture's bandwidth is kept; at most 400 steps). Saves a CSV of frequency and dBFS and returns the median floor and the ten strongest peaks. The radio's own DC spike is removed from every step. Use it to look for signals or noise sources; levels are relative to full scale at the set gain.",
        props: r#"{"instrument":{"type":"string"},"start_hz":{"type":"number"},"stop_hz":{"type":"number"},"sample_rate_hz":{"type":"number"},"gain_db":{"type":"number"},"antenna":{"type":"string","enum":["LNAH","LNAL","LNAW"]},"fft_len":{"type":"integer"},"samples_per_step":{"type":"integer"}}"#, required: &["start_hz"] },
    ToolSpec { name: "bench_sdr_coherence", tier: Tier::ReadConfig, kinds: &[Kind::Sdr],
        desc: "SDR with two receivers (LimeSDR): capture both channels together at freq_hz, repeat `repeats` times with a fresh tune each time, and report the phase and gain of channel 0 relative to channel 1 at the strongest tone, how stable the phase is within a capture and across retunes, and the coherence. Needs one signal on both inputs (a splitter).",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number"},"sample_rate_hz":{"type":"number"},"gain_db":{"type":"number"},"antenna":{"type":"string","enum":["LNAH","LNAL","LNAW"]},"repeats":{"type":"integer"}}"#, required: &["freq_hz"] },
    ToolSpec { name: "bench_scope_self_cal", tier: Tier::Full, kinds: &[Kind::Scope],
        desc: "Run the scope's self-calibration (*CAL?). It rewrites the stored calibration, takes a minute or two and drives its own levels onto the inputs. All probes and cables must be disconnected first; confirm_inputs_disconnected must be true, and that must come from the person at the bench.",
        props: r#"{"instrument":{"type":"string"},"confirm_inputs_disconnected":{"type":"boolean"}}"#, required: &["confirm_inputs_disconnected"] },
];

pub(crate) fn out_dir() -> PathBuf {
    let d = std::env::var("BENCH_MCP_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("bench_mcp"));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The instrument a call is for: the named one, or the only one the tool applies to.
fn pick<'a>(
    insts: &'a [Instrument],
    tool: &ToolSpec,
    args: &Value,
) -> Result<&'a Instrument, String> {
    // [@ANCHOR: bench_mcp_pick]
    let fits = |i: &&Instrument| tool.kinds.contains(&i.kind);
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        let i = insts
            .iter()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no instrument named {n:?}"))?;
        return if fits(&i) {
            Ok(i)
        } else {
            Err(format!("{} does not apply to {n:?}", tool.name))
        };
    }
    let all: Vec<&Instrument> = insts.iter().filter(fits).collect();
    match all.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("{} applies to no configured instrument", tool.name)),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn connect(i: &Instrument) -> Result<Link, String> {
    match i.kind {
        Kind::Scope => Link::tcp(i.host.as_deref().unwrap_or_default(), i.port),
        Kind::Psu => Link::serial(i.serial.as_deref().unwrap_or_default(), i.baud),
        Kind::Vna => Err("the VNA has no SCPI link".into()),
        Kind::Sdr => Err("an SDR has no SCPI link".into()),
        Kind::UsbHub => Err("the USB hub tools have no SCPI link".into()),
        Kind::Audio => Err("the audio tools have no SCPI link".into()),
        Kind::Hamlib => Err("the hamlib tools open their own link".into()),
        Kind::Rotator => Err("the rotator tools open their own link".into()),
        Kind::Example => Err("the example tools open their own link".into()),
    }
}

/// One call at a time per instrument: a serial port or a scope socket has a single conversation.
fn instrument_lock(name: &str) -> Arc<Mutex<()>> {
    static LOCKS: Mutex<Option<HashMap<String, Arc<Mutex<()>>>>> = Mutex::new(None);
    let mut g = LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new)
        .entry(name.to_string())
        .or_default()
        .clone()
}

fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    if manual::is_manual_tool(name) {
        return manual::call(name, args);
    }
    if name.starts_with("bench_audio_") {
        return audio::call(insts, name, args);
    }
    if name.starts_with("bench_rotator_") {
        return rotator::call(insts, name, args);
    }
    if name.starts_with("bench_hamlib_") {
        return hamlib::call(insts, name, args);
    }
    if name.starts_with("bench_example_") {
        return example::call(insts, name, args);
    }
    if name.starts_with("bench_usb_") {
        return usb_hub::call(insts, name, args);
    }
    let tool = TOOLS
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    if name == "bench_list" {
        return Ok(json!(insts
            .iter()
            .map(|i| json!({"name": i.name, "driver": i.driver, "tier": format!("{:?}", i.tier)}))
            .collect::<Vec<_>>()));
    }
    let inst = pick(insts, tool, args)?;
    if inst.tier < tool.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            tool.tier, inst.name, inst.tier
        ));
    }
    let lock = instrument_lock(&inst.name);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    if inst.kind == Kind::Vna {
        // The unit's recovery inside measurements power cycles through the hub module's implementation.
        vna::install_power_hook();
        return match name {
            "bench_identify" => Ok(vna::identify(&inst.vna)),
            "bench_status" => vna::status(&inst.vna),
            "bench_vna_sweep" => vna::sweep(&inst.vna, args, &out_dir()),
            "bench_vna_power_cycle" => vna::power_cycle(&inst.vna, args),
            "bench_vna_generator_on" => vna_tools::generator_on(args),
            "bench_vna_generator_off" => vna_tools::generator_off(&inst.vna),
            "bench_vna_generator_fm" => vna_tools::generator_fm(args),
            "bench_vna_generator_level" => vna_tools::generator_level(args),
            "bench_vna_generator_am" => vna_tools::generator_am(args),
            "bench_vna_meter" => vna_tools::meter(&inst.vna, args),
            "bench_vna_spectrum" => vna_tools::spectrum(&inst.vna, args, &out_dir()),
            "bench_vna_calibrate_measure" => {
                vna_tools::calibrate_measure(&inst.vna, args, &out_dir())
            }
            "bench_vna_calibrate_build" => vna_tools::calibrate_build(args, &out_dir()),
            _ => Err(format!("{name} does not apply to {:?}", inst.name)),
        };
    }
    if inst.kind == Kind::Sdr {
        return match name {
            "bench_identify" | "bench_status" => sdr::identify(&inst.sdr),
            "bench_sdr_capture" => sdr::capture(&inst.sdr, args, &out_dir()),
            "bench_sdr_survey" => sdr::survey(&inst.sdr, args, &out_dir()),
            "bench_sdr_coherence" => sdr::coherence(&inst.sdr, args),
            _ => Err(format!("{name} does not apply to {:?}", inst.name)),
        };
    }
    let mut l = connect(inst)?;
    let lim = psu::Limits {
        max_volts: inst.max_volts,
        max_amps: inst.max_amps,
        max_on_s: inst.max_on_s,
    };
    match (name, inst.kind) {
        ("bench_identify", _) => Ok(json!({"idn": l.query("*IDN?")?})),
        ("bench_status", Kind::Scope) => scope::status(&mut l),
        ("bench_status", Kind::Psu) => psu::state(&mut l),
        ("bench_measure", Kind::Scope) => scope::measure(&mut l, args),
        ("bench_measure", Kind::Psu) => psu::measure(&mut l),
        ("bench_capture", _) => scope::capture(&mut l, args, &out_dir()),
        ("bench_scope_spectrum", Kind::Scope) => scope::spectrum(&mut l, args, &out_dir()),
        ("bench_screenshot", _) => scope::screenshot(&mut l, &out_dir()),
        ("bench_set", Kind::Scope) => scope::set(&mut l, args),
        ("bench_set", Kind::Psu) => psu::set(&mut l, args, &lim),
        ("bench_psu_safe_state", _) => {
            psu::safe_state(&mut l, &psu_timer(&inst.name, lock.clone()))
        }
        ("bench_psu_output", _) => psu_output(
            inst,
            &mut l,
            args,
            &lim,
            &psu_timer(&inst.name, lock.clone()),
        ),
        ("bench_scope_self_cal", _) => scope::self_cal(&mut l, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

/// The switch-off timer state of a supply, one per instrument name, sharing the instrument's lock.
fn psu_timer(name: &str, lock: Arc<Mutex<()>>) -> psu::Timer {
    // [@ANCHOR: bench_mcp_main_psu_timer]
    static TIMERS: Mutex<Option<HashMap<String, Arc<std::sync::atomic::AtomicU64>>>> =
        Mutex::new(None);
    let mut g = TIMERS.lock().unwrap_or_else(|e| e.into_inner());
    let generation = g
        .get_or_insert_with(HashMap::new)
        .entry(name.to_string())
        .or_default()
        .clone();
    psu::Timer { lock, generation }
}

/// `bench_psu_output`: the arguments are checked before the supply is asked anything.
fn psu_output(
    inst: &Instrument,
    l: &mut Link,
    args: &Value,
    lim: &psu::Limits,
    timer: &psu::Timer,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_main_psu_output]
    let flag = |k: &str| match args.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{k} must be true or false")),
    };
    let on = args
        .get("on")
        .and_then(|v| v.as_bool())
        .ok_or("on must be true or false")?;
    let (dry, confirmed) = (flag("dry_run")?, flag("confirm_load_connected")?);
    if !on {
        return if dry {
            Ok(json!({"dry_run": true, "sent": [], "would": ["OUTP OFF", "OUTP? (must read OFF)"]}))
        } else {
            psu::safe_state(l, timer)
        };
    }
    let for_s = psu::on_seconds(args, lim)?;
    if dry {
        return psu::plan_on(l, lim, for_s);
    }
    if !confirmed {
        return Err(format!(
            "bench_psu_output on drives whatever is connected to {:?}; call it with confirm_load_connected: true only when the person at the bench has said the load is connected and may be driven (dry_run: true shows the plan)",
            inst.name
        ));
    }
    let (serial, baud) = (inst.serial.clone().unwrap_or_default(), inst.baud);
    psu::output_on(l, lim, for_s, timer, move || Link::serial(&serial, baud))
}

/// Every tool this server can offer, as listed for a bench with each driver configured at the top
/// tier. No hardware is touched. The manual's argument tables and its tests read the schemas here.
pub(crate) fn all_tool_defs() -> Vec<Tool> {
    let bench = config::parse(
        r#"{"instruments":{
        "scope":{"driver":"siglent_sds","tier":"full","host":"127.0.0.1"},
        "psu":{"driver":"atm6301","tier":"full","serial":"/dev/null"},
        "vna":{"driver":"vnwa_open","tier":"full"},
        "blade":{"driver":"bladerf","tier":"full"},
        "lime":{"driver":"limesdr","tier":"full"},
        "usb":{"driver":"usb_hub","tier":"full"},
        "sound":{"driver":"audio","tier":"full"},
        "rotor":{"driver":"rotator","tier":"full","rotctld_port":4533},
        "rig":{"driver":"hamlib","tier":"full","model":1,"transmit":{"dummy_load":true,"attested_by":"EXAMPLE","attested_on":"2026-01-01","cabling":"example dummy load","max_power_w":5,"max_seconds":10,"frequency_ranges":[[7000000,7100000]],"allowed_modes":["CW"]}},
        "example":{"driver":"example","tier":"full","host":"192.0.2.1"}}}"#,
    )
    .expect("the all-instruments bench configuration parses");
    tool_defs(&bench)
}

fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    // [@ANCHOR: bench_mcp_main_tool_defs]
    let usb = usb_hub::tool_defs(insts);
    let audio = audio::tool_defs(insts);
    let hamlib = hamlib::tool_defs(insts);
    let rotator = rotator::tool_defs(insts);
    let example = example::tool_defs(insts);
    let tools: Vec<Tool> = TOOLS
        .iter()
        .filter(|t| t.name == "bench_list" || insts.iter().any(|i| t.kinds.contains(&i.kind) && i.tier >= t.tier))
        .map(|t| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(t.props).unwrap(), "required": t.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(t.name, format!("[{:?}] {}", t.tier, t.desc), Arc::new(schema))
        })
        .chain(usb)
        .chain(audio)
        .chain(hamlib)
        .chain(rotator)
        .chain(example)
        .chain(manual::tool_defs())
        .collect();
    manual::with_pointers(tools)
}

struct BenchServer {
    insts: Arc<Vec<Instrument>>,
}

impl ServerHandler for BenchServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
            .with_server_info(Implementation::new("bench_mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("Bench test equipment. Use bench_list for the instruments; tools above an instrument's tier are refused. The manual is in this server: start with bench_manual_index, bench_manual_search or bench_manual_for_tool (the same pages are MCP resources bench-manual://<path>).")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: tool_defs(&self.insts),
            ..Default::default()
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        manual::list_resources()
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        manual::read_resource(&request.uri)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        let args = Value::Object(request.arguments.unwrap_or_default());
        let insts = self.insts.clone();
        // The instruments are blocking peers, so keep them off the async runtime threads.
        let out = tokio::task::spawn_blocking(move || call(&insts, &name, &args))
            .await
            .map_err(|e| McpError::internal_error(format!("tool task: {e}"), None))?;
        let result = match out {
            Ok(v) => CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap(),
            )]),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
        };
        Ok(result.into())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The root helper of the USB hub tools: `sudo -n bench_mcp --usb-priv <operation> ...`; it takes no config and serves no MCP.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(|a| a.as_str()) == Some(usb_hub::privileged::FLAG) {
        std::process::exit(usb_hub::privileged::main(&argv[1..]));
    }
    // The transmit watchdog of the hamlib tools: this program run again, with the rig's rigctld address and a deadline.
    if argv.first().map(|a| a.as_str()) == Some(hamlib::guard::FLAG) {
        std::process::exit(hamlib::guard::main(&argv[1..]));
    }
    let insts = config::load()?;
    // A termination signal releases a keyed rig and stops the daemons this server started, then exits.
    tokio::spawn(async {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            return;
        };
        tokio::select! { _ = term.recv() => {}, _ = int.recv() => {} }
        let _ = tokio::task::spawn_blocking(hamlib::shutdown).await;
        std::process::exit(143);
    });
    let service = BenchServer {
        insts: Arc::new(insts),
    }
    .serve(stdio())
    .await?;
    let ended = service.waiting().await;
    // Whatever way the connection ended, stop what this server started.
    hamlib::shutdown();
    ended?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insts() -> Vec<Instrument> {
        config::parse(
            r#"{"instruments":{"scope":{"driver":"siglent_sds","host":"127.0.0.1"},
            "psu":{"driver":"atm6301","tier":"read_config","serial":"/dev/null"}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn tool_schemas_parse_and_dangerous_tools_are_tiered() {
        assert!(TOOLS
            .iter()
            .all(|t| serde_json::from_str::<Value>(t.props).is_ok()));
        let cal = TOOLS
            .iter()
            .find(|t| t.name == "bench_scope_self_cal")
            .unwrap();
        assert_eq!(cal.tier, Tier::Full);
    }

    // Tests [@ANCHOR: bench_mcp_pick]
    #[test]
    fn instrument_is_picked_by_name_or_by_being_the_only_fit() {
        let v = insts();
        let t = |n| TOOLS.iter().find(|t| t.name == n).unwrap();
        assert_eq!(
            pick(&v, t("bench_capture"), &json!({})).unwrap().name,
            "scope"
        );
        assert_eq!(
            pick(&v, t("bench_psu_output"), &json!({})).unwrap().name,
            "psu"
        );
        assert!(pick(&v, t("bench_status"), &json!({}))
            .unwrap_err()
            .contains("name the instrument"));
        assert!(pick(&v, t("bench_capture"), &json!({"instrument": "psu"})).is_err());
        assert!(pick(&v, t("bench_status"), &json!({"instrument": "nope"})).is_err());
    }

    #[test]
    fn only_tools_some_instrument_may_use_are_listed() {
        let names: Vec<String> = tool_defs(&insts())
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert!(
            names.contains(&"bench_set".to_string())
                && names.contains(&"bench_psu_safe_state".to_string())
                && !names.contains(&"bench_psu_output".to_string())
        );
        assert!(!names.contains(&"bench_scope_self_cal".to_string()));
    }

    // Tests [@ANCHOR: bench_mcp_manual_searchindex_score]
    // Tests [@ANCHOR: bench_mcp_manual_call]
    // Tests [@ANCHOR: bench_mcp_manual_excerpt]
    // Tests [@ANCHOR: bench_mcp_manual_releases]
    #[test]
    fn the_vna_sweep_tool_is_listed_and_allowed_only_at_read_config() {
        let vna = |tier: &str| {
            config::parse(&format!(
                r#"{{"instruments":{{"vna":{{"driver":"vnwa_open","tier":"{tier}"}}}}}}"#
            ))
            .unwrap()
        };
        let listed = |v: &[Instrument]| tool_defs(v).iter().any(|t| t.name == "bench_vna_sweep");
        assert!(listed(&vna("read_config")));
        assert!(!listed(&vna("read_only")));
        let e = call(
            &vna("read_only"),
            "bench_vna_sweep",
            &json!({"start_hz": 10e6}),
        )
        .unwrap_err();
        assert!(e.contains("needs tier"), "{e}");
        // Nothing but the VNA's own tools apply to it: no SCPI tool reaches it.
        let e = call(&vna("full"), "bench_set", &json!({})).unwrap_err();
        assert!(e.contains("applies to no configured instrument"), "{e}");
        // And a bad sweep is refused before any hardware is touched.
        let e = call(
            &vna("read_config"),
            "bench_vna_sweep",
            &json!({"start_hz": 10e6, "points": 0}),
        )
        .unwrap_err();
        assert!(e.contains("points"), "{e}");
    }

    // Tests [@ANCHOR: bench_vna_power_cycle]
    #[test]
    fn the_power_cycle_tool_needs_a_hub_and_read_config_and_does_not_block_other_instruments() {
        let _evidence = crate::usb_hub::testkit::evidence_lock();
        let vna = |tier: &str, hub: &str| {
            config::parse(&format!(
                r#"{{"instruments":{{"vna":{{"driver":"vnwa_open","tier":"{tier}"{hub}}}}}}}"#
            ))
            .unwrap()
        };
        let e = call(&vna("read_only", ""), "bench_vna_power_cycle", &json!({})).unwrap_err();
        assert!(e.contains("needs tier"), "{e}");
        let e = call(&vna("read_config", ""), "bench_vna_power_cycle", &json!({})).unwrap_err();
        assert!(e.contains("no hub configured"), "{e}");
        // A hub that does not exist is a clear error from uhubctl, before anything else happens.
        let e = call(
            &vna("read_config", r#","hub":"9-9","hub_port":1"#),
            "bench_vna_power_cycle",
            &json!({"verify": false}),
        )
        .unwrap_err();
        assert!(e.contains("uhubctl off on hub 9-9"), "{e}");
        let e = call(
            &vna("read_config", r#","hub":"9-9","hub_port":1"#),
            "bench_vna_power_cycle",
            &json!({"verify": "yes"}),
        )
        .unwrap_err();
        assert!(e.contains("verify"), "{e}");
        // Each instrument has its own lock: a long VNA call does not hold up the scope or the supply.
        let held = instrument_lock("lock_test_vna");
        let _g = held.lock().unwrap();
        assert!(instrument_lock("lock_test_scope").try_lock().is_ok());
        assert!(instrument_lock("lock_test_psu").try_lock().is_ok());
        assert!(instrument_lock("lock_test_vna").try_lock().is_err());
    }

    // Tests [@ANCHOR: bench_mcp_main_tool_defs]
    #[test]
    fn every_vna_function_has_its_own_tool_at_read_config_and_none_reaches_other_instruments() {
        let _evidence = crate::usb_hub::testkit::evidence_lock();
        let vna = |tier: &str| {
            config::parse(&format!(r#"{{"instruments":{{"vna":{{"driver":"vnwa_open","tier":"{tier}"}},"psu":{{"driver":"atm6301","tier":"full","serial":"/dev/null"}}}}}}"#)).unwrap()
        };
        let names = [
            "bench_vna_sweep",
            "bench_vna_power_cycle",
            "bench_vna_generator_on",
            "bench_vna_generator_off",
            "bench_vna_generator_level",
            "bench_vna_generator_am",
            "bench_vna_generator_fm",
            "bench_vna_spectrum",
            "bench_vna_meter",
            "bench_vna_calibrate_measure",
            "bench_vna_calibrate_build",
        ];
        let listed = |v: &[Instrument]| {
            tool_defs(v)
                .iter()
                .map(|t| t.name.to_string())
                .collect::<Vec<_>>()
        };
        for n in names {
            assert!(listed(&vna("read_config")).contains(&n.to_string()), "{n}");
            assert!(
                !listed(&vna("read_only")).contains(&n.to_string()),
                "{n} listed at read_only"
            );
            let spec = TOOLS.iter().find(|t| t.name == n).unwrap();
            assert_eq!(spec.tier, Tier::ReadConfig, "{n}");
            assert_eq!(spec.kinds, &[Kind::Vna], "{n}");
            let e = call(&vna("read_only"), n, &json!({})).unwrap_err();
            assert!(e.contains("needs tier"), "{n}: {e}");
        }
    }

    // Tests [@ANCHOR: bench_mcp_main_psu_output]
    // Tests [@ANCHOR: bench_mcp_main_psu_timer]
    #[test]
    fn psu_output_needs_confirmation_honours_dry_run_and_switches_off_by_the_safe_state() {
        use crate::example::testkit::FakeDevice;
        let dev = FakeDevice::start(|c| match c {
            "VOLT?" => Some("3.3".into()),
            "CURR?" => Some("0.5".into()),
            "OUTP?" => Some("OFF".into()),
            c if c.ends_with('?') => Some("0".into()),
            _ => None,
        });
        let v = insts();
        let psu_inst = v.iter().find(|i| i.name == "psu").unwrap();
        let lim = psu::Limits {
            max_volts: 5.5,
            max_amps: 1.0,
            max_on_s: 600.0,
        };
        let timer = psu_timer("main_test_psu", Arc::new(Mutex::new(())));
        assert!(Arc::ptr_eq(
            &timer.generation,
            &psu_timer("main_test_psu", Arc::new(Mutex::new(()))).generation
        ));
        let mut l = dev.link()().unwrap();
        let e = psu_output(psu_inst, &mut l, &json!({"on": true}), &lim, &timer).unwrap_err();
        assert!(e.contains("confirm_load_connected"), "{e}");
        let e = psu_output(
            psu_inst,
            &mut l,
            &json!({"on": true, "dry_run": "yes"}),
            &lim,
            &timer,
        )
        .unwrap_err();
        assert!(e.contains("dry_run"), "{e}");
        let e = psu_output(
            psu_inst,
            &mut l,
            &json!({"on": true, "for_s": 9999, "confirm_load_connected": true}),
            &lim,
            &timer,
        )
        .unwrap_err();
        assert!(e.contains("max_on_s"), "{e}");
        let d = psu_output(
            psu_inst,
            &mut l,
            &json!({"on": true, "dry_run": true}),
            &lim,
            &timer,
        )
        .unwrap();
        assert_eq!(d["dry_run"], true);
        let d = psu_output(
            psu_inst,
            &mut l,
            &json!({"on": false, "dry_run": true}),
            &lim,
            &timer,
        )
        .unwrap();
        assert_eq!(d["dry_run"], true);
        assert!(
            !dev.received()
                .iter()
                .any(|c| c.starts_with("OUTP ") && !c.ends_with('?')),
            "{:?}",
            dev.received()
        );
        let off = psu_output(psu_inst, &mut l, &json!({"on": false}), &lim, &timer).unwrap();
        assert_eq!(off["verified"], true);
    }

    #[test]
    fn higher_tier_calls_are_refused_by_the_server() {
        let e = call(
            &insts(),
            "bench_scope_self_cal",
            &json!({"confirm_inputs_disconnected": true}),
        )
        .unwrap_err();
        assert!(e.contains("needs tier"), "{e}");
        let e = call(&insts(), "bench_psu_output", &json!({"on": true})).unwrap_err();
        assert!(e.contains("needs tier Full"), "{e}");
    }
}

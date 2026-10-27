// SPDX-License-Identifier: AGPL-3.0-or-later
//! bench_mcp: one MCP server (rmcp, stdio) for the bench test equipment: a Siglent SDS1000X-E
//! oscilloscope over the LAN and an Abestop ATM6301 power supply on a USB-serial port. There is no
//! raw-SCPI tool. The instruments are listed in the JSON file named by `BENCH_MCP_CONFIG`, each with
//! its own permission tier (`read_only`, `read_config`, `full`); the server, not the model, refuses a
//! tool above an instrument's tier and does not list tools no instrument may use. `BENCH_MCP_OUT`
//! is the directory for captures and screenshots.

mod config;
mod dsp;
mod link;
mod psu;
mod scope;
mod sdr;
mod vna;

use config::{Instrument, Kind, Tier};
use link::Link;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
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
        props: r#"{"instrument":{"type":"string"},"channel":{"type":"integer","enum":[1,2]},"items":{"type":"array","items":{"type":"string","enum":["PKPK","MAX","MIN","MEAN","RMS","FREQ","PER","DUTY","RISE","FALL"]},"description":"scope only; default PKPK,MEAN,FREQ,DUTY"}}"#, required: &[] },
    ToolSpec { name: "bench_capture", tier: Tier::ReadOnly, kinds: &[Kind::Scope],
        desc: "Read the scope channel's waveform memory, scale to volts, save it as little-endian f32 and return a summary (min, max, mean, peak-to-peak, rising edges, estimated frequency, sample interval, file). Refuses a stopped scope unless allow_stopped is true, because a stopped scope returns a frozen old frame.",
        props: r#"{"instrument":{"type":"string"},"channel":{"type":"integer","enum":[1,2]},"allow_stopped":{"type":"boolean"}}"#, required: &["channel"] },
    ToolSpec { name: "bench_screenshot", tier: Tier::ReadOnly, kinds: &[Kind::Scope], desc: "Save the scope screen as an image file and return its path.", props: r#"{"instrument":{"type":"string"}}"#, required: &[] },
    ToolSpec { name: "bench_set", tier: Tier::ReadConfig, kinds: ANY,
        desc: "Scope: timebase_s (seconds per division), vdiv_v, offset_v, coupling (A1M/D1M/GND), attenuation (probe ratio), trigger_mode (AUTO/NORM/SINGLE/STOP); channel selects which channel. Supply: voltage_v and current_a setpoints, each capped by the max_volts and max_amps in the server's config; does not change the output state.",
        props: r#"{"instrument":{"type":"string"},"channel":{"type":"integer","enum":[1,2]},"timebase_s":{"type":"number"},"vdiv_v":{"type":"number"},"offset_v":{"type":"number"},"coupling":{"type":"string","enum":["A1M","D1M","GND"]},"attenuation":{"type":"number"},"trigger_mode":{"type":"string","enum":["AUTO","NORM","SINGLE","STOP"]},"voltage_v":{"type":"number"},"current_a":{"type":"number"}}"#, required: &[] },
    ToolSpec { name: "bench_psu_output", tier: Tier::ReadConfig, kinds: &[Kind::Psu],
        desc: "Switch the supply output. Off is allowed at read_config. On needs the instrument's tier to be full, and is refused unless the setpoints stored in the supply are within the configured caps.",
        props: r#"{"instrument":{"type":"string"},"on":{"type":"boolean"}}"#, required: &["on"] },
    ToolSpec { name: "bench_vna_sweep", tier: Tier::ReadConfig, kinds: &[Kind::Vna],
        desc: "VNA: a linear sweep of start_hz to stop_hz in `points` points (1 to 401, about 0.2 s each at 60 ms per point; the unit emits for six point times) with RF on the TX port, measuring S21 (RX channel) and S11 (reflection channel). The results are raw ratios, not calibrated: the cable, bridge and front-end are not removed. Saves a CSV (frequency, S21 and S11 as re/im, dB and degrees) and returns a summary, and the rows themselves when there are at most 51 points. if_hz (a divisor of 48 kHz such as 12000, 6000 or 4000), point_ms (0.2 to 100) and rf_level (0 to 16383) override the instrument's settings for this sweep.",
        props: r#"{"instrument":{"type":"string"},"start_hz":{"type":"number"},"stop_hz":{"type":"number"},"points":{"type":"integer"},"if_hz":{"type":"number"},"point_ms":{"type":"number"},"rf_level":{"type":"integer"}}"#, required: &["start_hz"] },
    ToolSpec { name: "bench_sdr_capture", tier: Tier::ReadConfig, kinds: &[Kind::Sdr],
        desc: "SDR (bladeRF, LimeSDR): tune to freq_hz, capture `samples` complex baseband samples (default 262144, up to 16777216) at sample_rate_hz (default 2 MHz) and gain_db (default 30), receive only. Saves the raw I/Q as little-endian f32 pairs and returns RMS level, peak, DC offset, clipped sample count, the noise floor and the strongest peaks (dBFS at the set gain, not calibrated power). A bladeRF below 237.5 MHz needs its XB-200 configured (\"xb200\" in the config). channel 1 is the LimeSDR's second receiver.",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number"},"sample_rate_hz":{"type":"number"},"gain_db":{"type":"number"},"samples":{"type":"integer"},"channel":{"type":"integer","enum":[0,1]}}"#, required: &["freq_hz"] },
    ToolSpec { name: "bench_sdr_survey", tier: Tier::ReadConfig, kinds: &[Kind::Sdr],
        desc: "SDR: a power survey from start_hz to stop_hz, stepping the tuning and stitching the spectra (70 % of each capture's bandwidth is kept; at most 400 steps). Saves a CSV of frequency and dBFS and returns the median floor and the ten strongest peaks. The radio's own DC spike is removed from every step. Use it to look for signals or noise sources; levels are relative to full scale at the set gain.",
        props: r#"{"instrument":{"type":"string"},"start_hz":{"type":"number"},"stop_hz":{"type":"number"},"sample_rate_hz":{"type":"number"},"gain_db":{"type":"number"},"fft_len":{"type":"integer"},"samples_per_step":{"type":"integer"}}"#, required: &["start_hz"] },
    ToolSpec { name: "bench_sdr_coherence", tier: Tier::ReadConfig, kinds: &[Kind::Sdr],
        desc: "SDR with two receivers (LimeSDR): capture both channels together at freq_hz, repeat `repeats` times with a fresh tune each time, and report the phase and gain of channel 0 relative to channel 1 at the strongest tone, how stable the phase is within a capture and across retunes, and the coherence. Needs one signal on both inputs (a splitter).",
        props: r#"{"instrument":{"type":"string"},"freq_hz":{"type":"number"},"sample_rate_hz":{"type":"number"},"gain_db":{"type":"number"},"repeats":{"type":"integer"}}"#, required: &["freq_hz"] },
    ToolSpec { name: "bench_scope_self_cal", tier: Tier::Full, kinds: &[Kind::Scope],
        desc: "Run the scope's self-calibration (*CAL?). It rewrites the stored calibration, takes a minute or two and drives its own levels onto the inputs. All probes and cables must be disconnected first; confirm_inputs_disconnected must be true, and that must come from the person at the bench.",
        props: r#"{"instrument":{"type":"string"},"confirm_inputs_disconnected":{"type":"boolean"}}"#, required: &["confirm_inputs_disconnected"] },
];

fn out_dir() -> PathBuf {
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
    if name == "bench_psu_output"
        && args.get("on").and_then(|v| v.as_bool()) == Some(true)
        && inst.tier < Tier::Full
    {
        return Err(format!(
            "switching the output on needs tier Full; {:?} is configured at {:?}",
            inst.name, inst.tier
        ));
    }
    let lock = instrument_lock(&inst.name);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    if inst.kind == Kind::Vna {
        return match name {
            "bench_identify" => Ok(vna::identify(&inst.vna)),
            "bench_status" => vna::status(&inst.vna),
            "bench_vna_sweep" => vna::sweep(&inst.vna, args, &out_dir()),
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
    };
    match (name, inst.kind) {
        ("bench_identify", _) => Ok(json!({"idn": l.query("*IDN?")?})),
        ("bench_status", Kind::Scope) => scope::status(&mut l),
        ("bench_status", Kind::Psu) => psu::state(&mut l),
        ("bench_measure", Kind::Scope) => scope::measure(&mut l, args),
        ("bench_measure", Kind::Psu) => psu::measure(&mut l),
        ("bench_capture", _) => scope::capture(&mut l, args, &out_dir()),
        ("bench_screenshot", _) => scope::screenshot(&mut l, &out_dir()),
        ("bench_set", Kind::Scope) => scope::set(&mut l, args),
        ("bench_set", Kind::Psu) => psu::set(&mut l, args, &lim),
        ("bench_psu_output", _) => {
            let on = args
                .get("on")
                .and_then(|v| v.as_bool())
                .ok_or("on must be true or false")?;
            if on && inst.tier < Tier::Full {
                return Err(format!(
                    "switching the output on needs tier Full; {:?} is configured at {:?}",
                    inst.name, inst.tier
                ));
            }
            psu::output(&mut l, on, &lim)
        }
        ("bench_scope_self_cal", _) => scope::self_cal(&mut l, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    TOOLS
        .iter()
        .filter(|t| t.name == "bench_list" || insts.iter().any(|i| t.kinds.contains(&i.kind) && i.tier >= t.tier))
        .map(|t| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(t.props).unwrap(), "required": t.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(t.name, format!("[{:?}] {}", t.tier, t.desc), Arc::new(schema))
        })
        .collect()
}

struct BenchServer {
    insts: Arc<Vec<Instrument>>,
}

impl ServerHandler for BenchServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("bench_mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("Bench test equipment over SCPI. Use bench_list for the instruments; tools above an instrument's tier are refused.")
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
    let insts = config::load()?;
    let service = BenchServer {
        insts: Arc::new(insts),
    }
    .serve(stdio())
    .await?;
    service.waiting().await?;
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
                && names.contains(&"bench_psu_output".to_string())
        );
        assert!(!names.contains(&"bench_scope_self_cal".to_string()));
    }

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

// SPDX-License-Identifier: AGPL-3.0-or-later
//! bench_mcp: one MCP server (rmcp, stdio) for the bench test equipment: a Siglent SDS1000X-E
//! oscilloscope over the LAN and an Abestop ATM6301 power supply on a USB-serial port. There is no
//! raw-SCPI tool. The instruments are listed in the JSON file named by `BENCH_MCP_CONFIG`, each with
//! its own permission tier (`read_only`, `read_config`, `full`); the server, not the model, refuses a
//! tool above an instrument's tier and does not list tools no instrument may use. `BENCH_MCP_OUT`
//! is the directory for captures and screenshots.

mod config;
mod link;
mod psu;
mod scope;

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

const TOOLS: &[ToolSpec] = &[
    ToolSpec { name: "bench_list", tier: Tier::ReadOnly, kinds: ANY, desc: "List the configured instruments with their driver and permission tier.", props: "{}", required: &[] },
    ToolSpec { name: "bench_identify", tier: Tier::ReadOnly, kinds: ANY, desc: "Instrument identity (*IDN?).", props: r#"{"instrument":{"type":"string"}}"#, required: &[] },
    ToolSpec { name: "bench_status", tier: Tier::ReadOnly, kinds: ANY,
        desc: "Scope: running/stopped, timebase, sample rate, per-channel volts/div, offset, coupling, probe attenuation. Supply: output on/off, setpoints, protection limits, measured volts and amps.",
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

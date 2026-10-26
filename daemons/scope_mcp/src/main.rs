// SPDX-License-Identifier: AGPL-3.0-or-later
//! scope_mcp: an MCP server (rmcp, stdio) for a Siglent SDS1000X-E series oscilloscope on
//! the LAN (SCPI, TCP 5025). There is no raw-SCPI tool. Every tool belongs to a permission tier and
//! the server, not the model, refuses tools above `SCOPE_MCP_TIER` (`read_only` default,
//! `read_config`, `full`). Settings: `SCOPE_MCP_HOST` (required), `SCOPE_MCP_PORT` (5025),
//! `SCOPE_MCP_OUT` (directory for captures and screenshots).

mod scpi;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::stdio;
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt};
use scpi::Scope;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(PartialEq, PartialOrd, Clone, Copy, Debug)]
enum Tier {
    ReadOnly = 0,
    ReadConfig = 1,
    Full = 2,
}

struct ToolSpec {
    name: &'static str,
    tier: Tier,
    desc: &'static str,
    props: &'static str,
    required: &'static [&'static str],
}

const TOOLS: &[ToolSpec] = &[
    ToolSpec { name: "scope_identify", tier: Tier::ReadOnly, desc: "Instrument identity (*IDN?).", props: "{}", required: &[] },
    ToolSpec { name: "scope_status", tier: Tier::ReadOnly, desc: "Acquisition state (running/stopped), timebase, sample rate and per-channel volts/div, offset, coupling, probe attenuation.", props: "{}", required: &[] },
    ToolSpec { name: "scope_measure", tier: Tier::ReadOnly,
        desc: "Built-in parameter measurements on a channel (the scope's own, from its current frame). Values with **** mean the scope could not measure them.",
        props: r#"{"channel":{"type":"integer","enum":[1,2]},"items":{"type":"array","items":{"type":"string","enum":["PKPK","MAX","MIN","MEAN","RMS","FREQ","PER","DUTY","RISE","FALL"]},"description":"default PKPK,MEAN,FREQ,DUTY"}}"#, required: &["channel"] },
    ToolSpec { name: "scope_capture", tier: Tier::ReadOnly,
        desc: "Read the channel's waveform memory, scale to volts, save it as little-endian f32 and return a summary (min, max, mean, peak-to-peak, rising edges, estimated frequency, sample interval, file). Refuses a stopped scope unless allow_stopped is true, because a stopped scope returns a frozen old frame.",
        props: r#"{"channel":{"type":"integer","enum":[1,2]},"allow_stopped":{"type":"boolean"}}"#, required: &["channel"] },
    ToolSpec { name: "scope_screenshot", tier: Tier::ReadOnly, desc: "Save the screen as an image file and return its path.", props: "{}", required: &[] },
    ToolSpec { name: "scope_set", tier: Tier::ReadConfig,
        desc: "Change acquisition settings. Any field may be omitted. timebase_s: seconds per division. vdiv_v: volts per division. offset_v: vertical offset. coupling: A1M/D1M/GND. attenuation: probe ratio (1, 10, ...). trigger_mode: AUTO/NORM/SINGLE/STOP.",
        props: r#"{"channel":{"type":"integer","enum":[1,2]},"timebase_s":{"type":"number"},"vdiv_v":{"type":"number"},"offset_v":{"type":"number"},"coupling":{"type":"string","enum":["A1M","D1M","GND"]},"attenuation":{"type":"number"},"trigger_mode":{"type":"string","enum":["AUTO","NORM","SINGLE","STOP"]}}"#, required: &[] },
    ToolSpec { name: "scope_self_cal", tier: Tier::Full,
        desc: "Run the scope's self-calibration (*CAL?). It rewrites the scope's stored calibration, takes a minute or two and drives its own levels onto the inputs. All probes and cables must be disconnected from the inputs first; confirm_inputs_disconnected must be true, and that must come from the person at the bench.",
        props: r#"{"confirm_inputs_disconnected":{"type":"boolean"}}"#, required: &["confirm_inputs_disconnected"] },
];

fn tier_from_env() -> Tier {
    match std::env::var("SCOPE_MCP_TIER").as_deref() {
        Ok("full") => Tier::Full,
        Ok("read_config") => Tier::ReadConfig,
        _ => Tier::ReadOnly,
    }
}

fn out_dir() -> PathBuf {
    let d = std::env::var("SCOPE_MCP_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("scope_mcp"));
    let _ = std::fs::create_dir_all(&d);
    d
}

fn connect() -> Result<Scope, String> {
    let host =
        std::env::var("SCOPE_MCP_HOST").map_err(|_| "SCOPE_MCP_HOST is not set".to_string())?;
    let port = std::env::var("SCOPE_MCP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5025);
    Scope::connect(&host, port)
}

fn ch(args: &Value) -> Result<u8, String> {
    match args.get("channel").and_then(|c| c.as_u64()) {
        Some(c @ 1..=2) => Ok(c as u8),
        _ => Err("channel must be 1 or 2".into()),
    }
}

fn stamp() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    t.to_string()
}

fn status(s: &mut Scope) -> Result<Value, String> {
    let mode = s.query("TRIG_MODE?")?;
    let tdiv = s.number("TDIV?")?;
    let sara = s.number("SARA?")?;
    let mut chans = Vec::new();
    for c in 1..=2 {
        chans.push(json!({
            "channel": c,
            "vdiv_v": s.number(&format!("C{c}:VDIV?"))?,
            "offset_v": s.number(&format!("C{c}:OFST?"))?,
            "coupling": Scope::value(&s.query(&format!("C{c}:CPL?"))?),
            "attenuation": s.number(&format!("C{c}:ATTN?"))?,
        }));
    }
    let m = Scope::value(&mode).to_string();
    Ok(
        json!({"trigger_mode": m, "acquisition": if m == "STOP" { "stopped" } else { "running" },
              "timebase_s_per_div": tdiv, "sample_rate_sa_s": sara, "channels": chans}),
    )
}

struct Analysis {
    min: f32,
    max: f32,
    mean: f64,
    edges: u32,
    freq: Option<f64>,
}

/// Summary of a captured waveform. Rising edges are counted through a hysteresis band of 20% of the
/// span around the midpoint, so noise on a flat level is not counted as edges.
fn analyse(volts: &[f32], sara: f64) -> Analysis {
    // [@ANCHOR: scope_mcp_analyse]
    let (mn, mx) = volts
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let mean = volts.iter().map(|&v| v as f64).sum::<f64>() / volts.len() as f64;
    let mid = (mn + mx) / 2.0;
    let (hi, lo) = (mid + 0.1 * (mx - mn), mid - 0.1 * (mx - mn));
    let (mut state_high, mut first, mut last, mut edges) =
        (volts[0] > mid, None::<usize>, 0usize, 0u32);
    for (i, &v) in volts.iter().enumerate() {
        if state_high && v < lo {
            state_high = false;
        } else if !state_high && v > hi {
            state_high = true;
            edges += 1;
            first.get_or_insert(i);
            last = i;
        }
    }
    let freq = match first {
        Some(f) if edges > 1 => Some(sara * (edges - 1) as f64 / (last - f) as f64),
        _ => None,
    };
    Analysis {
        min: mn,
        max: mx,
        mean,
        edges,
        freq,
    }
}

fn capture(s: &mut Scope, args: &Value) -> Result<Value, String> {
    let c = ch(args)?;
    let stopped = Scope::value(&s.query("TRIG_MODE?")?) == "STOP";
    if stopped && args.get("allow_stopped").and_then(|v| v.as_bool()) != Some(true) {
        return Err("the scope is stopped, so its waveform memory is an old frozen frame. Set trigger_mode AUTO (scope_set) or pass allow_stopped=true.".into());
    }
    let vdiv = s.number(&format!("C{c}:VDIV?"))?;
    let ofst = s.number(&format!("C{c}:OFST?"))?;
    let sara = s.number("SARA?")?;
    s.set_timeout(30);
    let raw = s.block(&format!("C{c}:WF? DAT2"))?;
    s.set_timeout(10);
    let volts: Vec<f32> = raw
        .iter()
        .map(|&b| ((b as i8) as f32) * (vdiv as f32) / 25.0 - ofst as f32)
        .collect();
    if volts.is_empty() {
        return Err("empty waveform".into());
    }
    let st = analyse(&volts, sara);
    let path = out_dir().join(format!("ch{c}_{}.f32", stamp()));
    let bytes: Vec<u8> = volts.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(
        json!({"channel": c, "samples": volts.len(), "sample_interval_s": 1.0 / sara, "acquisition_was_stopped": stopped,
              "min_v": st.min, "max_v": st.max, "mean_v": st.mean, "pkpk_v": st.max - st.min,
              "rising_edges": st.edges, "estimated_frequency_hz": st.freq, "file": path, "file_format": "little-endian f32 volts"}),
    )
}

fn set(s: &mut Scope, args: &Value) -> Result<Value, String> {
    let mut done = Vec::new();
    let mut cmd = |s: &mut Scope, c: String| -> Result<(), String> {
        s.send(&c)?;
        done.push(c);
        Ok(())
    };
    if let Some(v) = args.get("timebase_s").and_then(|v| v.as_f64()) {
        cmd(s, format!("TDIV {v:E}S"))?;
    }
    let need_ch = ["vdiv_v", "offset_v", "coupling", "attenuation"]
        .iter()
        .any(|k| args.get(k).is_some());
    if need_ch {
        let c = ch(args)?;
        if let Some(v) = args.get("vdiv_v").and_then(|v| v.as_f64()) {
            cmd(s, format!("C{c}:VDIV {v:E}V"))?;
        }
        if let Some(v) = args.get("offset_v").and_then(|v| v.as_f64()) {
            cmd(s, format!("C{c}:OFST {v:E}V"))?;
        }
        if let Some(v) = args.get("coupling").and_then(|v| v.as_str()) {
            if !["A1M", "D1M", "GND"].contains(&v) {
                return Err("bad coupling".into());
            }
            cmd(s, format!("C{c}:CPL {v}"))?;
        }
        if let Some(v) = args.get("attenuation").and_then(|v| v.as_f64()) {
            cmd(s, format!("C{c}:ATTN {v}"))?;
        }
    }
    if let Some(v) = args.get("trigger_mode").and_then(|v| v.as_str()) {
        if !["AUTO", "NORM", "SINGLE", "STOP"].contains(&v) {
            return Err("bad trigger_mode".into());
        }
        cmd(s, format!("TRIG_MODE {v}"))?;
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(json!({"sent": done, "status": status(s)?}))
}

fn call(name: &str, args: &Value) -> Result<Value, String> {
    let tool = TOOLS
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    if tool.tier > tier_from_env() {
        return Err(format!(
            "{name} needs tier {:?}; the server is running at {:?} (SCOPE_MCP_TIER)",
            tool.tier,
            tier_from_env()
        ));
    }
    let mut s = connect()?;
    match name {
        "scope_identify" => Ok(json!({"idn": s.query("*IDN?")?})),
        "scope_status" => status(&mut s),
        "scope_measure" => {
            let c = ch(args)?;
            let items: Vec<String> = match args.get("items").and_then(|i| i.as_array()) {
                Some(a) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                None => ["PKPK", "MEAN", "FREQ", "DUTY"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            };
            let mut m = serde_json::Map::new();
            for it in items {
                if !it.chars().all(|c| c.is_ascii_uppercase()) {
                    return Err("bad item".into());
                }
                let r = s.query(&format!("C{c}:PAVA? {it}"))?;
                m.insert(it, json!(Scope::value(&r)));
            }
            Ok(Value::Object(m))
        }
        "scope_capture" => capture(&mut s, args),
        "scope_screenshot" => {
            let raw = s.raw_until_idle("SCDP", 1500, 30)?;
            if raw.len() < 100 {
                return Err(format!("screenshot: only {} bytes", raw.len()));
            }
            let ext = if raw.starts_with(b"BM") {
                "bmp"
            } else if raw.starts_with(b"\x89PNG") {
                "png"
            } else {
                "bin"
            };
            let p = out_dir().join(format!("screen_{}.{ext}", stamp()));
            std::fs::write(&p, &raw).map_err(|e| e.to_string())?;
            Ok(json!({"file": p, "bytes": raw.len()}))
        }
        "scope_set" => set(&mut s, args),
        "scope_self_cal" => {
            if args
                .get("confirm_inputs_disconnected")
                .and_then(|v| v.as_bool())
                != Some(true)
            {
                return Err("refused: confirm_inputs_disconnected must be true, after the person at the bench has unplugged every probe".into());
            }
            s.set_timeout(300);
            let r = s.query("*CAL?")?;
            Ok(
                json!({"cal_reply": r, "note": "0 means the scope accepted the command; the calibration may keep running for a minute or two. Wait before measuring."}),
            )
        }
        _ => Err("unreachable".into()),
    }
}

fn tool_defs() -> Vec<Tool> {
    TOOLS
        .iter()
        .filter(|t| t.tier <= tier_from_env())
        .map(|t| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(t.props).unwrap(), "required": t.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(t.name, format!("[{:?}] {}", t.tier, t.desc), Arc::new(schema))
        })
        .collect()
}

struct ScopeServer;

impl ServerHandler for ScopeServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("scope_mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("Siglent SDS1000X-E oscilloscope over LAN SCPI. Tools above the server's tier are not offered.")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: tool_defs(),
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
        // The scope is a blocking TCP peer, so keep it off the async runtime threads.
        let out = tokio::task::spawn_blocking(move || call(&name, &args))
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
    let service = ScopeServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: scope_mcp_analyse]
    #[test]
    fn square_wave_edges_and_frequency() {
        // 1 kHz square wave sampled at 1 MSa/s, 0 V to 3 V, with a little noise on the levels.
        let v: Vec<f32> = (0..10_000)
            .map(|i| {
                if (i / 500) % 2 == 1 {
                    3.0 + 0.02 * ((i % 7) as f32 - 3.0)
                } else {
                    0.02 * ((i % 5) as f32 - 2.0)
                }
            })
            .collect();
        let a = analyse(&v, 1.0e6);
        assert_eq!(a.edges, 10);
        let f = a.freq.unwrap();
        assert!((f - 1000.0).abs() < 1.0, "{f}");
        assert!(a.max > 3.0 && a.min < 0.0);
    }

    #[test]
    fn flat_line_has_no_edges_or_frequency() {
        let v = vec![-3.0f32; 1000];
        let a = analyse(&v, 1.0e9);
        assert_eq!((a.edges, a.freq), (0, None));
    }

    #[test]
    fn tiers_are_ordered() {
        assert!(Tier::ReadOnly < Tier::ReadConfig && Tier::ReadConfig < Tier::Full);
        let cal = TOOLS.iter().find(|t| t.name == "scope_self_cal").unwrap();
        assert_eq!(cal.tier, Tier::Full);
        assert!(TOOLS
            .iter()
            .all(|t| serde_json::from_str::<Value>(t.props).is_ok()));
    }
}

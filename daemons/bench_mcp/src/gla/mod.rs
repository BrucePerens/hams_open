// SPDX-License-Identifier: AGPL-3.0-or-later
//! Logic analyzer and pattern generator tools for the Glasgow (`bench_gla_*`), built on the Glasgow backend (`glasgow`): host-side
//! trigger, pre-trigger window, sample rate, VCD, CSV and sigrok `.sr` files, a summary with glitch report, UART, I2C and SPI decoders,
//! and a bounded pattern generator. Entries: `docs/GLASGOW_LOGIC_TOOLS.md`.

use crate::config::{Instrument, Kind, Tier};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::sync::Arc;

pub mod decode;
pub mod export;
pub mod pattern;
pub mod stats;
pub mod tools;
pub mod trigger;
pub mod wave;

// [@ANCHOR: bench_mcp_gla_spec]
struct Spec {
    name: &'static str,
    tier: Tier,
    desc: &'static str,
    props: &'static str,
    required: &'static [&'static str],
}

const CAPTURE_PROPS: &str = concat!(
    r#""pins":{"type":"string","description":"pin spec, e.g. A0:3 or A0:1,B0; default A0:3"},"duration_s":{"type":"number","description":"0.1 up to the config's max_capture_s, default 2"},"pull":{"type":"string","enum":["none","up","down"]},"confirm":{"type":"boolean","description":"true: required with pull up or down; the person at the bench agrees weak pulls may be put on the target's pins"},"#,
    r#""trigger":{"type":"object","description":"{type: none|edge|level|pattern, pin, edge: rise|fall|either, level: 0|1, pattern: '1x0x' (one char per pin), min_ns}"},"pre_trigger_ms":{"type":"number","description":"keep this much of the capture before the trigger"},"post_trigger_ms":{"type":"number","description":"keep this much after the trigger (default: the rest)"},"samples":{"type":"integer","description":"window length in samples at sample_rate_hz (instead of post_trigger_ms)"},"sample_rate_hz":{"type":"number","description":"1 to 48000000; default: 4 samples on the shortest pulse"},"glitch_ns":{"type":"integer","description":"pulses shorter than this are reported as glitches"},"formats":{"type":"array","items":{"type":"string","enum":["vcd","csv","sr"]}},"decode":{"type":"object","description":"{type: uart, pin, baud, bits, parity, invert} | {type: i2c, scl, sda} | {type: spi, cs, sck, copi, cipo, mode, bits, msb_first, cs_active_low}"}"#
);
const ANALYZE_PROPS: &str = concat!(
    r#""file":{"type":"string","description":"file name of a .vcd in the output directory"},"#,
    r#""trigger":{"type":"object","description":"{type: none|edge|level|pattern, pin, edge: rise|fall|either, level: 0|1, pattern: '1x0x' (one char per pin), min_ns}"},"pre_trigger_ms":{"type":"number","description":"keep this much of the capture before the trigger"},"post_trigger_ms":{"type":"number","description":"keep this much after the trigger (default: the rest)"},"samples":{"type":"integer","description":"window length in samples at sample_rate_hz (instead of post_trigger_ms)"},"sample_rate_hz":{"type":"number","description":"1 to 48000000; default: 4 samples on the shortest pulse"},"glitch_ns":{"type":"integer","description":"pulses shorter than this are reported as glitches"},"formats":{"type":"array","items":{"type":"string","enum":["vcd","csv","sr"]}},"decode":{"type":"object","description":"{type: uart, pin, baud, bits, parity, invert} | {type: i2c, scl, sda} | {type: spi, cs, sck, copi, cipo, mode, bits, msb_first, cs_active_low}"}"#
);

const SPECS: &[Spec] = &[
    Spec { name: "bench_gla_limits", tier: Tier::ReadOnly,
        desc: "LOGIC ANALYZER (Glasgow): what the USB link can carry: bytes per recorded edge, the highest sustained edge rate for 12 Mb/s and 480 Mb/s links, the trigger and sample-rate model of the revision C analyzer. A model, labelled as such; the measured limits are in the manual page. No device access besides reading the USB speed from sysfs. Manual: tools/bench_gla_limits",
        props: r#""pins":{"type":"integer"},"link_mbps":{"type":"number","description":"default: the speed the Glasgow is linked at, else 480"}"#, required: &[] },
    Spec { name: "bench_gla_analyze", tier: Tier::ReadOnly,
        desc: "LOGIC ANALYZER, FILE ONLY: analyses a .vcd file in the output directory (a capture of bench_glasgow_logic_capture or bench_gla_capture, or any VCD of 1-bit wires): trigger window with pre-trigger part, sample rate, summary per pin (edges, frequency, duty cycle, pulse widths, glitches), files in VCD, CSV and sigrok .sr, and an optional UART, I2C or SPI decode. Touches no device. Manual: tools/bench_gla_analyze",
        props: ANALYZE_PROPS, required: &["file"] },
    Spec { name: "bench_gla_capture", tier: Tier::ReadConfig,
        desc: "LOGIC ANALYZER, PASSIVE (Glasgow): records pins through bench_glasgow_logic_capture (the port's voltage must have been set by bench_glasgow_port_set; revision C needs a 480 Mb/s link), then applies a trigger with a pre-trigger window, a sample rate, writes VCD, CSV and sigrok .sr files and returns the summary with a glitch report and an optional UART, I2C or SPI decode. The revision C analyzer has no hardware trigger: the trigger is found in the recorded data. Manual: tools/bench_gla_capture",
        props: CAPTURE_PROPS, required: &[] },
    Spec { name: "bench_gla_safe_state", tier: Tier::ReadConfig,
        desc: "LOGIC ANALYZER / PATTERN GENERATOR (Glasgow): the safe state of the Glasgow, the same as bench_glasgow_safe_state which it runs: every port's voltage regulator and every pin driver off, read back at 0 V. Safe at any time; use it first when anything went wrong. Manual: tools/bench_gla_safe_state",
        props: "", required: &[] },
    Spec { name: "bench_gla_pattern", tier: Tier::Full,
        desc: "PATTERN GENERATOR, DRIVES PINS (Glasgow): plans a square wave or PWM (frequency_hz, duty, cycles or duration_s), a pulse train (width_ms, gap_ms, count), a bit pattern (bits, bit_ms, repeat) or raw steps, tells the frequency and duty it will really make, and has bench_glasgow_pattern play it. Host-timed: holds of 0.5 ms or more (a square wave up to about 1 kHz), about 1 ms of jitter. The backend's rules apply: the port voltage set first, confirm true, at most max_capture_s, pins released at the end. dry_run returns the plan. Manual: tools/bench_gla_pattern",
        props: r#""pins":{"type":"string","description":"1 to 8 pins, e.g. A0:3"},"pattern":{"type":"object","description":"{type: square|pwm|pulses|bits|steps, pin (index among pins), frequency_hz, duty, cycles, duration_s, width_ms, gap_ms, count, bits, bit_ms, repeat, steps}"},"confirm":{"type":"boolean"},"dry_run":{"type":"boolean"}"#, required: &["pins", "pattern"] },
];

/// The tools to list: those of the logic-analyzer module at or below the tier of a configured Glasgow.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    // [@ANCHOR: gla_tool_defs]
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Glasgow && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(&format!("{{{}\"instrument\":{{\"type\":\"string\"}}}}", if s.props.is_empty() { String::new() } else { format!("{},", s.props) })).unwrap_or(Value::Null), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

// [@ANCHOR: bench_mcp_gla_real]
/// The production backend: the Glasgow module's own `call`, with the same instrument list.
struct Real<'a>(&'a [Instrument]);

impl tools::Backend for Real<'_> {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        // [@ANCHOR: gla_real_backend]
        crate::glasgow::call(self.0, tool, args)
    }
}

fn glasgow_instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: gla_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Glasgow).collect();
    match (
        args.get("instrument").and_then(|n| n.as_str()),
        ours.as_slice(),
    ) {
        (Some(n), l) => l
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no glasgow instrument named {n:?}")),
        (None, [one]) => Ok(one),
        (None, []) => Err("no glasgow instrument is configured".into()),
        (None, many) => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Routes a `bench_gla_*` call: the instrument and its tier are checked here, the work is in `tools`.
pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: gla_call]
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let inst = glasgow_instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    let backend = Real(insts);
    let ctx = tools::Ctx {
        backend: &backend,
        out_dir: crate::out_dir(),
        max_capture_s: inst.glasgow.max_capture_s,
    };
    let mut args = args.clone();
    if let Value::Object(m) = &mut args {
        m.entry("instrument").or_insert(json!(inst.name));
    }
    match name {
        "bench_gla_limits" => {
            let speed = args.get("link_mbps").and_then(|s| s.as_f64()).or_else(|| {
                crate::glasgow::parse::usb_device(std::path::Path::new("/sys/bus/usb/devices"))
                    .and_then(|u| u.get("link_speed_mbps").and_then(|s| s.as_f64()))
            });
            let mut v = tools::limits(
                args.get("pins").and_then(|p| p.as_u64()).unwrap_or(4) as u32,
                speed.unwrap_or(480.0),
            );
            v["link_mbps_source"] = json!(if args.get("link_mbps").is_some() {
                "argument"
            } else if speed.is_some() {
                "sysfs"
            } else {
                "assumed"
            });
            Ok(v)
        }
        "bench_gla_safe_state" => tools::Backend::call(
            &backend,
            "bench_glasgow_safe_state",
            &json!({"instrument": inst.name}),
        ),
        "bench_gla_analyze" => tools::analyze(&ctx, &args),
        "bench_gla_capture" => tools::capture(&ctx, &args),
        _ => tools::pattern_tool(&ctx, &args),
    }
}

#[cfg(test)]
mod tests;

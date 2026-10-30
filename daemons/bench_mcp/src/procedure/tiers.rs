// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tools a procedure may call, with their tiers, taken from the same listing the MCP clients see
//! (`crate::tool_defs`). A procedure never calls a tool above the tier cap it was started with, never starts another
//! procedure, and the dispatcher then applies the instrument's own tier as for any call.

use crate::config::{Instrument, Tier};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Tools a procedure cannot call: the procedure tools themselves (a procedure that starts procedures would escape the
/// run limit and the tier cap of its parent).
pub const NOT_FOR_PROCEDURES: &str = "bench_proc_";

pub struct ToolInfo {
    pub tier: Tier,
    pub description: String,
    pub schema: Value,
}

pub struct ToolIndex {
    tools: BTreeMap<String, ToolInfo>,
}

/// The name of a tier as the tools' `tier` argument and the manual spell it.
pub fn tier_name(t: Tier) -> &'static str {
    // [@ANCHOR: proc_tier_name]
    match t {
        Tier::ReadOnly => "read_only",
        Tier::ReadConfig => "read_config",
        Tier::Full => "full",
    }
}

/// The tier named by a `tier` argument.
pub fn parse_tier(s: &str) -> Result<Tier, String> {
    // [@ANCHOR: proc_tier_parse]
    match s {
        "read_only" => Ok(Tier::ReadOnly),
        "read_config" => Ok(Tier::ReadConfig),
        "full" => Ok(Tier::Full),
        o => Err(format!(
            "tier {o:?} is not one of read_only, read_config, full"
        )),
    }
}

impl ToolIndex {
    /// The tools listed for this bench whose tier is at most `cap`, without the procedure tools.
    pub fn build(insts: &[Instrument], cap: Tier) -> ToolIndex {
        // [@ANCHOR: proc_tool_index]
        let mut tools = BTreeMap::new();
        for t in crate::tool_defs(insts) {
            let name = t.name.to_string();
            if name.starts_with(NOT_FOR_PROCEDURES) {
                continue;
            }
            let desc = t.description.as_deref().unwrap_or_default().to_string();
            let tier = match desc.split_once(']').map(|(a, _)| a) {
                Some("[ReadOnly") => Tier::ReadOnly,
                Some("[ReadConfig") => Tier::ReadConfig,
                Some("[Full") => Tier::Full,
                _ => continue,
            };
            if tier > cap {
                continue;
            }
            tools.insert(
                name,
                ToolInfo {
                    tier,
                    description: desc,
                    schema: Value::Object((*t.input_schema).clone()),
                },
            );
        }
        ToolIndex { tools }
    }

    /// Whether the procedure may call `name`; the error says why not and the kind for the program (`policy`, `tier`).
    pub fn allow(
        &self,
        name: &str,
        cap: Tier,
        all: &[Instrument],
    ) -> Result<Tier, (&'static str, String)> {
        // [@ANCHOR: proc_tool_allow]
        if name.starts_with(NOT_FOR_PROCEDURES) {
            return Err((
                "policy",
                format!("{name}: a procedure cannot start or control procedures"),
            ));
        }
        if let Some(t) = self.tools.get(name) {
            return Ok(t.tier);
        }
        let full = ToolIndex::build(all, Tier::Full);
        match full.tools.get(name) {
            Some(t) => Err((
                "tier",
                format!(
                    "{name} needs tier {}; this run's tier cap is {}",
                    tier_name(t.tier),
                    tier_name(cap)
                ),
            )),
            None => Err((
                "tool",
                format!("{name} is not a tool of this bench (unknown, or above every instrument's tier)"),
            )),
        }
    }

    /// The index as `bench.tools()` returns it: name to tier, description and argument schema.
    pub fn to_json(&self) -> Value {
        // [@ANCHOR: proc_tool_index_json]
        let mut m = serde_json::Map::new();
        for (n, t) in &self.tools {
            m.insert(
                n.clone(),
                json!({"tier": tier_name(t.tier), "description": t.description, "schema": t.schema}),
            );
        }
        Value::Object(m)
    }
}

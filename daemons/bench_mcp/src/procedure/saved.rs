// SPDX-License-Identifier: AGPL-3.0-or-later
//! Saved procedures: named, reusable programs in the managed directory (`saved/<name>/`) with versions, a description and the
//! tier they need. Saving under an existing name adds a version; the last `KEEP_VERSIONS` are kept. The declared tier is checked
//! against the tools the source names as string literals, so a procedure cannot claim a lower tier than it uses.

use super::examples;
use super::tiers::{parse_tier, tier_name, ToolIndex};
use super::{store, Config};
use crate::config::{Instrument, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;

/// How many versions of one procedure are kept.
pub const KEEP_VERSIONS: usize = 20;
/// The largest source of a saved procedure, bytes.
pub const MAX_SOURCE: usize = 262_144;

/// A procedure name: 1 to 60 characters of `a-z`, `0-9`, `_` and `-`, starting with a letter or digit. No path can be made of it.
pub fn valid_name(name: &str) -> bool {
    // [@ANCHOR: proc_saved_valid_name]
    let ok_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
    let first = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    first && name.len() <= 60 && name.chars().all(ok_char)
}

fn dir_of(cfg: &Config, name: &str) -> Result<PathBuf, String> {
    // [@ANCHOR: proc_saved_dir_of]
    if name.starts_with(examples::PREFIX) {
        return Err(format!("{name:?} is a shipped example: it cannot be saved over or deleted (save a copy under another name)"));
    }
    if !valid_name(name) {
        return Err(format!("{name:?} is not a procedure name (1 to 60 characters of a-z, 0-9, _ and -, starting with a letter or digit)"));
    }
    Ok(store::root(cfg).join("saved").join(name))
}

/// Saves a procedure, `what` being (name, source, description, declared tier), as the next version of `name`. The syntax is checked by the backend; the declared `tier` must be at least the
/// tier of every tool the source names as a literal. Returns the version, the tier and the tools found.
pub fn save(
    cfg: &Config,
    insts: &[Instrument],
    backend: &dyn super::backend::Backend,
    what: (&str, &str, &str, &str),
    note: &str,
) -> Result<Value, String> {
    // [@ANCHOR: proc_saved_save]
    let (name, source, description, tier) = what;
    let dir = dir_of(cfg, name)?;
    let declared = parse_tier(tier)?;
    if source.trim().is_empty() || source.len() > MAX_SOURCE {
        return Err("source must be 1 byte to 256 KiB of program text".into());
    }
    let tmp = store::root(cfg).join("check");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let checked = backend.check(source, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    let checked = checked?;
    if checked["ok"] != json!(true) {
        return Err(format!(
            "the source does not compile: {} (line {})",
            checked["error"].as_str().unwrap_or("?"),
            checked["line"]
        ));
    }
    let index = ToolIndex::build(insts, Tier::Full);
    let mut calls = Vec::new();
    for n in checked["calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n.as_str())
    {
        match index.allow(n, Tier::Full, insts) {
            Ok(t) if t > declared => {
                return Err(format!(
                    "the source calls {n}, which needs tier {}, but the declared tier is {}: declare {} or change the source",
                    tier_name(t),
                    tier_name(declared),
                    tier_name(t)
                ))
            }
            Ok(t) => calls.push(json!({"tool": n, "tier": tier_name(t)})),
            Err((_, why)) => calls.push(json!({"tool": n, "unavailable": why})),
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut meta = store::read_json(&dir.join("meta.json"))
        .unwrap_or_else(|_| json!({"name": name, "versions": []}));
    let versions = meta["versions"].as_array().cloned().unwrap_or_default();
    let version = versions
        .iter()
        .filter_map(|v| v["version"].as_u64())
        .max()
        .unwrap_or(0)
        + 1;
    std::fs::write(dir.join(format!("v{version}.py")), source).map_err(|e| e.to_string())?;
    let mut versions = versions;
    versions.push(json!({"version": version, "saved_unix": store::now_unix(), "bytes": source.len(), "tier": tier, "note": note}));
    while versions.len() > KEEP_VERSIONS {
        let old = versions.remove(0);
        let _ =
            std::fs::remove_file(dir.join(format!("v{}.py", old["version"].as_u64().unwrap_or(0))));
    }
    meta["versions"] = Value::Array(versions);
    if !description.is_empty() || meta["description"].is_null() {
        meta["description"] = json!(description);
    }
    meta["tier"] = json!(tier);
    store::write_json(&dir.join("meta.json"), &meta)?;
    Ok(json!({"name": name, "version": version, "tier": tier, "tool_calls": calls}))
}

/// A saved procedure: its description, tier, versions and the source of `version` (the latest when `None`).
pub fn load(cfg: &Config, name: &str, version: Option<u64>) -> Result<Value, String> {
    // [@ANCHOR: proc_saved_load]
    if let Some(e) = examples::find(name) {
        if version.is_some_and(|v| v != 1) {
            return Err(format!("{name} is a shipped example with one version"));
        }
        return Ok(
            json!({"name": name, "version": 1, "tier": e.tier, "description": e.description, "shipped": true,
            "versions": [{"version": 1, "tier": e.tier, "note": "shipped with the server"}], "source": e.source}),
        );
    }
    let dir = dir_of(cfg, name)?;
    let meta = store::read_json(&dir.join("meta.json"))
        .map_err(|_| format!("no saved procedure {name:?} (see bench_proc_list)"))?;
    let versions: Vec<u64> = meta["versions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["version"].as_u64())
        .collect();
    let version = match version {
        Some(v) if versions.contains(&v) => v,
        Some(v) => return Err(format!("{name} has no version {v} (it has {versions:?})")),
        None => *versions
            .iter()
            .max()
            .ok_or("the procedure has no versions")?,
    };
    let source =
        std::fs::read_to_string(dir.join(format!("v{version}.py"))).map_err(|e| e.to_string())?;
    let tier = meta["versions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["version"] == json!(version))
        .map(|v| v["tier"].clone())
        .unwrap_or(meta["tier"].clone());
    Ok(
        json!({"name": name, "version": version, "tier": tier, "description": meta["description"], "versions": meta["versions"], "source": source}),
    )
}

/// Deletes one version, or (without `version`) the whole procedure.
pub fn delete(cfg: &Config, name: &str, version: Option<u64>) -> Result<Value, String> {
    // [@ANCHOR: proc_saved_delete]
    let dir = dir_of(cfg, name)?;
    let mut meta = store::read_json(&dir.join("meta.json"))
        .map_err(|_| format!("no saved procedure {name:?}"))?;
    let Some(v) = version else {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        return Ok(json!({"name": name, "deleted": "all versions"}));
    };
    let mut versions = meta["versions"].as_array().cloned().unwrap_or_default();
    let before = versions.len();
    versions.retain(|x| x["version"] != json!(v));
    if versions.len() == before {
        return Err(format!("{name} has no version {v}"));
    }
    let _ = std::fs::remove_file(dir.join(format!("v{v}.py")));
    if versions.is_empty() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        return Ok(json!({"name": name, "deleted": "the last version, so the procedure is gone"}));
    }
    meta["versions"] = Value::Array(versions);
    store::write_json(&dir.join("meta.json"), &meta)?;
    Ok(json!({"name": name, "deleted": format!("version {v}")}))
}

/// The saved procedures: name, description, tier, latest version and how many versions.
pub fn list(cfg: &Config) -> Vec<Value> {
    // [@ANCHOR: proc_saved_list]
    let shipped = examples::SHIPPED.iter().map(|e| json!({"name": e.name, "description": e.description, "tier": e.tier, "latest": 1, "versions": 1, "shipped": true}));
    let Ok(rd) = std::fs::read_dir(store::root(cfg).join("saved")) else {
        return shipped.collect();
    };
    let mut out: Vec<Value> = rd
        .flatten()
        .filter_map(|e| store::read_json(&e.path().join("meta.json")).ok())
        .map(|m| {
            let latest = m["versions"].as_array().into_iter().flatten().filter_map(|v| v["version"].as_u64()).max();
            json!({"name": m["name"], "description": m["description"], "tier": m["tier"], "latest": latest, "versions": m["versions"].as_array().map(|a| a.len())})
        })
        .collect();
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    out.extend(shipped);
    out
}

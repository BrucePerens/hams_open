// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rigs radioctl knows and what one of them can do, read from the rig description files (`rigs/*.toml` of radioctl; the directory is
//! the config's `rigs_dir` or `RADIOCTL_RIGS`). Only the identity and the capability list are read here, with a small line reader: the
//! daemon itself validates a description completely when it starts. Nothing here needs a rig or a daemon.

use serde_json::{json, Value};
use std::path::Path;

// [@ANCHOR: bench_mcp_radioctl_models_rig]
/// What the head of a description file says about the rig.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rig {
    pub id: String,
    pub name: String,
    pub family: String,
    pub capabilities: Vec<String>,
}

fn quoted(s: &str) -> Vec<String> {
    s.split('"').skip(1).step_by(2).map(String::from).collect()
}

/// Reads the id, name, family and capabilities of a description text.
pub fn parse_description(text: &str) -> Result<Rig, String> {
    // [@ANCHOR: radioctl_models_parse]
    let mut rig = Rig::default();
    let mut section = String::new();
    let mut in_caps = false;
    for raw in text.lines() {
        let l = raw.split('#').next().unwrap_or("").trim();
        if in_caps {
            rig.capabilities.extend(quoted(l));
            in_caps = !l.contains(']');
            continue;
        }
        if l.starts_with('[') {
            section = l.to_string();
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match (section.as_str(), k) {
            ("", "capabilities") => {
                rig.capabilities.extend(quoted(v));
                in_caps = !v.contains(']');
            }
            ("[rig]", "id") => rig.id = quoted(v).concat(),
            ("[rig]", "name") => rig.name = quoted(v).concat(),
            ("[rig]", "family") => rig.family = quoted(v).concat(),
            _ => {}
        }
    }
    if rig.id.is_empty() || rig.name.is_empty() {
        return Err("no [rig] id and name in the description".into());
    }
    Ok(rig)
}

/// The display name of the rig a description file describes (for the diagnose tool).
pub fn description_name(path: &Path) -> Result<String, String> {
    // [@ANCHOR: radioctl_models_description_name]
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read the rig description {}: {e}", path.display()))?;
    parse_description(&text)
        .map(|r| r.name)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Every description in `dir`, sorted by id. A file that does not parse is skipped.
pub fn list(dir: &Path) -> Result<Vec<Rig>, String> {
    // [@ANCHOR: radioctl_models_list]
    let rd = std::fs::read_dir(dir).map_err(|e| {
        format!(
            "cannot read the rig description directory {}: {e}",
            dir.display()
        )
    })?;
    let mut out: Vec<Rig> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|t| parse_description(&t).ok())
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    if out.is_empty() {
        return Err(format!("no rig descriptions in {}", dir.display()));
    }
    Ok(out)
}

/// `bench_radioctl_models`: the list (filtered by `search`, at most `limit`), or what one `rig` can do.
pub fn tool(dir: &Path, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: radioctl_models_tool]
    let all = list(dir)?;
    if let Some(id) = args.get("rig").and_then(|r| r.as_str()) {
        let r = all.iter().find(|r| r.id == id).ok_or_else(|| {
            format!("no rig description {id:?}. Next: search the list with bench_radioctl_models and the search argument.")
        })?;
        return Ok(json!({
            "rig": r.id, "name": r.name, "family": r.family, "capabilities": r.capabilities,
            "note": "This is what radioctl's description of the rig declares (see its verification level in the description), not what a particular unit does; a rig answers -11 for what its firmware lacks.",
        }));
    }
    let limit = match args.get("limit") {
        None | Some(Value::Null) => 50,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=500).contains(n))
            .ok_or("limit must be a whole number from 1 to 500")? as usize,
    };
    let search = args
        .get("search")
        .and_then(|s| s.as_str())
        .map(|s| s.to_ascii_lowercase());
    let hits: Vec<&Rig> = all
        .iter()
        .filter(|r| {
            search.as_ref().is_none_or(|s| {
                format!("{} {} {}", r.id, r.name, r.family)
                    .to_ascii_lowercase()
                    .contains(s.as_str())
            })
        })
        .collect();
    Ok(json!({
        "total_rigs": all.len(), "matching": hits.len(), "shown": hits.len().min(limit),
        "rigs": hits.iter().take(limit).map(|r| json!({"rig": r.id, "name": r.name, "family": r.family})).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rig descriptions of radioctl in this repository.
    pub fn rigs() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../radioctl/rigs")
    }

    // Tests [@ANCHOR: radioctl_models_parse]
    #[test]
    fn a_description_head_gives_id_name_family_and_capabilities() {
        let kx3 = std::fs::read_to_string(rigs().join("elecraft-kx3.toml")).unwrap();
        let r = parse_description(&kx3).unwrap();
        assert_eq!(
            (r.id.as_str(), r.name.as_str(), r.family.as_str()),
            ("elecraft-kx3", "Elecraft KX3", "elecraft-ascii")
        );
        assert!(r.capabilities.contains(&"read-signal".to_string()));
        assert!(
            r.capabilities.contains(&"identify".to_string()),
            "a capability on the second line of the array"
        );
        assert!(parse_description("nothing").is_err());
    }

    // Tests [@ANCHOR: radioctl_models_description_name]
    #[test]
    fn the_name_of_a_description_file_is_read_and_a_missing_file_is_an_error() {
        assert_eq!(
            description_name(&rigs().join("elecraft-kx3.toml")).unwrap(),
            "Elecraft KX3"
        );
        assert!(description_name(&rigs().join("no-such-rig.toml")).is_err());
    }

    // Tests [@ANCHOR: radioctl_models_list]
    // Tests [@ANCHOR: radioctl_models_tool]
    #[test]
    fn the_tool_searches_the_list_and_describes_one_rig() {
        let v = tool(&rigs(), &json!({"search": "kx3"})).unwrap();
        assert_eq!(v["matching"], 1);
        assert_eq!(v["rigs"][0]["rig"], "elecraft-kx3");
        assert!(v["total_rigs"].as_u64().unwrap() > 200);
        let v = tool(&rigs(), &json!({"rig": "elecraft-kx3"})).unwrap();
        assert!(v["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "split"));
        assert!(tool(&rigs(), &json!({"rig": "nope"})).is_err());
        assert!(tool(&rigs(), &json!({"limit": 0})).is_err());
        assert!(tool(std::path::Path::new("/nonexistent-rigs"), &json!({})).is_err());
        let e = tool(&rigs(), &json!({"limit": 2})).unwrap();
        assert_eq!(e["shown"], 2);
    }
}

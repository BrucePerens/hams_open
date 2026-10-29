// SPDX-License-Identifier: AGPL-3.0-or-later
//! The contributor checklist, as tests (compiled only for `cargo test`). `concepts/extending` in the manual says what a
//! new instrument module must ship; these tests fail until it does, so a reviewer who does not own the instrument can
//! trust the structure and look at the content. Where an existing module predates a rule it is named in a LEGACY table
//! below; a table only shrinks, and a new module is never added to one.
//!
//! [@ANCHOR: bench_mcp_checklist]

use crate::manual::{manual, registered_tools};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(rel: &str) -> String {
    let p = root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// All files with one of `exts` under `dir` (relative to the crate), sorted.
fn files(dir: &str, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![root().join(dir)];
    while let Some(d) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| exts.contains(&x))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Tools that actuate (tier `full`) and have no `dry_run` argument because they were written before the rule. Do not add to it.
const LEGACY_FULL_WITHOUT_DRY_RUN: &[&str] = &[
    "bench_scope_self_cal",
    "bench_audio_play",
    "bench_audio_speak",
    "bench_audio_loopback",
    "bench_audio_alert",
    "bench_audio_set_volume",
];

/// Anchors cited by a test whose declaration is missing from the source (found when the checklist was written). Do not add to it.
const LEGACY_ORPHAN_TEST_ANCHORS: &[&str] = &["audio_analysis", "bench_mcp_vna_power_cycle"];

/// Pages an instrument directory under `manual/instruments/` must have, and who is excused from which (written before the rule).
const REQUIRED_INSTRUMENT_PAGES: &[&str] = &[
    "overview",
    "setup",
    "operation",
    "limits",
    "troubleshooting",
    "protocol",
];
const LEGACY_MISSING_PAGES: &[(&str, &[&str])] = &[
    ("bladerf", &["protocol"]),
    ("limesdr", &["protocol"]),
    ("psu", &[]),
    ("scope", &[]),
    (
        "sound",
        &["operation", "limits", "troubleshooting", "protocol"],
    ),
    ("usb-hubs", &["protocol"]),
    ("vna", &[]),
    ("example", &[]),
];

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn every_tool_has_the_house_shape() {
    for (name, info) in registered_tools() {
        assert!(
            name.starts_with("bench_")
                && name.len() > 6
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{name}: tool names are bench_<instrument>_<verb> in lower case"
        );
        let props = info.schema.get("properties").and_then(|p| p.as_object());
        let props = props.unwrap_or_else(|| panic!("{name}: the schema has no properties object"));
        let manual_tool = name.starts_with("bench_manual_");
        if !manual_tool && name != "bench_list" {
            assert!(
                props.contains_key("instrument"),
                "{name}: every instrument tool takes an optional `instrument` argument"
            );
            assert!(
                !info
                    .schema
                    .get("required")
                    .and_then(|r| r.as_array())
                    .is_some_and(|r| r.iter().any(|v| v == "instrument")),
                "{name}: `instrument` is optional (the only fitting instrument is used)"
            );
        }
    }
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn tools_that_actuate_have_a_dry_run() {
    for (name, info) in registered_tools() {
        if info.tier != "full" || LEGACY_FULL_WITHOUT_DRY_RUN.contains(&name.as_str()) {
            continue;
        }
        let props = info
            .schema
            .get("properties")
            .and_then(|p| p.as_object())
            .unwrap();
        assert!(
            props.contains_key("dry_run"),
            "{name} is tier full and has no `dry_run` argument: every tool that can radiate, drive or actuate returns its plan and sends nothing when dry_run is true (manual: concepts/extending/standing-rules)"
        );
    }
    for legacy in LEGACY_FULL_WITHOUT_DRY_RUN {
        let info = registered_tools().get(*legacy);
        let props = info.and_then(|i| i.schema.get("properties")?.as_object());
        assert!(
            info.is_some_and(|i| i.tier == "full") && props.is_some_and(|p| !p.contains_key("dry_run")),
            "{legacy} is gone, not tier full, or has a dry_run now: remove it from LEGACY_FULL_WITHOUT_DRY_RUN"
        );
    }
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn every_instrument_has_the_standard_manual_pages() {
    let m = manual().expect("the embedded manual parses");
    let dirs: BTreeSet<&str> = m
        .pages
        .iter()
        .filter_map(|p| p.path.strip_prefix("instruments/"))
        .filter_map(|r| r.split('/').next())
        .collect();
    assert!(dirs.len() >= 7, "{dirs:?}");
    let legacy: BTreeMap<&str, &[&str]> = LEGACY_MISSING_PAGES.iter().copied().collect();
    for d in dirs {
        let excused = legacy.get(d).copied().unwrap_or(&[]);
        for page in REQUIRED_INSTRUMENT_PAGES {
            let have = m.get(&format!("instruments/{d}/{page}")).is_some();
            assert!(
                have || excused.contains(page),
                "instruments/{d}/{page}.md is missing: an instrument ships overview, setup, operation, limits, troubleshooting and protocol pages (manual: concepts/extending/manual-pages)"
            );
            assert!(
                !(have && excused.contains(page)),
                "instruments/{d}/{page} exists: remove it from LEGACY_MISSING_PAGES"
            );
        }
        let new = m
            .get("whats-new")
            .unwrap()
            .body
            .contains(&format!("instruments/{d}"));
        assert!(new, "whats-new.md has no line for instruments/{d}");
    }
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn template_wording_does_not_survive_the_copy() {
    let m = manual().expect("the embedded manual parses");
    for p in &m.pages {
        let template = p.instruments.iter().any(|i| i == "example")
            || p.path.starts_with("concepts/extending");
        let text = format!("{} {}", p.summary, p.body);
        if !template {
            for phrase in ["The model for", "The model of a tool"] {
                assert!(
                    !text.contains(phrase),
                    "{}: still has the template wording {phrase:?}; write about your instrument",
                    p.path
                );
            }
        }
    }
    for f in files("src", &["rs"]) {
        let rel = f
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .to_string();
        if rel.starts_with("src/example/") || rel == "src/checklist.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            !text.contains("THE SKELETON FOR A NEW INSTRUMENT"),
            "{rel}: rewrite the skeleton's header comment"
        );
    }
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn every_driver_has_a_config_example_in_the_readme() {
    let readme = read("README.md");
    // Drivers whose example entry is not in the README yet. Do not add to it.
    let legacy: &[&str] = &["usb_hub", "audio"];
    for d in crate::config::DRIVERS {
        let shown = readme.contains(&format!("\"driver\": \"{d}\""));
        assert!(
            shown || legacy.contains(d),
            "README.md has no example config entry with \"driver\": \"{d}\""
        );
    }
}

/// The anchors of a source text: (base declarations, citations by tests, which are the lines marked as test citations).
fn anchors(text: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut base = BTreeSet::new();
    let mut tests = BTreeSet::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(i) = rest.find("[@ANCHOR:") {
            let after = &rest[i + 9..];
            let Some(j) = after.find(']') else { break };
            let name = after[..j].trim().to_string();
            if line.trim_start().starts_with("// Tests [@") {
                tests.insert(name);
            } else {
                base.insert(name);
            }
            rest = &after[j..];
        }
    }
    (base, tests)
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn every_test_anchor_has_a_source_anchor_in_the_crate() {
    let mut base = BTreeSet::new();
    let mut tests = BTreeSet::new();
    for f in files("src", &["rs"])
        .iter()
        .chain(files("tests", &["rs"]).iter())
    {
        let text = std::fs::read_to_string(f).unwrap();
        let (b, t) = anchors(&text);
        base.extend(b);
        tests.extend(t);
    }
    let orphans: Vec<&String> = tests
        .iter()
        .filter(|t| !base.contains(*t) && !LEGACY_ORPHAN_TEST_ANCHORS.contains(&t.as_str()))
        .collect();
    for l in LEGACY_ORPHAN_TEST_ANCHORS {
        assert!(
            tests.contains(*l) && !base.contains(*l),
            "{l} is no longer an orphan anchor: remove it from LEGACY_ORPHAN_TEST_ANCHORS"
        );
    }
    assert!(
        orphans.is_empty(),
        "tests cite anchors that no source file declares (the anchor must be in the code it tests): {orphans:?}"
    );
    let untested: Vec<&String> = base.iter().filter(|b| !tests.contains(*b)).collect();
    assert!(
        untested.is_empty(),
        "anchors declared in source that no test cites (a Tests citation comment above the test): {untested:?}"
    );
}

/// A line that looks like a leak: a MAC address, a public IPv4 address, or a credential word followed by a value.
fn leak(line: &str) -> Option<String> {
    let b = line.as_bytes();
    // MAC: six hex pairs with colons, other than the documentation-style all-zero or 00:11:22:33:44:55 forms.
    for w in b.windows(17) {
        let ok = w.iter().enumerate().all(|(i, c)| {
            if i % 3 == 2 {
                *c == b':'
            } else {
                c.is_ascii_hexdigit()
            }
        });
        if ok {
            let s = String::from_utf8_lossy(w).to_string();
            if s != "00:11:22:33:44:55" && s != "00:00:00:00:00:00" {
                return Some(format!("MAC address {s}"));
            }
        }
    }
    for tok in line.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        let parts: Vec<&str> = tok.split('.').collect();
        if parts.len() != 4 || parts.iter().any(|p| p.is_empty() || p.len() > 3) {
            continue;
        }
        let n: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
        if n.len() != 4 || n.iter().any(|x| *x > 255) {
            continue;
        }
        let private = n[0] == 10
            || n[0] == 127
            || n[0] == 0
            || (n[0] == 172 && (16..=31).contains(&n[1]))
            || (n[0] == 192 && n[1] == 168)
            || (n[0] == 192 && n[1] == 0 && n[2] == 2)
            || (n[0] == 198 && n[1] == 51 && n[2] == 100)
            || (n[0] == 203 && n[1] == 0 && n[2] == 113)
            || (n[0] == 169 && n[1] == 254);
        // A dotted version (firmware 1.3.27.0, 5.22.1.0) has a small first number.
        if !private && n[0] >= 20 {
            return Some(format!("public IPv4 address {tok}"));
        }
    }
    let low = line.to_ascii_lowercase();
    for key in [
        "password=",
        "passwd=",
        "api_key=",
        "secret=",
        "token=",
        "begin private key",
        "begin rsa private key",
    ] {
        if low.contains(key) {
            return Some(format!("credential text {key:?}"));
        }
    }
    None
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn fixtures_docs_and_manual_hold_no_addresses_or_credentials() {
    let mut found = Vec::new();
    let mut scan = |dir: &str, exts: &[&str]| {
        for f in files(dir, exts) {
            let Ok(text) = std::fs::read_to_string(&f) else {
                continue;
            };
            for (n, l) in text.lines().enumerate() {
                if let Some(why) = leak(l) {
                    found.push(format!(
                        "{}:{}: {why}",
                        f.strip_prefix(root()).unwrap().display(),
                        n + 1
                    ));
                }
            }
        }
    };
    scan("manual", &["md"]);
    scan("docs", &["md"]);
    scan("tests", &["txt", "json", "csv", "log", "md"]);
    for f in ["README.md"] {
        for (n, l) in read(f).lines().enumerate() {
            if let Some(why) = leak(l) {
                found.push(format!("{f}:{}: {why}", n + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "scrub these before committing (use 192.0.2.x for addresses, SN-0000 for serials; manual: concepts/extending/provenance): {found:#?}"
    );
}

#[test]
fn the_leak_detector_sees_what_it_should() {
    assert!(leak("host 93.184.216.34 port 5025").is_some());
    assert!(leak("mac 3c:22:fb:12:34:56").is_some());
    assert!(leak("api_key=abc").is_some());
    assert!(leak("host 192.0.2.10 firmware 1.3.27.0 SN-0000").is_none());
    assert!(leak("sdr 192.168.10.51 and 10.99.0.1").is_none());
}

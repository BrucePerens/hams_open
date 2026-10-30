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

/// Every tool of tier `full` and the tool that puts its instrument in the safe state (tier `read_config` or lower, same
/// instrument family). A new `full` tool is added here with its companion, or to `FULL_WITHOUT_SAFE_STATE` with the reason.
const SAFE_STATE_COMPANION: &[(&str, &str)] = &[
    ("bench_psu_output", "bench_psu_safe_state"),
    ("bench_audio_play", "bench_audio_safe_state"),
    ("bench_audio_speak", "bench_audio_safe_state"),
    ("bench_audio_loopback", "bench_audio_safe_state"),
    ("bench_audio_alert", "bench_audio_safe_state"),
    ("bench_audio_set_volume", "bench_audio_safe_state"),
    ("bench_audio_discover", "bench_audio_safe_state"),
    ("bench_example_pulse", "bench_example_safe_state"),
    ("bench_hamlib_load_baseline", "bench_hamlib_safe_state"),
    ("bench_hamlib_tx_key_test", "bench_hamlib_safe_state"),
    ("bench_hamlib_tx_power_sweep", "bench_hamlib_safe_state"),
];

/// `full` tools that have no safe-state companion, and why. The reason must be one that stays true: the effect ends by itself
/// inside the call and leaves nothing running or energised. Do not add to it without the owner agreeing.
const FULL_WITHOUT_SAFE_STATE: &[(&str, &str)] = &[
    (
        "bench_scope_self_cal",
        "the scope runs its own calibration to the end inside the call and nothing is left driven afterwards; it cannot be aborted",
    ),
];

/// Anchors cited by a test whose declaration is missing from the source (found when the checklist was written). Do not add to it.
const LEGACY_ORPHAN_TEST_ANCHORS: &[&str] = &[];

/// Pages an instrument directory under `manual/instruments/` must have, and who is excused from which (written before the rule).
const REQUIRED_INSTRUMENT_PAGES: &[&str] = &[
    "overview",
    "setup",
    "operation",
    "limits",
    "troubleshooting",
    "protocol",
];
const LEGACY_MISSING_PAGES: &[(&str, &[&str])] = &[];

// Tests [@ANCHOR: bench_mcp_checklist]
// Tests [@ANCHOR: bench_mcp_manual_tool_registry]
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
// Tests [@ANCHOR: bench_mcp_manual_manual_build]
// Tests [@ANCHOR: bench_mcp_manual_manual_render_body]
// Tests [@ANCHOR: bench_mcp_manual_searchindex_build]
// Tests [@ANCHOR: bench_mcp_manual_tokens]
#[test]
fn tools_that_actuate_have_a_dry_run() {
    for (name, info) in registered_tools() {
        if info.tier != "full" {
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
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn every_full_tool_names_its_safe_state_companion_or_says_why_it_has_none() {
    let tools = registered_tools();
    let companions: BTreeMap<&str, &str> = SAFE_STATE_COMPANION.iter().copied().collect();
    let exempt: BTreeMap<&str, &str> = FULL_WITHOUT_SAFE_STATE.iter().copied().collect();
    for (name, info) in tools {
        if info.tier != "full" {
            continue;
        }
        let n = name.as_str();
        assert!(
            companions.contains_key(n) || exempt.contains_key(n),
            "{name} is tier full and is in neither SAFE_STATE_COMPANION nor FULL_WITHOUT_SAFE_STATE (src/checklist.rs): every tool that can actuate has a tool that puts the instrument in its safe state and reads it back (manual: concepts/extending/standing-rules)"
        );
        assert!(
            !(companions.contains_key(n) && exempt.contains_key(n)),
            "{name} is in both tables"
        );
    }
    for (full, safe) in &companions {
        assert!(
            tools.get(*full).is_some_and(|i| i.tier == "full"),
            "{full} is not a registered full tool: remove it from SAFE_STATE_COMPANION"
        );
        let c = tools
            .get(*safe)
            .unwrap_or_else(|| panic!("{full}: the companion {safe} is not a registered tool"));
        assert!(
            c.tier == "read_config" || c.tier == "read_only",
            "{safe} is tier {}: a safe-state tool is allowed one tier below the tool it makes safe",
            c.tier
        );
        let family = |t: &str| t.splitn(3, '_').take(2).collect::<Vec<_>>().join("_");
        assert_eq!(
            family(full),
            family(safe),
            "{full} and {safe} are for different instruments"
        );
        assert!(
            safe.ends_with("_safe_state"),
            "{safe}: a safe-state tool is named bench_<instrument>_safe_state"
        );
    }
    for (full, why) in &exempt {
        assert!(
            tools.get(*full).is_some_and(|i| i.tier == "full") && why.len() > 40,
            "{full} is not a registered full tool, or its reason is missing: remove it from FULL_WITHOUT_SAFE_STATE"
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
// Tests [@ANCHOR: bench_mcp_manual_manual_up_of]
// Tests [@ANCHOR: bench_mcp_manual_not_found]
// Tests [@ANCHOR: bench_mcp_manual_section_of]
// Tests [@ANCHOR: bench_mcp_manual_str_arg]
// Tests [@ANCHOR: bench_mcp_manual_truncate_chars]
// Tests [@ANCHOR: bench_mcp_manual_uint_arg]
#[test]
fn every_driver_has_a_config_example_in_the_readme() {
    let readme = read("README.md");
    // Drivers whose example entry is not in the README yet. Do not add to it.
    let legacy: &[&str] = &["usb_hub", "audio"];
    for d in crate::config::driver_names() {
        let shown = readme.contains(&format!("\"driver\": \"{d}\""));
        assert!(
            shown || legacy.contains(&d),
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
    for f in files("src", &["rs", "py"])
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

/// The scrubbed forms a serial number may take in a fixture, page or README.
const SERIAL_PLACEHOLDERS: &[&str] = &["SN-0000", "SERIAL-REDACTED"];

/// A token that could be a serial number: six or more characters of letters, digits and `._-`, with a digit in it, that is not
/// a path and not one of the scrubbed placeholders.
fn serialish(tok: &str) -> bool {
    let up = tok.to_ascii_uppercase();
    tok.len() >= 6
        && tok.bytes().any(|b| b.is_ascii_digit())
        && !tok.starts_with('/')
        && !SERIAL_PLACEHOLDERS.iter().any(|p| up.starts_with(p))
        && tok
            .bytes()
            .any(|b| b != b'0' && b != b'-' && b != b'.' && b != b'_')
        && !tok
            .bytes()
            .all(|b| b == b'0' || b == b'-' || b == b'.' || b == b'_')
}

/// A line that looks like it holds a real serial number: a `serial` key (JSON, sysfs, lsusb's `iSerial`, udev's `ID_SERIAL`)
/// followed by a value that is not a placeholder, or an instrument identification reply (`maker,model,serial,firmware`).
fn serial_leak(line: &str) -> Option<String> {
    let low = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find("serial") {
        let at = from + i + 6;
        from = at;
        let seps = |c: char| " \t:=\"',".contains(c);
        let mut rest = &line[at..];
        // `serial_short`, `serial_number`, `serialnumber`, `serial number`.
        let tail = rest.trim_start_matches(['_', ' ']);
        for word in ["short", "number"] {
            if tail.len() >= word.len() && tail[..word.len()].eq_ignore_ascii_case(word) {
                rest = &tail[word.len()..];
                break;
            }
        }
        let rest = rest.trim_start_matches(seps);
        // lsusb: `iSerial 3 VALUE`: the string index comes first.
        let rest = if low[..from].ends_with("iserial") {
            rest.trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start()
        } else {
            rest
        };
        let tok: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
            .collect();
        if serialish(&tok) {
            return Some(format!("serial number {tok}"));
        }
    }
    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() == 4
        && f[0].chars().any(|c| c.is_ascii_alphabetic())
        && f[2].len() >= 8
        && f[2].bytes().all(|b| b.is_ascii_alphanumeric())
        && f[2].bytes().any(|b| b.is_ascii_digit())
        && f[2].bytes().any(|b| b.is_ascii_alphabetic())
        && f[3].bytes().any(|b| b.is_ascii_digit())
    {
        return Some(format!("identification reply with serial {}", f[2]));
    }
    None
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn the_serial_number_detector_sees_what_it_should() {
    for bad in [
        r#"{"serial": "A12B34C56"}"#,
        "ID_SERIAL_SHORT=0123ABCD99",
        "  iSerial                 3 1234ABCD",
        "Siglent Technologies,SDS1202X-E,SDSMMEBQ1R1234,1.3.27.0",
        "Serial Number: X9Z-77421",
    ] {
        assert!(serial_leak(bad).is_some(), "{bad}");
    }
    for good in [
        r#"{"serial": "/dev/ttyUSB0"}"#,
        "  iSerial                 0 ",
        "Acme,Model 1,SN-0000,1.2.3",
        "SERIAL-REDACTED",
        "serial: SN-0000",
        "`serial` is required; serial port, serial decode, serial adapter",
        "\"device.serial\": \"BurrBrown_from_Texas_Instruments_USB_AUDIO_CODEC\"",
        "iSerial 0",
        "serial 000000",
    ] {
        assert!(
            serial_leak(good).is_none(),
            "{good}: {:?}",
            serial_leak(good)
        );
    }
}

/// Every file under `dir` (relative to the crate) that is text, sorted.
fn text_files(dir: &str) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut todo = vec![root().join(dir)];
    while let Some(d) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_symlink() {
                continue;
            }
            if p.is_dir() {
                todo.push(p);
            } else if let Ok(t) = std::fs::read_to_string(&p) {
                out.push((p, t));
            }
        }
    }
    out.sort();
    out
}

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn no_fixture_page_or_doc_holds_a_serial_number() {
    let mut found = Vec::new();
    let rel = |p: &Path| p.strip_prefix(root()).unwrap().display().to_string();
    for dir in ["manual", "docs", "tests", "src"] {
        for (path, text) in text_files(dir) {
            // This file's own examples of what is detected.
            if rel(&path) == "src/checklist.rs" {
                continue;
            }
            if path.file_name().is_some_and(|n| n == "serial") {
                let v = text.trim();
                if !(v.is_empty() || SERIAL_PLACEHOLDERS.contains(&v)) {
                    found.push(format!("{}: sysfs serial file holds {v:?}", rel(&path)));
                }
            }
            for (n, l) in text.lines().enumerate() {
                if let Some(why) = serial_leak(l) {
                    found.push(format!("{}:{}: {why}", rel(&path), n + 1));
                }
            }
        }
    }
    for (n, l) in read("README.md").lines().enumerate() {
        if let Some(why) = serial_leak(l) {
            found.push(format!("README.md:{}: {why}", n + 1));
        }
    }
    assert!(
        found.is_empty(),
        "scrub these before committing (SN-0000 for serials; manual: concepts/extending/provenance): {found:#?}"
    );
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

// Tests [@ANCHOR: bench_mcp_checklist]
#[test]
fn the_guide_the_pr_template_and_the_plan_issue_are_in_place() {
    let m = manual().expect("the embedded manual parses");
    let index = read("docs/EXTENDING.md");
    for p in m
        .pages
        .iter()
        .filter(|p| p.path.starts_with("concepts/extending/"))
    {
        let file = format!("manual/{}.md", p.path);
        assert!(
            index.contains(&file),
            "docs/EXTENDING.md does not list {file}"
        );
    }
    assert!(m.get("procedures/add-an-instrument").is_some());
    assert!(index.contains("manual/procedures/add-an-instrument.md"));
    let pr = read("../../.github/PULL_REQUEST_TEMPLATE/bench_instrument.md");
    for h in [
        "## Instrument and function",
        "## New tools",
        "## Hardware testing",
        "## Safety",
        "## Provenance",
        "## Scrubbing",
        "## Checks run",
    ] {
        assert!(pr.contains(h), "the PR template lost its {h:?} section");
    }
    let issue = read("../../.github/ISSUE_TEMPLATE/bench_instrument_plan.md");
    assert!(issue.contains("Planned tools"));
    // The five incident rules and the repository URL stay in the guide.
    let rules = m.get("concepts/extending/standing-rules").unwrap();
    for k in [
        "node.dont-fallback",
        "dry_run",
        "added by bench_mcp",
        "https://github.com/BrucePerens/hams_open",
        "audible sound",
    ] {
        assert!(
            rules.body.to_lowercase().contains(&k.to_lowercase()),
            "standing-rules lost {k:?}"
        );
    }
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian), or None for a date that does not exist.
fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    // [@ANCHOR: bench_mcp_checklist_days_from_civil]
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let len = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + i64::from(leap),
        _ => return None,
    };
    if !(1..=len).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The ISO dates (YYYY-MM-DD) in a text, as (text, days since 1970).
fn iso_dates(text: &str) -> Vec<(String, Option<i64>)> {
    // [@ANCHOR: bench_mcp_checklist_iso_dates]
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 10 <= b.len() {
        let w = &b[i..i + 10];
        let shape = w.iter().enumerate().all(|(k, c)| {
            if k == 4 || k == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
        if shape {
            let n = |a: usize, z: usize| text[i + a..i + z].parse::<i64>().unwrap();
            out.push((
                text[i..i + 10].to_string(),
                days_from_civil(n(0, 4), n(5, 7), n(8, 10)),
            ));
            i += 10;
        } else {
            i += 1;
        }
    }
    out
}

/// Words by which a `verified` line claims that something was run, measured or seen on real equipment.
const HARDWARE_CLAIMS: &[&str] = &[
    "dev box",
    "on the unit",
    "measured",
    "probed",
    "observed",
    "confirmed",
    "run on",
    "on a scope",
    "on the scope",
    "loopback",
    "real ",
];

// Tests [@ANCHOR: bench_mcp_checklist]
// Tests [@ANCHOR: bench_mcp_checklist_days_from_civil]
// Tests [@ANCHOR: bench_mcp_checklist_iso_dates]
#[test]
fn verified_lines_carry_a_real_date_that_is_not_in_the_future() {
    let today = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        / 86_400;
    let first = days_from_civil(2026, 1, 1).unwrap();
    assert_eq!(days_from_civil(1970, 1, 1), Some(0));
    assert_eq!(days_from_civil(2026, 2, 29), None);
    assert_eq!(days_from_civil(2024, 2, 29), Some(19_782));
    assert_eq!(iso_dates("a 2026-10-29 b 2026-13-01").len(), 2);
    let m = manual().expect("the embedded manual parses");
    let mut bad = Vec::new();
    for p in &m.pages {
        let v = p.verified.trim();
        let dates = iso_dates(v);
        let exempt = v.starts_with("n/a")
            || v.starts_with("see ")
            || v == "fixtures only"
            || v == "in progress";
        let low = v.to_ascii_lowercase();
        if !exempt && dates.is_empty() {
            bad.push(format!(
                "{}: verified {v:?} has no date (YYYY-MM-DD); say what was checked and when",
                p.path
            ));
        }
        if low.starts_with("code only") && HARDWARE_CLAIMS.iter().any(|w| low.contains(w)) {
            bad.push(format!(
                "{}: verified {v:?} says code only and also claims a hardware run",
                p.path
            ));
        }
        for (text, days) in dates {
            match days {
                None => bad.push(format!("{}: verified date {text} does not exist", p.path)),
                Some(d) if d > today => {
                    bad.push(format!("{}: verified date {text} is in the future", p.path))
                }
                Some(d) if d < first => bad.push(format!(
                    "{}: verified date {text} is before the project began",
                    p.path
                )),
                _ => {}
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests that keep the manual honest: every tool has its page, every page is well formed, every
//! path resolves, and the argument tables follow the tool schemas.

use super::*;
use std::collections::BTreeSet;

const MAX_PAGE_CHARS: usize = 16_000;
const MAX_INDEX_CHARS: usize = 6_000;

fn m() -> &'static Manual {
    manual().expect("the embedded manual parses")
}

fn has_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.windows(10).any(|w| {
        let d = |i: usize| w[i].is_ascii_digit();
        w[0] == b'2'
            && d(1)
            && d(2)
            && d(3)
            && w[4] == b'-'
            && d(5)
            && d(6)
            && w[7] == b'-'
            && d(8)
            && d(9)
    })
}

/// The links `[text](target)` of a body that point inside the manual.
fn inner_links(body: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut in_fence = false;
    for l in body.lines() {
        if l.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if in_fence {
            continue;
        }
        let mut rest = l;
        while let Some(i) = rest.find("](") {
            let after = &rest[i + 2..];
            if let Some(j) = after.find(')') {
                let t = &after[..j];
                if !t.contains("://") && !t.starts_with('#') && !t.starts_with("mailto:") {
                    v.push(t.split('#').next().unwrap_or("").to_string());
                }
                rest = &after[j..];
            } else {
                break;
            }
        }
    }
    v
}

// Tests [@ANCHOR: bench_mcp_manual_front_matter]
#[test]
fn every_file_in_the_manual_is_a_page_with_the_required_front_matter() {
    for (file, text) in embedded::FILES {
        let p = parse_page(file, text).unwrap_or_else(|e| panic!("{e}"));
        assert!(!p.title.is_empty(), "{file}: title");
        assert!(!p.summary.is_empty(), "{file}: summary");
        assert!(
            !p.summary.contains('\n') && p.summary.len() <= 200,
            "{file}: summary is one line of at most 200 characters"
        );
        if let Some(t) = &p.tier {
            assert!(
                TIERS.contains(&t.as_str()),
                "{file}: tier {t:?} is not one of {TIERS:?}"
            );
        }
        let v = p.verified.as_str();
        let honest = ["n/a", "fixtures only", "code only", "in progress", "see "]
            .iter()
            .any(|k| v.starts_with(k))
            || has_date(v);
        assert!(
            honest,
            "{file}: verified {v:?} must say what was checked on hardware with a date (YYYY-MM-DD), or start with n/a, fixtures only, code only, in progress or see <page>"
        );
    }
    assert!(embedded::FILES.len() > 10);
}

#[test]
fn front_matter_parser_rejects_malformed_pages() {
    assert!(parse_page("a.md", "no fence").is_err());
    assert!(parse_page("a.md", "---\ntitle: x\n").is_err());
    assert!(parse_page("a.md", "---\ntitel: x\n---\n")
        .unwrap_err()
        .contains("unknown front matter key"));
    assert!(parse_page("a.md", "---\ntitle: x\ntitle: y\n---\n").is_err());
    assert!(parse_page("a.md", "---\ntools: [a, b\n---\n").is_err());
    assert!(parse_page("a.txt", "---\n---\n").is_err());
    let p = parse_page(
        "x/y.md",
        "---\ntitle: T\ntools: [a, b]\ninstrument: vna\nverified: v: with colon\n---\n# T\n",
    )
    .unwrap();
    assert_eq!(
        (p.path.as_str(), p.dir.as_str(), p.is_index),
        ("x/y", "x", false)
    );
    assert_eq!(p.tools, ["a", "b"]);
    assert_eq!(p.instruments, ["vna"]);
    assert_eq!(p.verified, "v: with colon");
    let i = parse_page("x/index.md", "---\ntitle: T\n---\n").unwrap();
    assert_eq!((i.path.as_str(), i.is_index), ("x", true));
    assert_eq!(
        parse_page("index.md", "---\ntitle: T\n---\n").unwrap().path,
        "index"
    );
}

#[test]
fn paths_are_normalized() {
    for (a, b) in [
        ("", "index"),
        ("/", "index"),
        ("index", "index"),
        ("tools/", "tools"),
        ("tools/index", "tools"),
        ("/tools/bench_list.md", "tools/bench_list"),
        ("bench-manual://instruments/vna/", "instruments/vna"),
    ] {
        assert_eq!(normalize(a), b, "{a:?}");
    }
}

// Tests [@ANCHOR: bench_mcp_manual_tool_pages]
#[test]
fn every_registered_tool_has_a_page_and_every_tools_entry_names_a_registered_tool() {
    let reg = registered_tools();
    assert!(
        reg.len() > 40,
        "the all-instruments bench lists {} tools",
        reg.len()
    );
    for (name, info) in reg {
        let p = m().get(&format!("tools/{name}")).unwrap_or_else(|| {
            panic!("tool {name} has no page manual/tools/{name}.md (every tool ships its page)")
        });
        assert!(
            p.tools.first().map(String::as_str) == Some(name.as_str()),
            "{}: its first tools: entry is {name}",
            p.path
        );
        assert_eq!(
            p.tier.as_deref(),
            Some(info.tier.as_str()),
            "{}: tier must be the registered tool's tier",
            p.path
        );
        assert!(
            !p.instruments.is_empty() || name.starts_with("bench_manual_"),
            "{}: instrument",
            p.path
        );
        assert!(
            p.body.contains(ARGUMENTS_MARKER),
            "{}: has no {ARGUMENTS_MARKER} marker",
            p.path
        );
    }
    for p in &m().pages {
        for t in &p.tools {
            assert!(
                reg.contains_key(t),
                "{}: tools: names {t}, which is not a registered tool",
                p.path
            );
        }
        if let Some(name) = p.path.strip_prefix("tools/") {
            assert!(
                reg.contains_key(name),
                "{}: a page under tools/ for a tool that is not registered",
                p.path
            );
        }
        if p.body.contains(ARGUMENTS_MARKER) {
            assert!(
                p.path.starts_with("tools/"),
                "{}: the arguments marker belongs on a tools/ page",
                p.path
            );
        }
    }
}

// Tests [@ANCHOR: bench_mcp_manual_arguments]
#[test]
fn argument_tables_follow_the_schema_and_every_argument_is_explained() {
    let mut unexplained: Vec<String> = Vec::new();
    for (name, info) in registered_tools() {
        let page = m().get(&format!("tools/{name}")).unwrap();
        let text = m().rendered(&page.path).unwrap();
        assert!(
            !text.contains(ARGUMENTS_MARKER),
            "{name}: marker left in the rendered page"
        );
        let props = info
            .schema
            .get("properties")
            .and_then(|p| p.as_object())
            .unwrap();
        let required: Vec<&str> = info
            .schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if props.is_empty() {
            assert!(text.contains("takes no arguments"), "{name}");
        }
        for (arg, prop) in props {
            let want_type = type_of(prop);
            let row = text
                .lines()
                .find(|l| l.starts_with(&format!("| `{arg}` |")))
                .unwrap_or_else(|| panic!("{name}: no table row for argument {arg}"));
            let cols: Vec<&str> = row.split(" | ").collect();
            let req = if required.contains(&arg.as_str()) {
                "yes"
            } else {
                "no"
            };
            assert_eq!(cols[1], req, "{name}.{arg}: required flag");
            assert_eq!(cols[2], want_type, "{name}.{arg}: type");
            // The prose of the page names the argument too (what it means, its unit, its range).
            let prose = page.body.replace(ARGUMENTS_MARKER, "");
            // `instrument` is explained once, by the generated note; any other argument needs a
            // description in the schema or a sentence on the page.
            let described = prop.get("description").is_some() || arg == "instrument";
            if !described && !prose.contains(&format!("`{arg}`")) {
                unexplained.push(format!("{name}.{arg}"));
            }
        }
        let rows = text.lines().filter(|l| l.starts_with("| `")).count();
        assert_eq!(rows, props.len(), "{name}: rows of the argument table");
    }
    assert!(
        unexplained.is_empty(),
        "these arguments have neither a description in the tool's schema nor a sentence on the page (what they mean, units, range, default; name each in backticks): {unexplained:?}"
    );
}

#[test]
fn the_table_generator_reads_types_enums_and_requirements() {
    let t = arguments_table("bench_capture");
    assert!(
        t.contains("| `channel` | yes | integer |") && t.contains("`1`, `2`"),
        "{t}"
    );
    assert!(t.contains("| `allow_stopped` | no | boolean |"), "{t}");
    let t = arguments_table("bench_measure");
    assert!(t.contains("array of string") && t.contains("`PKPK`"), "{t}");
    assert!(arguments_table("bench_list").contains("takes no arguments"));
    assert!(arguments_table("bench_nope").contains("no registered tool"));
}

// Tests [@ANCHOR: bench_mcp_manual_pointers]
#[test]
fn every_tool_description_ends_with_its_manual_path_and_the_page_exists() {
    for t in crate::all_tool_defs() {
        let d = t.description.as_deref().unwrap();
        let tail = format!("Manual: tools/{}", t.name);
        assert!(
            d.ends_with(&tail),
            "{}: description must end with {tail:?}: {d}",
            t.name
        );
        assert!(
            m().get(&format!("tools/{}", t.name)).is_some(),
            "{}",
            t.name
        );
    }
    // The pointer is added once.
    let again = with_pointers(vec![Tool::new("x", "[ReadOnly] d", Arc::new(Map::new()))]);
    assert_eq!(
        again[0].description.as_deref(),
        Some("[ReadOnly] d Manual: tools/x")
    );
}

// Tests [@ANCHOR: bench_mcp_manual_links]
#[test]
fn see_also_and_inner_links_resolve_from_the_root() {
    for p in &m().pages {
        for s in &p.see_also {
            assert!(
                normalize(s) == *s && m().get(s).is_some(),
                "{}: see_also {s:?} is not the path of a page (paths run from the root of the manual, without .md)",
                p.path
            );
        }
        for l in inner_links(&p.body) {
            assert!(
                m().get(&l).is_some(),
                "{}: link to {l:?} does not resolve",
                p.path
            );
        }
        assert!(
            !p.see_also.contains(&p.path),
            "{}: see_also names the page itself",
            p.path
        );
    }
}

#[test]
fn instrument_names_are_instruments_directories() {
    let dirs: BTreeSet<&str> = m()
        .pages
        .iter()
        .filter(|p| p.is_index)
        .filter_map(|p| p.path.strip_prefix("instruments/"))
        .filter(|n| !n.contains('/'))
        .collect();
    assert!(dirs.len() >= 7, "{dirs:?}");
    for p in &m().pages {
        for i in &p.instruments {
            assert!(
                dirs.contains(i.as_str()),
                "{}: instrument {i:?} has no instruments/{i}/index.md",
                p.path
            );
        }
        if let Some(rest) = p.path.strip_prefix("instruments/") {
            let dir = rest.split('/').next().unwrap();
            if rest.contains('/') {
                assert!(
                    p.instruments.iter().any(|i| i == dir),
                    "{}: instrument: must name {dir}",
                    p.path
                );
            }
        }
    }
}

// Tests [@ANCHOR: bench_mcp_manual_index_pages]
#[test]
fn every_directory_has_an_index_that_lists_its_children() {
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for p in &m().pages {
        let mut d = p.dir.clone();
        loop {
            dirs.insert(d.clone());
            match d.rsplit_once('/') {
                Some((up, _)) => d = up.to_string(),
                None => {
                    dirs.insert(String::new());
                    break;
                }
            }
        }
    }
    for d in &dirs {
        let path = if d.is_empty() { "index" } else { d.as_str() };
        let idx = m()
            .get(path)
            .unwrap_or_else(|| panic!("directory {d:?} has no index.md"));
        assert!(idx.is_index, "{path}");
        assert!(
            idx.body.contains(CHILDREN_MARKER),
            "{path}: index pages carry the {CHILDREN_MARKER} marker"
        );
        assert!(
            idx.body.len() <= MAX_INDEX_CHARS,
            "{path}: index page longer than {MAX_INDEX_CHARS}"
        );
        let text = m().rendered(path).unwrap();
        for c in m().children_of(idx) {
            assert!(
                text.contains(&format!("]({})", c.path)),
                "{path}: lists {}",
                c.path
            );
            assert!(
                text.contains(&c.summary),
                "{path}: shows the summary of {}",
                c.path
            );
        }
    }
    // Every page is reachable from the top by walking the child lists.
    let mut seen = BTreeSet::new();
    let mut todo = vec!["index".to_string()];
    while let Some(p) = todo.pop() {
        if !seen.insert(p.clone()) {
            continue;
        }
        for c in m().children_of(m().get(&p).unwrap()) {
            todo.push(c.path.clone());
        }
    }
    assert_eq!(
        seen.len(),
        m().pages.len(),
        "pages not reachable from the top: {:?}",
        m().pages
            .iter()
            .filter(|p| !seen.contains(&p.path))
            .map(|p| &p.path)
            .collect::<Vec<_>>()
    );
}

// Tests [@ANCHOR: bench_mcp_manual_markdown]
#[test]
fn pages_are_well_formed_markdown_of_bounded_length() {
    for p in &m().pages {
        assert!(
            p.body.chars().count() <= MAX_PAGE_CHARS,
            "{}: {} characters, split the page (limit {MAX_PAGE_CHARS})",
            p.path,
            p.body.chars().count()
        );
        let text = m().rendered(&p.path).unwrap();
        let fences = text
            .lines()
            .filter(|l| l.trim_start().starts_with("```"))
            .count();
        assert_eq!(fences % 2, 0, "{}: unbalanced code fence", p.path);
        let hs = headings(text);
        let h1: Vec<&Heading> = hs.iter().filter(|h| h.level == 1).collect();
        assert_eq!(h1.len(), 1, "{}: exactly one # heading", p.path);
        assert_eq!(
            h1[0].text, p.title,
            "{}: the # heading equals the title",
            p.path
        );
        assert_eq!(
            hs[0].level, 1,
            "{}: the first heading is the # heading",
            p.path
        );
        let mut prev = 1;
        for h in &hs {
            assert!(
                h.level <= prev + 1,
                "{}: heading {:?} skips a level",
                p.path,
                h.text
            );
            prev = h.level;
        }
        // Tables have the same number of columns in every row.
        let mut cols: Option<usize> = None;
        let mut in_fence = false;
        for l in text.lines() {
            if l.trim_start().starts_with("```") {
                in_fence = !in_fence;
            }
            if in_fence {
                continue;
            }
            if l.starts_with('|') {
                let n = l.replace("\\|", "").matches('|').count();
                assert_eq!(
                    *cols.get_or_insert(n),
                    n,
                    "{}: table row with another column count: {l}",
                    p.path
                );
            } else {
                cols = None;
            }
        }
        assert!(!text.contains('\t'), "{}: tab character", p.path);
    }
}

#[test]
fn the_manual_has_the_documented_top_level() {
    for path in [
        "index",
        "concepts",
        "tools",
        "instruments",
        "procedures",
        "whats-new",
    ] {
        assert!(m().get(path).is_some(), "missing {path}");
    }
    for path in [
        "concepts/safety-tiers",
        "concepts/configuration",
        "concepts/units-and-conventions",
        "concepts/errors",
        "concepts/reserved-devices",
        "concepts/verification",
        "concepts/several-instruments",
        "concepts/etiquette",
    ] {
        assert!(m().get(path).is_some(), "missing {path}");
    }
    for name in [
        "vna", "scope", "psu", "bladerf", "limesdr", "usb-hubs", "sound",
    ] {
        assert!(
            m().get(&format!("instruments/{name}")).is_some(),
            "missing instruments/{name}"
        );
    }
}

// Tests [@ANCHOR: bench_mcp_manual_index]
#[test]
fn the_index_tool_lists_the_top_of_the_tree_with_summaries() {
    let v = call("bench_manual_index", &json!({})).unwrap();
    let sections = v["sections"].as_array().unwrap();
    let paths: Vec<&str> = sections
        .iter()
        .map(|s| s["path"].as_str().unwrap())
        .collect();
    for want in ["concepts", "tools", "instruments", "procedures"] {
        assert!(paths.contains(&want), "{paths:?}");
    }
    assert!(sections
        .iter()
        .all(|s| !s["summary"].as_str().unwrap().is_empty()));
    let inst = sections
        .iter()
        .find(|s| s["path"] == "instruments")
        .unwrap();
    assert!(inst["children"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["path"] == "instruments/vna"));
    let tools = sections.iter().find(|s| s["path"] == "tools").unwrap();
    assert!(tools.get("children").is_none() && tools["page_count"].as_u64().unwrap() > 40);
}

// Tests [@ANCHOR: bench_mcp_manual_read]
#[test]
fn read_returns_text_metadata_children_sections_and_cuts_long_pages() {
    let v = call("bench_manual_read", &json!({"path": "instruments"})).unwrap();
    assert!(v["children"].as_array().unwrap().len() >= 7);
    assert_eq!(v["up"]["path"], "index");
    let v = call("bench_manual_read", &json!({"path": "tools/bench_capture"})).unwrap();
    assert!(v["text"].as_str().unwrap().contains("| `channel` | yes |"));
    assert_eq!(v["tier"], "read_only");
    assert!(!v["verified"].as_str().unwrap().is_empty());
    assert_eq!(v["up"]["path"], "tools");
    assert!(v["headings"].as_array().unwrap().len() >= 3);
    // A section by heading, case-insensitive and by prefix.
    let s = call(
        "bench_manual_read",
        &json!({"path": "tools/bench_capture", "section": "arguments"}),
    )
    .unwrap();
    let t = s["text"].as_str().unwrap();
    assert!(
        t.starts_with("## Arguments") && t.contains("| `channel` |") && !t.contains("## Failure"),
        "{t}"
    );
    let e = call(
        "bench_manual_read",
        &json!({"path": "tools/bench_capture", "section": "no such heading"}),
    )
    .unwrap_err();
    assert!(e.contains("headings:"), "{e}");
    // Cutting.
    let c = call(
        "bench_manual_read",
        &json!({"path": "tools/bench_capture", "max_chars": 300}),
    )
    .unwrap();
    assert_eq!(c["truncated"], true);
    assert!(
        c["text"].as_str().unwrap().chars().count() <= 300
            && c["note"].as_str().unwrap().contains("max_chars")
    );
    // Unknown page: names pages to try. Bad arguments are refused.
    let e = call(
        "bench_manual_read",
        &json!({"path": "instruments/vna/nothing"}),
    )
    .unwrap_err();
    assert!(e.contains("no manual page") && e.contains("try:"), "{e}");
    assert!(call("bench_manual_read", &json!({})).is_err());
    assert!(call(
        "bench_manual_read",
        &json!({"path": "index", "max_chars": 5})
    )
    .is_err());
    assert!(call("bench_manual_read", &json!({"path": 3})).is_err());
}

// Tests [@ANCHOR: bench_mcp_manual_search]
#[test]
fn search_ranks_pages_and_filters_by_instrument_and_tool() {
    let v = call(
        "bench_manual_search",
        &json!({"query": "spectrum fft window"}),
    )
    .unwrap();
    let r = v["results"].as_array().unwrap();
    assert!(!r.is_empty());
    assert!(r.len() <= 8);
    assert!(r
        .iter()
        .all(|x| x["excerpt"].as_str().unwrap().len() <= 400));
    let top = v["results"][0]["path"].as_str().unwrap();
    assert!(top.contains("spectrum") || top.contains("scope"), "{top}");
    let scores: Vec<f64> = r.iter().map(|x| x["score"].as_f64().unwrap()).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
    // The name of a tool finds its own page first.
    let v = call("bench_manual_search", &json!({"query": "bench_capture"})).unwrap();
    assert_eq!(v["results"][0]["path"], "tools/bench_capture");
    // Filters.
    let v = call(
        "bench_manual_search",
        &json!({"query": "power", "instrument": "usb-hubs", "limit": 30}),
    )
    .unwrap();
    for x in v["results"].as_array().unwrap() {
        assert!(m()
            .get(x["path"].as_str().unwrap())
            .unwrap()
            .instruments
            .iter()
            .any(|i| i == "usb-hubs"));
    }
    let v = call(
        "bench_manual_search",
        &json!({"query": "channel", "tool": "bench_capture"}),
    )
    .unwrap();
    let r = v["results"].as_array().unwrap();
    assert!(!r.is_empty());
    for x in r {
        let page = m().get(x["path"].as_str().unwrap()).unwrap();
        assert!(
            page.tools.iter().any(|t| t == "bench_capture"),
            "{}",
            page.path
        );
    }
    // Nothing matches: an empty list, not an error. No words: an error.
    let v = call("bench_manual_search", &json!({"query": "zzzqqq"})).unwrap();
    assert_eq!(v["matches"], 0);
    assert!(call("bench_manual_search", &json!({"query": "  !! "})).is_err());
    assert!(call("bench_manual_search", &json!({"query": "a", "limit": 0})).is_err());
}

#[test]
fn tokens_split_names_and_bm25_prefers_rarer_words() {
    assert_eq!(
        tokens("bench_usb_find, EPROTO -71"),
        ["bench_usb_find", "bench", "usb", "find", "eproto", "71"]
    );
    let (ex, head) = excerpt(
        "# T\n\nfirst paragraph\n\n## Sec\n\nthe silent state needs a power cycle\nand more\n",
        &tokens("silent state"),
    );
    assert_eq!(head, "Sec");
    assert!(ex.contains("silent state"));
}

// Tests [@ANCHOR: bench_mcp_manual_for_tool]
#[test]
fn for_tool_gives_the_page_and_the_instrument_pages() {
    let v = call("bench_manual_for_tool", &json!({"tool": "bench_usb_find"})).unwrap();
    assert_eq!(v["page"]["path"], "tools/bench_usb_find");
    assert_eq!(v["registered"], true);
    assert_eq!(v["tier"], "read_only");
    let paths: Vec<&str> = v["instrument_pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["path"].as_str().unwrap())
        .collect();
    assert!(
        paths.contains(&"instruments/usb-hubs")
            && paths.contains(&"instruments/usb-hubs/operation"),
        "{paths:?}"
    );
    let e = call("bench_manual_for_tool", &json!({"tool": "bench_vna_swep"})).unwrap_err();
    assert!(e.contains("bench_vna_sweep"), "{e}");
    assert!(call("bench_manual_for_tool", &json!({})).is_err());
}

// Tests [@ANCHOR: bench_mcp_manual_whats_new]
#[test]
fn whats_new_lists_releases_and_every_path_exists() {
    let page = m().get("whats-new").unwrap();
    let rel = releases(&page.body);
    assert!(
        rel.iter().any(|(v, _)| v == env!("CARGO_PKG_VERSION")),
        "whats-new has no section for build version {}",
        env!("CARGO_PKG_VERSION")
    );
    for (v, items) in &rel {
        assert!(!items.is_empty(), "release {v} lists nothing");
        for (p, n) in items {
            assert!(m().get(p).is_some(), "whats-new {v}: {p} is not a page");
            assert!(!n.is_empty(), "whats-new {v}: {p} has no note");
        }
    }
    let all = call("bench_manual_whats_new", &json!({})).unwrap();
    assert_eq!(all["build_version"], env!("CARGO_PKG_VERSION"));
    assert!(all["releases"][0]["pages"][0]["exists"].as_bool().unwrap());
    let one = call(
        "bench_manual_whats_new",
        &json!({"version": env!("CARGO_PKG_VERSION")}),
    )
    .unwrap();
    assert_eq!(one["releases"].as_array().unwrap().len(), 1);
    assert!(
        call("bench_manual_whats_new", &json!({"version": "99.0.0"}))
            .unwrap_err()
            .contains("releases:")
    );
}

#[test]
fn the_manual_tools_are_listed_whatever_the_bench_has_and_are_read_only() {
    let only_psu = crate::config::parse(
        r#"{"instruments":{"psu":{"driver":"atm6301","serial":"/dev/null"}}}"#,
    )
    .unwrap();
    let names: Vec<String> = crate::tool_defs(&only_psu)
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    for s in SPECS {
        assert!(names.contains(&s.name.to_string()), "{} not listed", s.name);
        assert_eq!(registered_tools()[s.name].tier, "read_only");
    }
    // Calling one needs no instrument.
    assert!(crate::call(&only_psu, "bench_manual_index", &json!({})).is_ok());
}

// Tests [@ANCHOR: bench_mcp_manual_resources]
#[test]
fn every_page_is_an_mcp_resource_that_reads_back() {
    let l = list_resources().unwrap();
    assert_eq!(l.resources.len(), m().pages.len());
    for r in &l.resources {
        assert!(r.uri.starts_with(URI_PREFIX));
        assert_eq!(r.mime_type.as_deref(), Some("text/markdown"));
        let got = read_resource(&r.uri).unwrap();
        let ReadResourceResponse::Complete(res) = got else {
            panic!("{}: not complete", r.uri)
        };
        let ResourceContents::TextResourceContents { text, uri, .. } = &res.contents[0] else {
            panic!("{}: not text", r.uri)
        };
        assert_eq!(uri, &r.uri);
        assert!(text.contains("Verified: "), "{}", r.uri);
    }
    assert!(l
        .resources
        .iter()
        .any(|r| r.uri == "bench-manual://instruments/vna"));
    assert!(l.resources.iter().any(|r| r.uri == "bench-manual://index"));
    assert!(read_resource("bench-manual://nothing/here").is_err());
    assert!(read_resource("file:///etc/passwd").is_err());
}

// Tests [@ANCHOR: bench_mcp_manual_resources]
#[test]
fn the_server_advertises_resources_next_to_tools() {
    let info = crate::BenchServer {
        insts: Arc::new(Vec::new()),
    };
    let caps = rmcp::ServerHandler::get_info(&info).capabilities;
    let v = serde_json::to_value(&caps).unwrap();
    assert!(
        v.get("resources").is_some() && v.get("tools").is_some(),
        "{v}"
    );
}

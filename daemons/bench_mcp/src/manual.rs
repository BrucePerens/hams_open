// SPDX-License-Identifier: AGPL-3.0-or-later
//! The manual: a tree of markdown pages under `manual/`, embedded in the binary by `build.rs`, that
//! documents this server and every instrument it drives. The agent reads it through the MCP itself:
//! the `bench_manual_*` tools (the primary path) and the same pages as MCP resources
//! (`bench-manual://<path>`; some clients ignore resources).
//!
//! A page is `manual/<path>.md`; the page of a directory is `manual/<dir>/index.md` and has the path
//! `<dir>`; the top page has the path `index`. Front matter (between `---` lines, one `key: value`
//! per line, lists as `[a, b]`): `title`, `summary` (one line), `verified` (required); `see_also`
//! (page paths, always from the root of the manual), `tools` (tool names the page documents),
//! `instrument` (the `instruments/<dir>` names the page is about), `tier` (`read_only`,
//! `read_config` or `full`). Two markers in a body are replaced when a page is read:
//! `<!-- arguments -->` (tool pages: the argument table, generated from the tool's JSON schema, so it
//! cannot drift from the code) and `<!-- children -->` (index pages: the pages below the directory
//! with their one-line summaries). Every tool and instrument function ships its page; the tests at
//! the end of this file fail until it does.

use rmcp::model::{
    ListResourcesResult, ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, Tool,
};
use rmcp::ErrorData as McpError;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/manual_files.rs"));
}

/// The URI scheme of the pages as MCP resources.
pub const URI_PREFIX: &str = "bench-manual://";

const KEYS: &[&str] = &[
    "title",
    "summary",
    "see_also",
    "tools",
    "instrument",
    "tier",
    "verified",
];
const LIST_KEYS: &[&str] = &["see_also", "tools", "instrument"];
const TIERS: &[&str] = &["read_only", "read_config", "full"];
const ARGUMENTS_MARKER: &str = "<!-- arguments -->";
const CHILDREN_MARKER: &str = "<!-- children -->";
const DEFAULT_MAX_CHARS: usize = 24_000;

#[derive(Debug, Clone)]
pub struct Page {
    /// Path from the root without `.md`; a directory's page has the directory's path.
    pub path: String,
    /// Directory the file is in (empty for the root).
    pub dir: String,
    pub is_index: bool,
    pub title: String,
    pub summary: String,
    pub see_also: Vec<String>,
    pub tools: Vec<String>,
    pub instruments: Vec<String>,
    pub tier: Option<String>,
    pub verified: String,
    pub body: String,
}

fn split_front_matter(text: &str) -> Result<(Vec<(String, String)>, String), String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Err("no front matter: the file must start with a --- line".into());
    }
    let mut pairs = Vec::new();
    let mut closed = false;
    for l in lines.by_ref() {
        if l.trim_end() == "---" {
            closed = true;
            break;
        }
        if l.trim().is_empty() {
            continue;
        }
        let (k, v) = l
            .split_once(':')
            .ok_or_else(|| format!("front matter line without a colon: {l:?}"))?;
        pairs.push((k.trim().to_string(), v.trim().to_string()));
    }
    if !closed {
        return Err("front matter is not closed by a --- line".into());
    }
    let body: Vec<&str> = lines.collect();
    Ok((pairs, body.join("\n").trim_start_matches('\n').to_string()))
}

fn parse_list(key: &str, v: &str) -> Result<Vec<String>, String> {
    if let Some(inner) = v.strip_prefix('[') {
        let inner = inner
            .strip_suffix(']')
            .ok_or_else(|| format!("{key}: list without a closing ]"))?;
        return Ok(inner
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect());
    }
    Ok(if v.is_empty() {
        Vec::new()
    } else {
        vec![v.to_string()]
    })
}

/// The page path of a file name relative to `manual/`.
fn path_of_file(file: &str) -> Result<(String, String, bool), String> {
    let stem = file
        .strip_suffix(".md")
        .ok_or_else(|| format!("{file}: only .md files belong in the manual"))?;
    let (dir, name) = match stem.rsplit_once('/') {
        Some((d, n)) => (d.to_string(), n),
        None => (String::new(), stem),
    };
    if name == "index" {
        let path = if dir.is_empty() {
            "index".to_string()
        } else {
            dir.clone()
        };
        return Ok((path, dir, true));
    }
    Ok((stem.to_string(), dir, false))
}

pub fn parse_page(file: &str, text: &str) -> Result<Page, String> {
    // [@ANCHOR: bench_mcp_manual_front_matter]
    let (path, dir, is_index) = path_of_file(file)?;
    let (pairs, body) = split_front_matter(text).map_err(|e| format!("{file}: {e}"))?;
    let mut map: HashMap<String, String> = HashMap::new();
    for (k, v) in pairs {
        if !KEYS.contains(&k.as_str()) {
            return Err(format!("{file}: unknown front matter key {k:?}"));
        }
        if map.insert(k.clone(), v).is_some() {
            return Err(format!("{file}: front matter key {k:?} twice"));
        }
    }
    if let Some(t) = map.get("tier") {
        if !TIERS.contains(&t.as_str()) {
            return Err(format!("{file}: tier {t:?} is not one of {TIERS:?}"));
        }
    }
    let get = |k: &str| map.get(k).cloned().unwrap_or_default();
    let list = |k: &str| -> Result<Vec<String>, String> {
        debug_assert!(LIST_KEYS.contains(&k));
        parse_list(k, &get(k)).map_err(|e| format!("{file}: {e}"))
    };
    Ok(Page {
        path,
        dir,
        is_index,
        title: get("title"),
        summary: get("summary"),
        see_also: list("see_also")?,
        tools: list("tools")?,
        instruments: list("instrument")?,
        tier: map.get("tier").cloned(),
        verified: get("verified"),
        body,
    })
}

/// The path a caller meant: tolerates a `bench-manual://` prefix, a leading slash, `.md`, a trailing
/// slash and a trailing `/index`; the empty path is the top page.
pub fn normalize(path: &str) -> String {
    // [@ANCHOR: bench_mcp_manual_links]
    let mut p = path.trim();
    p = p.strip_prefix(URI_PREFIX).unwrap_or(p);
    p = p.trim_matches('/');
    p = p.strip_suffix(".md").unwrap_or(p);
    p = p.strip_suffix("/index").unwrap_or(p);
    if p.is_empty() {
        "index".to_string()
    } else {
        p.to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// The tree

pub struct Manual {
    pub pages: Vec<Page>,
    by_path: HashMap<String, usize>,
    rendered: Vec<String>,
    search: SearchIndex,
}

static MANUAL: OnceLock<Result<Manual, String>> = OnceLock::new();

/// The embedded manual, built once.
pub fn manual() -> Result<&'static Manual, String> {
    MANUAL
        .get_or_init(|| Manual::build(embedded::FILES))
        .as_ref()
        .map_err(|e| e.clone())
}

impl Manual {
    pub fn build(files: &[(&str, &str)]) -> Result<Manual, String> {
        // [@ANCHOR: bench_mcp_manual_manual_build]
        let mut pages = Vec::new();
        for (file, text) in files {
            pages.push(parse_page(file, text)?);
        }
        pages.sort_by(|a, b| a.path.cmp(&b.path));
        let mut by_path = HashMap::new();
        for (i, p) in pages.iter().enumerate() {
            if by_path.insert(p.path.clone(), i).is_some() {
                return Err(format!("two pages have the path {}", p.path));
            }
        }
        let mut m = Manual {
            pages,
            by_path,
            rendered: Vec::new(),
            search: SearchIndex::default(),
        };
        m.rendered = (0..m.pages.len()).map(|i| m.render_body(i)).collect();
        m.search = SearchIndex::build(&m);
        Ok(m)
    }

    pub fn get(&self, path: &str) -> Option<&Page> {
        self.by_path.get(&normalize(path)).map(|&i| &self.pages[i])
    }

    fn index_of(&self, path: &str) -> Option<usize> {
        self.by_path.get(&normalize(path)).copied()
    }

    /// The directory whose pages an index page lists.
    fn children_of(&self, page: &Page) -> Vec<&Page> {
        // [@ANCHOR: bench_mcp_manual_index_pages]
        if !page.is_index {
            return Vec::new();
        }
        let d = &page.dir;
        let parent_of = |p: &Page| -> String {
            match p.dir.rsplit_once('/') {
                Some((up, _)) => up.to_string(),
                None => String::new(),
            }
        };
        let mut v: Vec<&Page> = self
            .pages
            .iter()
            .filter(|p| {
                if p.path == page.path {
                    return false;
                }
                (!p.is_index && p.dir == *d)
                    || (p.is_index && !p.dir.is_empty() && parent_of(p) == *d)
            })
            .collect();
        // Sub-directories first, then pages, each by path.
        v.sort_by_key(|p| (!p.is_index, p.path.clone()));
        v
    }

    /// The page that lists this one (the index of its directory).
    fn up_of(&self, page: &Page) -> Option<&Page> {
        // [@ANCHOR: bench_mcp_manual_manual_up_of]
        let dir = if page.is_index {
            match page.dir.rsplit_once('/') {
                Some((up, _)) => up.to_string(),
                None => String::new(),
            }
        } else {
            page.dir.clone()
        };
        if page.path == "index" {
            return None;
        }
        self.get(if dir.is_empty() { "index" } else { &dir })
    }

    fn brief(p: &Page) -> Value {
        json!({"path": p.path, "title": p.title, "summary": p.summary})
    }

    fn render_body(&self, i: usize) -> String {
        // [@ANCHOR: bench_mcp_manual_manual_render_body]
        let p = &self.pages[i];
        let mut text = p.body.clone();
        if text.contains(ARGUMENTS_MARKER) {
            let table = p
                .tools
                .first()
                .map(|t| arguments_table(t))
                .unwrap_or_else(|| "(this page documents no tool)".to_string());
            text = text.replace(ARGUMENTS_MARKER, &table);
        }
        if text.contains(CHILDREN_MARKER) {
            let mut list = String::new();
            for c in self.children_of(p) {
                let kind = if c.is_index { "/" } else { "" };
                list.push_str(&format!(
                    "* [{}]({}){} - {}\n",
                    c.title, c.path, kind, c.summary
                ));
            }
            if list.is_empty() {
                list.push_str("(nothing below this page yet)\n");
            }
            text = text.replace(CHILDREN_MARKER, list.trim_end());
        }
        text
    }

    /// The page text as served: markers replaced.
    pub fn rendered(&self, path: &str) -> Option<&str> {
        self.index_of(path).map(|i| self.rendered[i].as_str())
    }
}

// ---------------------------------------------------------------------------------------------
// Tool schemas (the argument tables)

/// What the registered tools look like: name, tier spelled as in the front matter, schema.
pub struct ToolInfo {
    pub tier: String,
    pub schema: Arc<Map<String, Value>>,
}

fn tool_registry() -> &'static BTreeMap<String, ToolInfo> {
    // [@ANCHOR: bench_mcp_manual_tool_registry]
    static REG: OnceLock<BTreeMap<String, ToolInfo>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut m = BTreeMap::new();
        for t in crate::all_tool_defs() {
            let desc = t.description.as_deref().unwrap_or_default();
            let tier = match desc.split_once(']').map(|(a, _)| a) {
                Some("[ReadOnly") => "read_only",
                Some("[ReadConfig") => "read_config",
                Some("[Full") => "full",
                _ => "unknown",
            };
            m.insert(
                t.name.to_string(),
                ToolInfo {
                    tier: tier.to_string(),
                    schema: t.input_schema.clone(),
                },
            );
        }
        m
    })
}

/// Every registered tool, whatever the configuration lists.
pub fn registered_tools() -> &'static BTreeMap<String, ToolInfo> {
    // [@ANCHOR: bench_mcp_manual_tool_pages]
    tool_registry()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn type_of(prop: &Value) -> String {
    match prop.get("type").and_then(|t| t.as_str()) {
        Some("array") => {
            let item = prop.get("items").and_then(|i| i.get("type"));
            format!(
                "array of {}",
                item.and_then(|t| t.as_str()).unwrap_or("any")
            )
        }
        Some(t) => t.to_string(),
        None => "any".to_string(),
    }
}

fn notes_of(prop: &Value) -> String {
    let mut parts = Vec::new();
    let values = |v: &Value| -> Option<String> {
        v.as_array().map(|a| {
            a.iter()
                .map(|x| match x.as_str() {
                    Some(s) => format!("`{s}`"),
                    None => format!("`{x}`"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
    };
    if let Some(e) = prop.get("enum").and_then(values) {
        parts.push(format!("one of {e}"));
    }
    if let Some(e) = prop
        .get("items")
        .and_then(|i| i.get("enum"))
        .and_then(values)
    {
        parts.push(format!("each one of {e}"));
    }
    for k in ["minimum", "maximum", "default"] {
        if let Some(v) = prop.get(k) {
            parts.push(format!("{k} {v}"));
        }
    }
    if let Some(d) = prop.get("description").and_then(|d| d.as_str()) {
        parts.push(d.to_string());
    }
    parts.join("; ")
}

/// The note every table gives for the `instrument` argument that the schemas leave undescribed.
const INSTRUMENT_NOTE: &str = "the alias of the configured instrument (see bench_list); may be left out when exactly one configured instrument fits the tool";

fn notes_for(name: &str, prop: &Value) -> String {
    let n = notes_of(prop);
    if name == "instrument" && n.is_empty() {
        return INSTRUMENT_NOTE.to_string();
    }
    n
}

/// The argument table of a tool, from its JSON schema.
pub fn arguments_table(tool: &str) -> String {
    // [@ANCHOR: bench_mcp_manual_arguments]
    let Some(info) = tool_registry().get(tool) else {
        return format!("(no registered tool named {tool})");
    };
    let props = info.schema.get("properties").and_then(|p| p.as_object());
    let Some(props) = props.filter(|p| !p.is_empty()) else {
        return "This tool takes no arguments.".to_string();
    };
    let required: Vec<&str> = info
        .schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let mut out =
        String::from("| Argument | Required | Type | Values and notes |\n|---|---|---|---|\n");
    for (name, prop) in props {
        out.push_str(&format!(
            "| `{name}` | {} | {} | {} |\n",
            if required.contains(&name.as_str()) {
                "yes"
            } else {
                "no"
            },
            cell(&type_of(prop)),
            cell(&notes_for(name, prop)),
        ));
    }
    out.trim_end().to_string()
}

/// Appends "Manual: tools/<name>" to every tool's description, so the agent finds the page from the
/// tool list. Applied once, to all tools, where the list is assembled.
pub fn with_pointers(tools: Vec<Tool>) -> Vec<Tool> {
    // [@ANCHOR: bench_mcp_manual_pointers]
    tools
        .into_iter()
        .map(|mut t| {
            let d = t.description.as_deref().unwrap_or_default().trim_end();
            let text = format!("{d} Manual: tools/{}", t.name);
            t.description = Some(text.into());
            t
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Search (BM25 over the rendered pages)

#[derive(Default)]
struct SearchIndex {
    docs: Vec<HashMap<String, f32>>,
    lens: Vec<f32>,
    avg_len: f32,
    df: HashMap<String, usize>,
}

/// Lower-case words; a word with underscores (a tool or argument name) is also indexed in parts.
fn tokens(text: &str) -> Vec<String> {
    // [@ANCHOR: bench_mcp_manual_tokens]
    let mut out = Vec::new();
    let lower = text.to_lowercase();
    for w in lower.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        let w = w.trim_matches('_');
        if w.is_empty() {
            continue;
        }
        out.push(w.to_string());
        if w.contains('_') {
            out.extend(w.split('_').filter(|s| !s.is_empty()).map(String::from));
        }
    }
    out
}

impl SearchIndex {
    fn build(m: &Manual) -> SearchIndex {
        // [@ANCHOR: bench_mcp_manual_searchindex_build]
        let mut idx = SearchIndex::default();
        for (i, p) in m.pages.iter().enumerate() {
            let mut tf: HashMap<String, f32> = HashMap::new();
            let mut add = |text: &str, weight: f32| {
                for t in tokens(text) {
                    *tf.entry(t).or_insert(0.0) += weight;
                }
            };
            add(&p.title, 3.0);
            add(&p.summary, 2.0);
            add(&p.path, 2.0);
            add(&p.tools.join(" "), 3.0);
            add(&p.instruments.join(" "), 2.0);
            add(&m.rendered[i], 1.0);
            idx.lens.push(tf.values().sum());
            for t in tf.keys() {
                *idx.df.entry(t.clone()).or_insert(0) += 1;
            }
            idx.docs.push(tf);
        }
        idx.avg_len = idx.lens.iter().sum::<f32>() / idx.lens.len().max(1) as f32;
        idx
    }

    fn score(&self, doc: usize, query: &[String]) -> f32 {
        // [@ANCHOR: bench_mcp_manual_searchindex_score]
        const K1: f32 = 1.5;
        const B: f32 = 0.75;
        let n = self.docs.len() as f32;
        let mut s = 0.0;
        for q in query {
            let Some(&tf) = self.docs[doc].get(q) else {
                continue;
            };
            let df = *self.df.get(q).unwrap_or(&0) as f32;
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            let norm = tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * self.lens[doc] / self.avg_len));
            s += idf * norm;
        }
        s
    }
}

/// The paragraph of `text` that matches most of the query words, with the heading it is under.
fn excerpt(text: &str, query: &[String]) -> (String, String) {
    // [@ANCHOR: bench_mcp_manual_excerpt]
    let mut heading = String::new();
    let mut best: (usize, usize, String, String) = (0, usize::MAX, String::new(), String::new());
    let mut in_fence = false;
    let mut para: Vec<&str> = Vec::new();
    let mut para_heading = String::new();
    let mut idx = 0usize;
    let flush = |para: &mut Vec<&str>,
                 para_heading: &str,
                 idx: &mut usize,
                 best: &mut (usize, usize, String, String)| {
        if para.is_empty() {
            return;
        }
        let joined = para.join(" ");
        let words: std::collections::HashSet<String> = tokens(&joined).into_iter().collect();
        let hits = query.iter().filter(|q| words.contains(*q)).count();
        if hits > best.0 {
            *best = (hits, *idx, joined, para_heading.to_string());
        }
        *idx += 1;
        para.clear();
    };
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && l.starts_with('#') {
            flush(&mut para, &para_heading, &mut idx, &mut best);
            heading = l.trim_start_matches('#').trim().to_string();
            continue;
        }
        if l.trim().is_empty() && !in_fence {
            flush(&mut para, &para_heading, &mut idx, &mut best);
            continue;
        }
        if para.is_empty() {
            para_heading = heading.clone();
        }
        para.push(l.trim());
    }
    flush(&mut para, &para_heading, &mut idx, &mut best);
    let (_, _, text, heading) = best;
    (clip_around(&text, query, 320), heading)
}

fn clip_around(text: &str, query: &[String], max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let lower: String = text.to_lowercase();
    let first = query
        .iter()
        .filter_map(|q| lower.find(q.as_str()))
        .min()
        .unwrap_or(0);
    let first_char = text[..first.min(text.len())].chars().count();
    let start = first_char.saturating_sub(60);
    let end = (start + max).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s.insert_str(0, "... ");
    }
    if end < chars.len() {
        s.push_str(" ...");
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Sections

struct Heading {
    level: usize,
    text: String,
    line: usize,
}

fn headings(text: &str) -> Vec<Heading> {
    // [@ANCHOR: bench_mcp_manual_markdown]
    let mut v = Vec::new();
    let mut in_fence = false;
    for (n, l) in text.lines().enumerate() {
        if l.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || !l.starts_with('#') {
            continue;
        }
        let level = l.chars().take_while(|c| *c == '#').count();
        if level <= 6 && l[level..].starts_with(' ') {
            v.push(Heading {
                level,
                text: l[level..].trim().to_string(),
                line: n,
            });
        }
    }
    v
}

/// The text from the heading that matches `want` (exact, then prefix, then substring, ignoring
/// case) to the next heading of the same or a higher level.
fn section_of(text: &str, want: &str) -> Result<String, String> {
    // [@ANCHOR: bench_mcp_manual_section_of]
    let hs = headings(text);
    let w = want.trim().trim_start_matches('#').trim().to_lowercase();
    let pos = hs
        .iter()
        .position(|h| h.text.to_lowercase() == w)
        .or_else(|| {
            hs.iter()
                .position(|h| h.text.to_lowercase().starts_with(&w))
        })
        .or_else(|| hs.iter().position(|h| h.text.to_lowercase().contains(&w)));
    let Some(i) = pos else {
        return Err(format!(
            "no heading matching {want:?}; headings: {}",
            hs.iter()
                .map(|h| h.text.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    };
    let lines: Vec<&str> = text.lines().collect();
    let end = hs[i + 1..]
        .iter()
        .find(|h| h.level <= hs[i].level)
        .map(|h| h.line)
        .unwrap_or(lines.len());
    Ok(lines[hs[i].line..end].join("\n"))
}

fn truncate_chars(text: &str, max: usize) -> (String, bool) {
    // [@ANCHOR: bench_mcp_manual_truncate_chars]
    if text.chars().count() <= max {
        return (text.to_string(), false);
    }
    let cut: String = text.chars().take(max).collect();
    // End at a line break when one is near, so a table row is not cut in two.
    let cut = match cut.rfind('\n') {
        Some(i) if i > max * 3 / 4 => cut[..i].to_string(),
        _ => cut,
    };
    (cut, true)
}

// ---------------------------------------------------------------------------------------------
// The five tools

struct Spec {
    name: &'static str,
    desc: &'static str,
    props: &'static str,
    required: &'static [&'static str],
}

const SPECS: &[Spec] = &[
    Spec {
        name: "bench_manual_index",
        desc: "The top of the manual tree with one-line summaries: how to find things, the concepts, the instruments, the procedures and the count of tool pages. Start here.",
        props: "{}",
        required: &[],
    },
    Spec {
        name: "bench_manual_read",
        desc: "Read one manual page by path (like instruments/vna/operation, or tools/bench_vna_sweep; the path of a directory gives its index page): its text, metadata (tier, what was verified on hardware), the pages it points to and its children. Optionally only one section by heading, and at most max_chars characters.",
        props: r#"{"path":{"type":"string","description":"page path from the root of the manual, without .md; empty or index is the top page"},"section":{"type":"string","description":"a heading of the page (case-insensitive; a prefix or part of the heading is enough)"},"max_chars":{"type":"integer","minimum":200,"description":"default 24000; a longer text is cut and says how much is left"}}"#,
        required: &["path"],
    },
    Spec {
        name: "bench_manual_search",
        desc: "Full-text search of the manual (BM25 ranking). Returns the best pages with the matching excerpt, its section and the path to read. Filter by instrument (an instruments/ directory name such as vna, scope, psu) or by tool name.",
        props: r#"{"query":{"type":"string"},"instrument":{"type":"string","description":"only pages about this instrument (a directory name under instruments/)"},"tool":{"type":"string","description":"only pages that document this tool"},"limit":{"type":"integer","minimum":1,"maximum":30,"description":"default 8"}}"#,
        required: &["query"],
    },
    Spec {
        name: "bench_manual_for_tool",
        desc: "The manual page of one MCP tool and the instrument pages that apply to it (setup, operation, limits, troubleshooting).",
        props: r#"{"tool":{"type":"string","description":"a tool name such as bench_vna_sweep"}}"#,
        required: &["tool"],
    },
    Spec {
        name: "bench_manual_whats_new",
        desc: "The manual pages added or changed in each build version, newest first, with a note per page. Optionally one version.",
        props: r#"{"version":{"type":"string","description":"one build version such as 0.1.0; default all"}}"#,
        required: &[],
    },
];

/// The manual tools, always listed whatever instruments are configured: they are read-only and
/// touch no hardware.
pub fn tool_defs() -> Vec<Tool> {
    SPECS
        .iter()
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[ReadOnly] {}", s.desc), Arc::new(schema))
        })
        .collect()
}

pub fn is_manual_tool(name: &str) -> bool {
    SPECS.iter().any(|s| s.name == name)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    // [@ANCHOR: bench_mcp_manual_str_arg]
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn uint_arg(args: &Value, key: &str, min: u64, max: u64) -> Result<Option<usize>, String> {
    // [@ANCHOR: bench_mcp_manual_uint_arg]
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) if (min..=max).contains(&n) => Ok(Some(n as usize)),
            _ => Err(format!("{key} must be an integer from {min} to {max}")),
        },
    }
}

pub fn call(name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_manual_call]
    let m = manual()?;
    match name {
        "bench_manual_index" => Ok(index(m)),
        "bench_manual_read" => read(m, args),
        "bench_manual_search" => search(m, args),
        "bench_manual_for_tool" => for_tool(m, args),
        "bench_manual_whats_new" => whats_new(m, args),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn index(m: &Manual) -> Value {
    // [@ANCHOR: bench_mcp_manual_index]
    let top = m.get("index").expect("the manual has a top page");
    let sections: Vec<Value> = m
        .children_of(top)
        .into_iter()
        .map(|c| {
            let mut v = Manual::brief(c);
            if c.is_index {
                let kids = m.children_of(c);
                v["page_count"] = json!(kids.len());
                // Instruments are listed by name; the long lists (tools) only by count.
                if c.path != "tools" {
                    v["children"] =
                        json!(kids.iter().map(|k| Manual::brief(k)).collect::<Vec<_>>());
                }
            }
            v
        })
        .collect();
    json!({
        "start": top.summary,
        "read_with": "bench_manual_read {\"path\": \"index\"} for the quickstart; bench_manual_search for a word; bench_manual_for_tool for a tool",
        "pages_total": m.pages.len(),
        "sections": sections,
    })
}

fn read(m: &Manual, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_manual_read]
    let path = str_arg(args, "path")?.ok_or("path is required (use index for the top page)")?;
    let max = uint_arg(args, "max_chars", 200, 1_000_000)?.unwrap_or(DEFAULT_MAX_CHARS);
    let Some(p) = m.get(path) else {
        return Err(not_found(m, path));
    };
    let full = m.rendered(&p.path).expect("a page is rendered");
    let heads: Vec<String> = headings(full).into_iter().map(|h| h.text).collect();
    let text = match str_arg(args, "section")? {
        Some(s) => section_of(full, s)?,
        None => full.to_string(),
    };
    let total = text.chars().count();
    let (text, truncated) = truncate_chars(&text, max);
    let mut out = json!({
        "path": p.path,
        "uri": format!("{URI_PREFIX}{}", p.path),
        "title": p.title,
        "summary": p.summary,
        "instrument": p.instruments,
        "tier": p.tier,
        "tools": p.tools,
        "verified": p.verified,
        "see_also": p.see_also.iter().filter_map(|s| m.get(s)).map(Manual::brief).collect::<Vec<_>>(),
        "children": m.children_of(p).into_iter().map(Manual::brief).collect::<Vec<_>>(),
        "headings": heads,
        "text": text,
        "chars_total": total,
        "truncated": truncated,
    });
    if let Some(up) = m.up_of(p) {
        out["up"] = Manual::brief(up);
    }
    if truncated {
        out["note"] = json!(format!(
            "cut at {max} of {total} characters; read a section by heading, or raise max_chars"
        ));
    }
    Ok(out)
}

fn not_found(m: &Manual, path: &str) -> String {
    // [@ANCHOR: bench_mcp_manual_not_found]
    let want = normalize(path);
    let mut near: Vec<&str> = m
        .pages
        .iter()
        .filter(|p| {
            p.path.contains(&want)
                || want.contains(&p.path)
                || p.path.rsplit('/').next() == want.rsplit('/').next()
        })
        .map(|p| p.path.as_str())
        .take(8)
        .collect();
    if near.is_empty() {
        near = m
            .pages
            .iter()
            .filter(|p| p.is_index)
            .map(|p| p.path.as_str())
            .collect();
    }
    format!("no manual page {path:?}; try: {}", near.join(", "))
}

fn search(m: &Manual, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_manual_search]
    let q = str_arg(args, "query")?.ok_or("query is required")?;
    let query = tokens(q);
    if query.is_empty() {
        return Err("query has no words".into());
    }
    let instrument = str_arg(args, "instrument")?;
    let tool = str_arg(args, "tool")?;
    let limit = uint_arg(args, "limit", 1, 30)?.unwrap_or(8);
    let mut hits: Vec<(f32, usize)> = (0..m.pages.len())
        .filter(|&i| {
            let p = &m.pages[i];
            instrument.is_none_or(|n| p.instruments.iter().any(|x| x == n))
                && tool.is_none_or(|t| p.tools.iter().any(|x| x == t))
        })
        .map(|i| (m.search.score(i, &query), i))
        .filter(|(s, _)| *s > 0.0)
        .collect();
    hits.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then(m.pages[a.1].path.cmp(&m.pages[b.1].path))
    });
    let results: Vec<Value> = hits
        .iter()
        .take(limit)
        .map(|&(s, i)| {
            let p = &m.pages[i];
            let (ex, head) = excerpt(&m.rendered[i], &query);
            json!({
                "path": p.path, "title": p.title, "summary": p.summary,
                "score": (f64::from(s) * 100.0).round() / 100.0,
                "section": head, "excerpt": ex,
            })
        })
        .collect();
    Ok(json!({"query": q, "matches": hits.len(), "results": results}))
}

fn for_tool(m: &Manual, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_manual_for_tool]
    let tool = str_arg(args, "tool")?.ok_or("tool is required")?;
    let reg = registered_tools();
    let page = m.get(&format!("tools/{tool}"));
    let documenting: Vec<&Page> = m
        .pages
        .iter()
        .filter(|p| {
            p.tools.iter().any(|t| t == tool)
                && Some(p.path.as_str()) != page.map(|p| p.path.as_str())
        })
        .collect();
    if page.is_none() && documenting.is_empty() {
        // Registered names that share words with the wrong name (a word counts when one is a prefix of the other).
        let words = |n: &str| n.split('_').map(String::from).collect::<Vec<_>>();
        let want = words(tool);
        let mut ranked: Vec<(usize, &str)> = reg
            .keys()
            .map(|n| {
                let have = words(n);
                let hits = want
                    .iter()
                    .filter(|w| {
                        have.iter().any(|h| {
                            w.len() >= 3
                                && common_prefix(w, h)
                                    >= 3.max(w.len().min(h.len()).saturating_sub(1))
                        })
                    })
                    .count();
                (hits, n.as_str())
            })
            .filter(|(h, _)| *h >= 2)
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        let near: Vec<&str> = ranked.iter().map(|(_, n)| *n).take(6).collect();
        return Err(format!(
            "no manual page for a tool named {tool:?}; registered tools that look similar: {}",
            near.join(", ")
        ));
    }
    let mut instrument_pages: Vec<Value> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut insts: Vec<String> = page.map(|p| p.instruments.clone()).unwrap_or_default();
    for d in &documenting {
        for i in &d.instruments {
            if !insts.contains(i) {
                insts.push(i.clone());
            }
        }
    }
    for i in &insts {
        let dir = format!("instruments/{i}");
        if let Some(idx) = m.get(&dir) {
            if seen.insert(idx.path.clone()) {
                instrument_pages.push(Manual::brief(idx));
            }
            for c in m.children_of(idx) {
                if seen.insert(c.path.clone()) {
                    instrument_pages.push(Manual::brief(c));
                }
            }
        }
    }
    Ok(json!({
        "tool": tool,
        "registered": reg.contains_key(tool),
        "tier": reg.get(tool).map(|t| t.tier.clone()),
        "page": page.map(Manual::brief),
        "also_documented_in": documenting.iter().map(|p| Manual::brief(p)).collect::<Vec<_>>(),
        "instrument_pages": instrument_pages,
        "concepts": ["concepts/safety-tiers", "concepts/errors"],
        "read_with": format!("bench_manual_read {{\"path\": \"tools/{tool}\"}}"),
    }))
}

/// The releases in `whats-new`: `## <version>` sections of `- <path>: <note>` lines.
pub fn releases(body: &str) -> Vec<(String, Vec<(String, String)>)> {
    // [@ANCHOR: bench_mcp_manual_releases]
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for l in body.lines() {
        if let Some(v) = l.strip_prefix("## ") {
            out.push((v.trim().to_string(), Vec::new()));
        } else if let (Some(item), Some(cur)) = (l.trim_start().strip_prefix("- "), out.last_mut())
        {
            let (p, n) = item.split_once(':').unwrap_or((item, ""));
            cur.1.push((p.trim().to_string(), n.trim().to_string()));
        }
    }
    out
}

fn whats_new(m: &Manual, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_manual_whats_new]
    let want = str_arg(args, "version")?;
    let page = m
        .get("whats-new")
        .ok_or("the manual has no whats-new page")?;
    let rel = releases(&page.body);
    let shown: Vec<Value> = rel
        .iter()
        .filter(|(v, _)| want.is_none_or(|w| w == v))
        .map(|(v, items)| {
            json!({"version": v, "pages": items.iter().map(|(p, n)| {
                json!({"path": p, "exists": m.get(p).is_some(), "title": m.get(p).map(|x| x.title.clone()), "note": n})
            }).collect::<Vec<_>>()})
        })
        .collect();
    if let (Some(w), true) = (want, shown.is_empty()) {
        return Err(format!(
            "no release {w:?} in the manual; releases: {}",
            rel.iter()
                .map(|(v, _)| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(json!({"build_version": env!("CARGO_PKG_VERSION"), "releases": shown}))
}

// ---------------------------------------------------------------------------------------------
// MCP resources

fn page_text_for_resource(m: &Manual, p: &Page) -> String {
    let mut head = format!("{}\n", p.title);
    head.push_str(&format!("Summary: {}\n", p.summary));
    if let Some(t) = &p.tier {
        head.push_str(&format!("Tier: {t}\n"));
    }
    head.push_str(&format!("Verified: {}\n\n", p.verified));
    head.push_str(m.rendered(&p.path).unwrap_or_default());
    head
}

/// `resources/list`: every page as `bench-manual://<path>`.
pub fn list_resources() -> Result<ListResourcesResult, McpError> {
    // [@ANCHOR: bench_mcp_manual_resources]
    let m = manual().map_err(|e| McpError::internal_error(e, None))?;
    let items = m
        .pages
        .iter()
        .map(|p| {
            Resource::new(format!("{URI_PREFIX}{}", p.path), p.path.clone())
                .with_title(p.title.clone())
                .with_description(p.summary.clone())
                .with_mime_type("text/markdown")
        })
        .collect();
    Ok(ListResourcesResult::with_all_items(items))
}

/// `resources/read` of one `bench-manual://<path>`.
pub fn read_resource(uri: &str) -> Result<ReadResourceResponse, McpError> {
    let m = manual().map_err(|e| McpError::internal_error(e, None))?;
    let path = uri.strip_prefix(URI_PREFIX).ok_or_else(|| {
        McpError::invalid_params(
            format!("not a manual resource (expected {URI_PREFIX}<path>): {uri}"),
            None,
        )
    })?;
    let Some(p) = m.get(path) else {
        return Err(McpError::resource_not_found(not_found(m, path), None));
    };
    let contents = ResourceContents::TextResourceContents {
        uri: uri.to_string(),
        mime_type: Some("text/markdown".into()),
        text: page_text_for_resource(m, p),
        meta: None,
    };
    Ok(ReadResourceResult::new(vec![contents]).into())
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests;

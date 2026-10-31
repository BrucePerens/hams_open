// SPDX-License-Identifier: AGPL-3.0-or-later
//! The programmer's manual (`docs/manual/`) is checked as code is: the index links every
//! chapter, every relative link and heading anchor resolves, every Rust block is either a
//! verbatim copy of an `examples/` file, a list of signatures that still exist in the source,
//! or marked as pseudo-code, every TOML block that claims to be a rig description, a transcript
//! or a radioctld configuration parses with the production parser, and every example the manual
//! names builds and (apart from the one that needs hardware) runs with the output it shows.
//!
//! When one of these fails after you changed code, the manual page named in the message shows
//! something that is no longer true: update the page, do not weaken the test.
#![allow(clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manual_dir() -> PathBuf {
    workspace().join("docs/manual")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn pages() -> Vec<(String, String)> {
    let mut pages: Vec<(String, String)> = fs::read_dir(manual_dir())
        .expect("docs/manual exists")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .map(|p| {
            let name = p.file_name().expect("name").to_string_lossy().into_owned();
            let text = read(&p);
            (name, text)
        })
        .collect();
    pages.sort();
    pages
}

/// GitHub's heading anchor: lower case, punctuation dropped, spaces to hyphens.
fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn headings(text: &str) -> BTreeSet<String> {
    let mut in_fence = false;
    let mut found = BTreeSet::new();
    for line in text.lines() {
        if line.starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence && line.starts_with('#') {
            found.insert(slug(line.trim_start_matches('#')));
        }
    }
    found
}

/// Link targets of `[text](target)` outside code fences and inline code.
fn links(text: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut found = Vec::new();
    for line in text.lines() {
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            match after.find(')') {
                Some(end) => {
                    found.push(after[..end].to_string());
                    rest = &after[end..];
                }
                None => break,
            }
        }
    }
    found
}

#[test]
fn the_index_links_every_chapter_and_every_link_resolves() {
    let pages = pages();
    let index = pages
        .iter()
        .find(|(name, _)| name == "README.md")
        .expect("docs/manual/README.md");
    let indexed: BTreeSet<String> = links(&index.1)
        .into_iter()
        .map(|l| l.split('#').next().unwrap_or("").to_string())
        .collect();
    for (name, _) in &pages {
        assert!(
            name == "README.md" || indexed.contains(name),
            "docs/manual/README.md does not link {name}"
        );
    }
    let slugs: Vec<(String, BTreeSet<String>)> = pages
        .iter()
        .map(|(name, text)| (name.clone(), headings(text)))
        .collect();
    for (name, text) in &pages {
        for target in links(text) {
            if target.starts_with("http://") || target.starts_with("https://") {
                continue;
            }
            let (path, fragment) = match target.split_once('#') {
                Some((p, f)) => (p, Some(f)),
                None => (target.as_str(), None),
            };
            let resolved = if path.is_empty() {
                manual_dir().join(name)
            } else {
                manual_dir().join(path)
            };
            assert!(
                resolved.exists(),
                "docs/manual/{name}: link {target:?} points to nothing"
            );
            let Some(fragment) = fragment else { continue };
            let file = resolved
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some((_, known)) = slugs.iter().find(|(n, _)| *n == file) {
                assert!(
                    known.contains(fragment),
                    "docs/manual/{name}: link {target:?} names a heading that does not exist"
                );
            }
        }
    }
}

struct Block {
    page: String,
    info: String,
    body: String,
}

fn blocks() -> Vec<Block> {
    let mut found = Vec::new();
    for (page, text) in pages() {
        let mut current: Option<(String, Vec<&str>)> = None;
        for line in text.lines() {
            if let Some(info) = line.strip_prefix("```") {
                match current.take() {
                    None => current = Some((info.trim().to_string(), Vec::new())),
                    Some((info, body)) => found.push(Block {
                        page: page.clone(),
                        info,
                        body: body.join("\n"),
                    }),
                }
            } else if let Some((_, body)) = current.as_mut() {
                body.push(line);
            }
        }
        assert!(current.is_none(), "docs/manual/{page}: unclosed code fence");
    }
    found
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// The library source of every crate with comments and attributes removed and whitespace
/// collapsed, so a signature copied from it matches however rustfmt wrapped it.
fn source_corpus() -> String {
    let mut files = Vec::new();
    rust_files(&workspace().join("crates"), &mut files);
    files.sort();
    let mut corpus = String::new();
    for file in files {
        if file.components().any(|c| c.as_os_str() == "src") {
            for line in read(&file).lines() {
                let trimmed = line.trim_start();
                if !trimmed.starts_with("//") && !trimmed.starts_with("#[") {
                    corpus.push_str(line);
                    corpus.push('\n');
                }
            }
        }
    }
    collapse(&corpus)
}

const ALLOWED_INFO: [&str; 8] = [
    "text",
    "sh",
    "toml fragment",
    "toml rigfile",
    "toml radioctld",
    "toml transcript",
    "rust signatures",
    "rust pseudo",
];

#[test]
fn every_code_block_is_checked_or_marked() {
    let corpus = source_corpus();
    for block in blocks() {
        let Block { page, info, body } = &block;
        if let Some(name) = info.strip_prefix("rust example=") {
            let path = workspace().join(format!("crates/radioctld/examples/{name}.rs"));
            let file = read(&path);
            let without_header = file
                .strip_prefix("// SPDX-License-Identifier: AGPL-3.0-or-later\n")
                .unwrap_or(&file);
            assert_eq!(
                body.trim_end(),
                without_header.trim_end(),
                "docs/manual/{page}: the block for example {name} differs from the file; \
                 copy crates/radioctld/examples/{name}.rs again"
            );
        } else if info == "rust signatures" {
            for item in body.split("\n\n") {
                let lines: Vec<&str> = item
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .collect();
                let wanted = collapse(&lines.join("\n"));
                if wanted.is_empty() {
                    continue;
                }
                assert!(
                    corpus.contains(&wanted),
                    "docs/manual/{page}: this signature is not in the source any more: {wanted}"
                );
            }
        } else if info == "toml rigfile" {
            radioctl_core::rigfile::parse(body)
                .unwrap_or_else(|e| panic!("docs/manual/{page}: rig description: {e}"));
        } else if info == "toml transcript" {
            radioctl_core::transcript::parse(body)
                .unwrap_or_else(|e| panic!("docs/manual/{page}: transcript: {e}"));
        } else if info == "toml radioctld" {
            radioctld::config::Config::parse(body, &workspace().join("rigs"))
                .unwrap_or_else(|e| panic!("docs/manual/{page}: radioctld configuration: {e}"));
        } else {
            assert!(
                ALLOWED_INFO.contains(&info.as_str()),
                "docs/manual/{page}: code block marked {info:?}; use `rust example=NAME`, \
                 `rust signatures`, `rust pseudo`, `toml rigfile|radioctld|transcript|fragment`, \
                 `sh` or `text`"
            );
        }
    }
}

/// Every `--example NAME` the manual mentions names a file that exists.
#[test]
fn examples_named_in_the_manual_exist() {
    let mut named = BTreeSet::new();
    for (_, text) in pages() {
        let mut words = text.split_whitespace().peekable();
        while let Some(word) = words.next() {
            if word == "--example" {
                if let Some(name) = words.peek() {
                    named.insert(
                        name.trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
                            .to_string(),
                    );
                }
            }
        }
    }
    assert!(!named.is_empty());
    for name in named {
        let path = workspace().join(format!("crates/radioctld/examples/{name}.rs"));
        assert!(
            path.exists(),
            "the manual names example {name}, which does not exist"
        );
    }
}

fn example_binary(name: &str) -> PathBuf {
    // target/<profile>/deps/<this test> -> target/<profile>/examples/<name>
    let test = std::env::current_exe().expect("current exe");
    test.parent()
        .and_then(Path::parent)
        .expect("target profile directory")
        .join("examples")
        .join(name)
}

/// Builds the examples once, so that running this test file alone (which builds no examples)
/// never runs a stale binary. Same profile and target directory as the test itself.
fn build_examples_once() {
    static BUILT: std::sync::Once = std::sync::Once::new();
    BUILT.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let release = std::env::current_exe()
            .map(|p| p.components().any(|c| c.as_os_str() == "release"))
            .unwrap_or(false);
        let mut build = Command::new(cargo);
        build
            .args(["build", "-p", "radioctld", "--examples"])
            .current_dir(workspace());
        if release {
            build.arg("--release");
        }
        let status = build.status().expect("cargo build --examples");
        assert!(status.success(), "building the examples failed");
    });
}

fn run_example(name: &str, args: &[&str]) -> String {
    build_examples_once();
    let binary = example_binary(name);
    assert!(binary.exists(), "{} was not built", binary.display());
    let output = Command::new(&binary)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(
        output.status.success(),
        "example {name} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The manual shows each example's output in a `text` block; the lines the example prints must
/// all appear on the page that shows the example (the run itself is deterministic apart from
/// the `source` word of a status change, which the page describes in prose).
fn assert_output_in_manual(page: &str, example: &str, output: &str) {
    let text = read(&manual_dir().join(page));
    for line in output.lines() {
        assert!(
            text.contains(line),
            "docs/manual/{page}: output line of example {example} is not shown: {line:?}"
        );
    }
}

#[test]
fn the_quick_start_example_prints_what_chapter_two_shows() {
    let output = run_example("quickstart", &[]);
    assert_output_in_manual("02-quick-start.md", "quickstart", &output);
}

#[test]
fn the_other_examples_run_and_print_what_their_chapters_show() {
    let events = run_example("events", &[]);
    assert!(events.contains("VFO A changed to 7100000 Hz"));
    assert!(events.contains("callback: rig elecraft-kx3 reported the change of VFO A"));
    let gate = run_example("transmit_gate", &[]);
    assert_output_in_manual("10-safety.md", "transmit_gate", &gate);
    let embedded = run_example("embed_daemon", &[]);
    assert_output_in_manual("09-radioctld.md", "embed_daemon", &embedded);
    let splitter = run_example("splitter_tcp", &[]);
    assert_output_in_manual("08-protocol-splitter.md", "splitter_tcp", &splitter);
    let file = run_example("rig_file", &["icom-ic-7300"]);
    assert!(file.contains("engine, virtual rig and file agree"));
}

/// The bench console, fed a script on a virtual IC-7300: reads, sets, a status change with its
/// source, scope sweeps, and no way to transmit.
#[test]
fn bench_console_reads_sets_scopes_and_cannot_transmit() {
    use std::io::Write;
    use std::process::Stdio;
    build_examples_once();
    let binary = example_binary("bench_console");
    assert!(binary.exists(), "{} was not built", binary.display());
    let mut child = Command::new(&binary)
        .args(["icom-ic-7300", "virtual"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bench_console");
    let script = "freq a\\nsetfreq a 7074000\\nfreq a\\nmode\\nlevel af-gain\\nscope on\\n\
                  frames 2\\nptt\\nsetptt 1\\nkey\\nquit\\n"
        .replace("\\n", "\n");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("script");
    let output = child.wait_with_output().expect("bench_console");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{text}");
    assert!(text.contains("= Frequency(A, Hz(14074000))"), "{text}");
    assert!(text.contains("= ok"), "the set was acknowledged: {text}");
    assert!(text.contains("= Frequency(A, Hz(7074000))"), "{text}");
    assert!(text.contains("= Mode(Usb)"), "{text}");
    assert!(text.contains("= Level(AfGain,"), "{text}");
    assert!(text.contains("scope seq="), "scope sweeps arrive: {text}");
    assert!(text.contains("= Ptt(false)"), "{text}");
    for word in ["setptt", "key"] {
        assert!(
            text.contains(&format!("unknown command \"{word}\"")),
            "the console must have no transmit command ({word}): {text}"
        );
    }
}

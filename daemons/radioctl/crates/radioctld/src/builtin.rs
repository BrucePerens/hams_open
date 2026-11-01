// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rig descriptions inside the binary, and where a program gets its descriptions from.
//!
//! `build.rs` embeds every `rigs/*.toml`, so `radioctl-selftest` works with no side files. The
//! owner may point at a directory instead: `--rigs-dir`, then `RADIOCTL_RIGS`, then the
//! embedded set.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `(file stem, text)` of every embedded description, sorted by stem.
const EMBEDDED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/builtin_rigs.rs"));

/// Where descriptions are read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RigSource {
    /// The set built into this program.
    Embedded,
    /// `*.toml` files in a directory.
    Dir(PathBuf),
}

// [@ANCHOR: radioctld_builtin_embedded_texts]
/// The embedded descriptions as `(label, text)` pairs; the label is `embedded:ID`.
pub fn embedded_texts() -> Vec<(String, String)> {
    EMBEDDED
        .iter()
        .map(|(id, text)| (format!("embedded:{id}"), (*text).to_string()))
        .collect()
}

// [@ANCHOR: radioctld_builtin_choose_source]
/// The source of descriptions: `--rigs-dir` wins, then `RADIOCTL_RIGS` (when not empty), else the
/// embedded set. No guess at `./rigs` or a source checkout: a shipped program does not have one.
pub fn choose_source(flag: Option<&Path>, env: Option<&OsStr>) -> RigSource {
    match (flag, env) {
        (Some(dir), _) => RigSource::Dir(dir.to_path_buf()),
        (None, Some(dir)) if !dir.is_empty() => RigSource::Dir(PathBuf::from(dir)),
        _ => RigSource::Embedded,
    }
}

// [@ANCHOR: radioctld_builtin_load_texts]
/// Every description of `source` as `(label, text)` pairs. A directory is read as `*.toml` files.
pub fn load_texts(source: &RigSource) -> Result<Vec<(String, String)>, String> {
    let RigSource::Dir(dir) = source else {
        return Ok(embedded_texts());
    };
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut texts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            texts.push((path.display().to_string(), text));
        }
    }
    Ok(texts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::rigfile;

    // Tests [@ANCHOR: radioctld_builtin_embedded_texts]
    #[test]
    fn every_embedded_description_parses_and_its_id_is_its_file_stem() {
        let texts = embedded_texts();
        assert!(texts.len() >= 27, "{}", texts.len());
        for (label, text) in texts {
            let loaded = rigfile::parse(&text).expect("parses");
            assert_eq!(format!("embedded:{}", loaded.description.id), label);
        }
    }

    // Tests [@ANCHOR: radioctld_builtin_choose_source]
    #[test]
    fn the_flag_beats_the_environment_beats_the_embedded_set() {
        let (a, b) = (Path::new("/a"), OsStr::new("/b"));
        assert_eq!(choose_source(Some(a), Some(b)), RigSource::Dir("/a".into()));
        assert_eq!(choose_source(None, Some(b)), RigSource::Dir("/b".into()));
        assert_eq!(
            choose_source(None, Some(OsStr::new(""))),
            RigSource::Embedded
        );
        assert_eq!(choose_source(None, None), RigSource::Embedded);
    }

    // Tests [@ANCHOR: radioctld_builtin_load_texts]
    #[test]
    fn a_directory_and_the_embedded_set_give_the_same_descriptions() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
        let mut from_dir = load_texts(&RigSource::Dir(dir)).expect("dir");
        // By text, not by label: a path label (`.../x.toml`) and an embedded label (`embedded:x`)
        // order differently when one id is a prefix of another (`x-9000` and `x-9000-old`).
        from_dir.sort_by(|a, b| a.1.cmp(&b.1));
        let mut embedded = load_texts(&RigSource::Embedded).expect("embedded");
        embedded.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(from_dir.len(), embedded.len());
        assert!(from_dir.iter().zip(&embedded).all(|(d, e)| d.1 == e.1));
        assert!(load_texts(&RigSource::Dir("/nonexistent-rigs".into())).is_err());
    }
}

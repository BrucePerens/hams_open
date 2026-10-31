// SPDX-License-Identifier: AGPL-3.0-or-later
//! Lists `../../rigs/*.toml` (top level only) as `include_str!` entries for `src/builtin.rs`, so
//! every description, including ones added later, is inside the binary with no per-rig code.

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")?;
    let rigs = Path::new(&manifest).join("../../rigs");
    println!("cargo:rerun-if-changed={}", rigs.display());
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&rigs)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    let mut code = String::from("&[\n");
    for name in &names {
        let file = rigs.join(format!("{name}.toml")).display().to_string();
        code.push_str(&format!("    ({name:?}, include_str!({file:?})),\n"));
    }
    code.push_str("]\n");
    let out = std::env::var("OUT_DIR")?;
    std::fs::write(Path::new(&out).join("builtin_rigs.rs"), code)?;
    Ok(())
}

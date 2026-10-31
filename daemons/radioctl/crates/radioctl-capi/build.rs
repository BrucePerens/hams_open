// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generates the C header with cbindgen into `OUT_DIR/radioctl.h`. The checked-in copy is
//! `include/radioctl.h`; a test fails when the two differ, so the header a C programmer reads is
//! always the one the code produces.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join("radioctl.h");
    let config = match cbindgen::Config::from_file(PathBuf::from(&dir).join("cbindgen.toml")) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("cbindgen.toml: {e}");
            std::process::exit(1);
        }
    };
    match cbindgen::Builder::new()
        .with_crate(&dir)
        .with_config(config)
        .generate()
    {
        Ok(bindings) => {
            bindings.write_to_file(&out);
        }
        Err(e) => {
            eprintln!("cbindgen: {e}");
            std::process::exit(1);
        }
    }
}

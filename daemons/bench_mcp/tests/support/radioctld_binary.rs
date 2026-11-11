// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `radioctld` program for tests (included by the unit tests and the integration tests with `#[path]`): `BENCH_MCP_RADIOCTLD` if set,
//! else built once per test process from `daemons/radioctl` (`cargo build -p radioctld`, a no-op when it is current), and the rig
//! descriptions of that workspace for virtual rigs. No hardware and no Hamlib.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The `rigs/` directory of radioctl in this repository.
#[allow(dead_code)]
pub fn rigs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../radioctl/rigs")
        .canonicalize()
        .expect("daemons/radioctl/rigs is next to bench_mcp")
}

/// The path of a `radioctld` executable.
#[allow(dead_code)]
pub fn radioctld() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(p) = std::env::var_os("BENCH_MCP_RADIOCTLD").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../radioctl/Cargo.toml");
        let mut cargo = std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        // The test run's own CARGO_* variables describe bench_mcp, not the radioctl workspace; only where to build is passed on.
        for (k, _) in std::env::vars_os() {
            let k = k.to_string_lossy().to_string();
            if k.starts_with("CARGO_") && k != "CARGO_TARGET_DIR" && k != "CARGO_HOME" && k != "CARGO_BUILD_JOBS" {
                cargo.env_remove(k);
            }
        }
        let out = cargo
            .args(["build", "-p", "radioctld", "--bin", "radioctld", "--message-format=json", "--manifest-path"])
            .arg(&manifest)
            .output()
            .expect("cargo is needed to build radioctld for the tests");
        assert!(
            out.status.success(),
            "building radioctld (daemons/radioctl) failed: {}",
            String::from_utf8_lossy(&out.stderr).lines().filter(|l| l.contains("error")).collect::<Vec<_>>().join(" | ")
        );
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["reason"] == "compiler-artifact" && v["target"]["name"] == "radioctld")
            .find_map(|v| v["executable"].as_str().map(PathBuf::from))
            .expect("cargo built radioctld but reported no executable")
    })
    .clone()
}

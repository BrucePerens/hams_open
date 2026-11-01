// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctld --help` and `-h` print the usage line on standard output and exit 0 (they used to
//! be "unknown argument" with status 2); a real mistake still exits 2. Nothing is opened.
#![allow(clippy::expect_used, clippy::panic)]

use std::process::Command;

// Tests [@ANCHOR: radioctld_main]
// Tests [@ANCHOR: radioctld_main_parse_args]
#[test]
fn help_prints_the_usage_and_succeeds_and_a_mistake_still_fails() {
    let bin = env!("CARGO_BIN_EXE_radioctld");
    for flag in ["--help", "-h"] {
        let out = Command::new(bin).arg(flag).output().expect("run");
        assert_eq!(out.status.code(), Some(0), "{flag}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.starts_with("usage: radioctld --config FILE"),
            "{text:?}"
        );
    }
    let out = Command::new(bin).arg("--bogus").output().expect("run");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));
}

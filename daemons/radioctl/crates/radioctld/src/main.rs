// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctld`: skeleton of the rigctld-protocol server. It opens no radio and listens
//! on no port yet.

use std::process::ExitCode;

// [@ANCHOR: radioctld_banner]
/// The one line the skeleton prints, naming the crate versions it was built from.
fn banner() -> String {
    format!(
        "radioctld {} (skeleton: no radio support yet)",
        env!("CARGO_PKG_VERSION")
    )
}

// [@ANCHOR: radioctld_main]
fn main() -> ExitCode {
    eprintln!("{}", banner());
    // Fail fast: a daemon that cannot serve anything must not look like one that can.
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctld_banner]
    // Tests [@ANCHOR: radioctld_main]
    #[test]
    fn banner_names_version_and_says_skeleton() {
        let line = banner();
        assert!(line.starts_with("radioctld 0.1.0"));
        assert!(line.contains("skeleton"));
    }
}

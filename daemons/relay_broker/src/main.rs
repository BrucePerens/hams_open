// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `hams-relay-broker` binary; the program is `relay_broker::cli::run_main` (Unix) or
//! `relay_broker::windows::run_main` (Windows).

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    relay_broker::cli::run_main()
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    relay_broker::windows::run_main()
}

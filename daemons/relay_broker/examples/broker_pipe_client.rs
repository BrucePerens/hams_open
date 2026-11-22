// SPDX-License-Identifier: AGPL-3.0-or-later
//! A test client for a running Windows broker: `broker_pipe_client status` or
//! `broker_pipe_client set UNIX_SECS`. Prints the response; talks to `\\.\pipe\hams-relay-broker`.
#![allow(clippy::expect_used)]

#[cfg(windows)]
fn main() {
    use relay_broker::wire::{Request, TimeSource};
    let args: Vec<String> = std::env::args().collect();
    let request = match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("status"), None) => Request::TimeStatus,
        (Some("set"), Some(secs)) => Request::SetSystemTime {
            target_unix_secs: secs.parse().expect("UNIX_SECS"),
            source: TimeSource::Other,
            agreeing: 1,
        },
        _ => {
            eprintln!("usage: broker_pipe_client status | set UNIX_SECS");
            std::process::exit(2);
        }
    };
    match relay_broker::windows::call(relay_broker::windows::PIPE_NAME, &request) {
        Ok(response) => println!("{response:?}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("broker_pipe_client is for the Windows broker; on Unix use broker_client.");
}

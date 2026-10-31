// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `hams-relay-broker` binary. Usage:
//! `hams-relay-broker [--config /etc/hams-relay-broker.conf] [--socket PATH] [--config-owner UID]`.
//! With socket activation the listener is fd 3; otherwise `--socket` names a path to bind
//! (mode 0660, for development). The config must be owned by `--config-owner` (default 0).

use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use relay_broker::audit::AuditLog;
use relay_broker::broker::Broker;
use relay_broker::config::Config;
use relay_broker::os::LinuxOs;
use relay_broker::server::{activated_listener, run};

/// Command-line options.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    config: PathBuf,
    socket: Option<PathBuf>,
    config_owner: u32,
}

// [@ANCHOR: relay_broker_main_parse_args]
/// Parses argv; unknown options are an error.
fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        config: PathBuf::from("/etc/hams-relay-broker.conf"),
        socket: None,
        config_owner: 0,
    };
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        let value = |it: &mut dyn Iterator<Item = &String>| {
            it.next().cloned().ok_or(format!("{a} needs a value"))
        };
        match a.as_str() {
            "--config" => args.config = PathBuf::from(value(&mut it)?),
            "--socket" => args.socket = Some(PathBuf::from(value(&mut it)?)),
            "--config-owner" => {
                args.config_owner = value(&mut it)?.parse().map_err(|_| "bad uid".to_string())?
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(args)
}

// [@ANCHOR: relay_broker_main]
fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hams-relay-broker: {e}");
            return ExitCode::from(2);
        }
    };
    let config = match Config::load(&args.config, args.config_owner) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "hams-relay-broker: {}:{}: {}",
                args.config.display(),
                e.line,
                e.message
            );
            return ExitCode::from(2);
        }
    };
    let audit = match AuditLog::open(config.audit_log.as_deref()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hams-relay-broker: audit log: {e}");
            return ExitCode::from(2);
        }
    };
    let listener = match (activated_listener(), &args.socket) {
        (Some(l), _) => l,
        (None, Some(path)) => {
            let _ = std::fs::remove_file(path);
            match UnixListener::bind(path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("hams-relay-broker: bind {}: {e}", path.display());
                    return ExitCode::from(2);
                }
            }
        }
        (None, None) => {
            eprintln!("hams-relay-broker: no activated socket and no --socket");
            return ExitCode::from(2);
        }
    };
    let mut broker = Broker::new(&config, audit, Box::new(LinuxOs::default()));
    match run(
        &mut broker,
        &listener,
        Duration::from_secs(config.idle_exit_secs),
        None,
    ) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hams-relay-broker: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: relay_broker_main_parse_args]
    // Tests [@ANCHOR: relay_broker_main]
    #[test]
    fn options_are_closed_and_defaults_point_at_etc() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let a = parse_args(&s(&["b"])).unwrap();
        assert_eq!(a.config, PathBuf::from("/etc/hams-relay-broker.conf"));
        assert_eq!(a.config_owner, 0);
        let a = parse_args(&s(&[
            "b",
            "--socket",
            "/tmp/x",
            "--config-owner",
            "1000",
            "--config",
            "c",
        ]))
        .unwrap();
        assert_eq!(a.socket, Some(PathBuf::from("/tmp/x")));
        assert_eq!(a.config_owner, 1000);
        assert!(parse_args(&s(&["b", "--bogus"])).is_err());
        assert!(parse_args(&s(&["b", "--socket"])).is_err());
        // main's own exit paths: no config file means exit 2, exercised through the binary in
        // tests/cli.rs; here the argument parser is the unit.
    }
}

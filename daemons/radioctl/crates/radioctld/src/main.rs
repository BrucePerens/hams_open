// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctld`: the rigctld-protocol server. This binary owns the one tokio runtime of the
//! process; the library only ever receives its `Handle`.
//!
//! Usage: `radioctld --config FILE [--check]`; `--help` or `-h` prints the usage and exits 0. `--check` validates the configuration and the rig
//! descriptions it names and exits without listening.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use radioctl_core::rig::RigOptions;
use radioctld::config::Config;
use radioctld::daemon;
use radioctld::rigs::load_description;

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cli {
    config: PathBuf,
    check: bool,
    /// `--help` or `-h` was given: print the usage and exit with status 0.
    help: bool,
}

const USAGE: &str = "usage: radioctld --config FILE [--check]";

// [@ANCHOR: radioctld_main_parse_args]
/// Parses the command line (without the program name).
fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut config = None;
    let mut check = false;
    let mut help = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let value = iter.next().ok_or("--config needs a file name")?;
                if config.replace(PathBuf::from(value)).is_some() {
                    return Err("--config given twice".to_string());
                }
            }
            "--check" => check = true,
            "--help" | "-h" => help = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if help {
        return Ok(Cli {
            config: config.unwrap_or_default(),
            check,
            help,
        });
    }
    Ok(Cli {
        config: config.ok_or("--config is required")?,
        check,
        help,
    })
}

// [@ANCHOR: radioctld_main_load]
/// Reads the configuration file and every rig description it names.
fn load(path: &Path) -> Result<(Config, Vec<radioctl_core::description::RigDescription>), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let config = Config::parse(&text, base)?;
    let descriptions = config
        .rigs
        .iter()
        .map(|rig| load_description(&rig.description))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((config, descriptions))
}

// [@ANCHOR: radioctld_main_run]
/// Runs the daemon until the process is interrupted.
fn run(cli: &Cli) -> Result<(), String> {
    let (config, descriptions) = load(&cli.config)?;
    if cli.check {
        eprintln!(
            "radioctld: configuration ok, {} rig(s), {} rotator(s)",
            config.rigs.len(),
            config.rotators.len()
        );
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("radioctld")
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start the runtime: {e}"))?;
    let handle = runtime.handle().clone();
    runtime.block_on(async move {
        let daemon = daemon::start(&handle, &config, descriptions, RigOptions::default).await?;
        for served in &daemon.rigs {
            eprintln!(
                "radioctld: {} listening on {}",
                served.name,
                served.server.local_addr()
            );
        }
        for served in &daemon.rotators {
            eprintln!(
                "radioctld: rotator {} listening on {}",
                served.name,
                served.server.local_addr()
            );
        }
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| format!("cannot wait for ctrl-c: {e}"))?;
        eprintln!("radioctld: stopping");
        daemon.stop().await;
        Ok(())
    })
}

// [@ANCHOR: radioctld_main]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("radioctld: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if cli.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("radioctld: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    // Tests [@ANCHOR: radioctld_main_parse_args]
    #[test]
    fn the_command_line_needs_a_config_and_rejects_the_rest() {
        let cli = parse_args(&args(&["--config", "a.toml", "--check"])).expect("cli");
        assert_eq!(cli.config, PathBuf::from("a.toml"));
        assert!(cli.check);
        assert!(parse_args(&args(&[])).is_err());
        assert!(parse_args(&args(&["--config"])).is_err());
        assert!(parse_args(&args(&["-c", "a", "-c", "b"])).is_err());
        assert!(parse_args(&args(&["-c", "a", "--listen", "0.0.0.0"])).is_err());
        // `--help` and `-h` ask for the usage, with or without a config, and are not an error.
        for flag in ["--help", "-h"] {
            assert!(parse_args(&args(&[flag])).expect("help").help, "{flag}");
            assert!(parse_args(&args(&["-c", "a", flag])).expect("help").help);
            assert!(!cli.help);
        }
    }

    // Tests [@ANCHOR: radioctld_main_load]
    // Tests [@ANCHOR: radioctld_main_run]
    // Tests [@ANCHOR: radioctld_main]
    #[test]
    fn loading_a_missing_file_fails_and_check_mode_does_not_listen() {
        assert!(load(Path::new("/nonexistent/radioctld.toml")).is_err());
        let dir = std::env::temp_dir().join(format!("radioctld-main-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let rigs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/elecraft-kx3.toml");
        let file = dir.join("radioctld.toml");
        std::fs::write(
            &file,
            format!(
                "[[rig]]\nname = \"kx3\"\ndescription = {:?}\nlisten = \"127.0.0.1:0\"\n[rig.transport]\nkind = \"virtual\"\n",
                rigs.display().to_string()
            ),
        )
        .expect("write");
        let cli = Cli {
            config: file,
            check: true,
            help: false,
        };
        assert_eq!(run(&cli), Ok(()));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}

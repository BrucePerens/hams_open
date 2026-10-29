// SPDX-License-Identifier: AGPL-3.0-or-later
//! `vnwa_open install-system-config [--apply] [--uninstall] [--check] [--force] [--restart-audio] [--root DIR]` and `vnwa_open is-vnwa-codec <usb path>`.
//! See `docs/PROTOCOL.md` and `packaging/README.md`.
use std::path::{Path, PathBuf};

use vnwa_open::install::{Installer, Mode, Options, ProcessExec};

fn main() {
    // [@ANCHOR: vnwa_open_cli_main]
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("is-vnwa-codec") => {
            let ok = a
                .get(2)
                .is_some_and(|p| vnwa_open::reserved::is_vnwa_codec(Path::new("/sys"), p));
            std::process::exit(if ok { 0 } else { 1 });
        }
        Some("install-system-config") => {
            let has = |f: &str| a.iter().any(|x| x == f);
            let root = a
                .iter()
                .position(|x| x == "--root")
                .and_then(|i| a.get(i + 1))
                .map(PathBuf::from);
            let mode = if has("--check") {
                Mode::Check
            } else if has("--uninstall") {
                Mode::Uninstall
            } else {
                Mode::Install
            };
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            let log = match &root {
                Some(r) => r.join("var/log/vnwa_open-install.log"),
                None => PathBuf::from(
                    std::env::var("XDG_STATE_HOME")
                        .unwrap_or_else(|_| format!("{home}/.local/state")),
                )
                .join("vnwa_open/install.log"),
            };
            let inst = Installer {
                opts: Options {
                    mode: mode.clone(),
                    apply: has("--apply"),
                    force: has("--force"),
                    restart_audio: has("--restart-audio"),
                    root,
                },
                exec: &ProcessExec,
                sys: PathBuf::from("/sys"),
                proc_asound: PathBuf::from("/proc/asound"),
                log,
                root_user: std::env::var("USER").as_deref() == Ok("root"),
            };
            match inst.run() {
                Ok(t) => {
                    print!("{t}");
                    if mode == Mode::Check {
                        let e = vnwa_open::sound_server::check(
                            Path::new("/sys"),
                            &vnwa_open::sound_server::run_program,
                        );
                        println!("Sound server: {:?}", e);
                        if let Some(w) = vnwa_open::sound_server::warning(&e) {
                            println!("WARNING: {w}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("vnwa_open install-system-config: {e}");
                    std::process::exit(2);
                }
            }
        }
        _ => {
            eprintln!("usage: vnwa_open install-system-config [--apply] [--uninstall] [--check] [--force] [--restart-audio] [--root DIR] | is-vnwa-codec <usb path, e.g. 3-2.2.2>");
            std::process::exit(2);
        }
    }
}

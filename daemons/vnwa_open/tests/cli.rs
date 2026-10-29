// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `vnwa_open` command line, run as the built binary: exit codes and output of its commands that need no unit.
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vnwa_open"))
        .args(args)
        .output()
        .unwrap()
}

// Tests [@ANCHOR: vnwa_open_cli_main]
#[test]
fn the_command_line_refuses_an_unknown_command_and_answers_a_codec_question_with_its_exit_status() {
    let o = run(&[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("usage: vnwa_open"));
    assert_eq!(run(&["is-vnwa-codec"]).status.code(), Some(1));
    assert_eq!(
        run(&["is-vnwa-codec", "9-9.9.9"]).status.code(),
        Some(1),
        "a USB path that is not a VNWA codec"
    );
}

// Tests [@ANCHOR: vnwa_open_cli_main]
#[test]
fn install_system_config_check_under_a_staging_root_reports_the_files_it_would_manage_and_changes_nothing(
) {
    let root = std::env::temp_dir().join(format!("vnwa_open_cli_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root_arg = root.to_str().unwrap();
    let o = run(&["install-system-config", "--check", "--root", root_arg]);
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{out} {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(out.contains("Sound server:"), "{out}");
    // Nothing but the install log (which a check does not write either) may appear under the root.
    let created: Vec<_> = std::fs::read_dir(&root).unwrap().collect();
    assert!(created.is_empty(), "{created:?}");
    let _ = std::fs::remove_dir_all(&root);
}

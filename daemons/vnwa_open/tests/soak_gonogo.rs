// SPDX-License-Identifier: AGPL-3.0-or-later
//! `tools/soak_gonogo.sh` judging a recorded soak log (GONOGO_VERDICT_ONLY), without a unit or a scope.
use std::process::Command;

fn verdict(name: &str, log: &str, events: &str) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!("vnwa_gonogo_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("soak.log"), log).unwrap();
    std::fs::write(dir.join("events.jsonl"), events).unwrap();
    let o = Command::new("bash")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/soak_gonogo.sh"))
        .arg(&dir)
        .env("GONOGO_VERDICT_ONLY", "1")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).ok();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).to_string(),
    )
}

fn good_log() -> String {
    let mut l = String::new();
    for r in 0..5 {
        l += &format!("{r} SWEEP round: vs last-cycle baseline (at 100) 0.100 % 0.050 deg\n");
        l += &format!("{r} GENERATOR round {r}: -6 dB 5.170 mV, -12 dB 2.590 mV, off 0.140 mV\n");
        l += &format!("{r} METER offset +0.001 Hz\n");
    }
    l
}

// Tests [@ANCHOR: vnwa_open_soak_scope_open_checked]
#[test]
fn a_scope_that_never_answered_is_a_no_go_that_names_the_evidence_and_the_action_and_a_clean_log_is_go(
) {
    let (code, out) = verdict("good", &good_log(), "");
    assert_eq!(code, 0, "{out}");
    assert!(out.trim_end().ends_with("GO"), "{out}");
    let summary = "round 1: NO-GO: scope 192.168.10.51:5025 did not answer *IDN? in 5 attempts over 37 s (attempt 1: read: Connection reset by peer (os error 104)). State: ping answers, port 5025 accepts, port 80: Connection refused (os error 111). Recommended action: power-cycle the scope (its single-client SCPI server probably still holds a dead session)";
    let ev = format!(
        "{{\"epoch\":1.0,\"iso\":\"x\",\"class\":\"generator_step_failed\",\"summary\":\"{summary}\",\"detail\":\"\",\"journal_5s\":\"\"}}\n"
    );
    let (code, out) = verdict("nogo", &good_log(), &ev);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("NO-GO:"), "{out}");
    assert!(
        out.contains("port 80: Connection refused"),
        "evidence kept: {out}"
    );
    assert!(out.contains("power-cycle the scope"), "{out}");
}

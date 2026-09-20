// SPDX-License-Identifier: LGPL-3.0-or-later
//! Builds the fixed-point Codec2 3200 path for bare-metal targets (`no_std`, no floating-point unit)
//! so a future change cannot quietly bring `std`, `alloc` or float-only methods back into it. This
//! runs `tools/check_codec2_no_std.sh --lib-only`: the library with `--no-default-features` (with
//! and without the 16 kHz bridge) for `riscv32imc-unknown-none-elf` and `thumbv7m-none-eabi`,
//! warnings denied. Skipped, with a message, when those rustup targets are not installed. The full
//! check (bench link, no soft-float symbols, QEMU checksum) is `tools/check_codec2_no_std.sh`.

use std::path::Path;
use std::process::Command;

#[test]
fn the_fixed_codec2_3200_path_builds_no_std_for_bare_metal_targets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("bash")
        .arg(root.join("tools/check_codec2_no_std.sh"))
        .arg("--lib-only")
        .env("SKIP_MISSING", "1")
        .output()
        .expect("run tools/check_codec2_no_std.sh");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if text.contains("missing rustup target") {
        eprintln!("skipped: {text}");
        return;
    }
    assert!(out.status.success(), "no_std build check failed:\n{text}");
}

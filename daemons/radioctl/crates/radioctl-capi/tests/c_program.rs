// SPDX-License-Identifier: AGPL-3.0-or-later
//! The C test program (`tests/c/capi_test.c`) compiled with the system `cc` against the generated
//! header and run against the library built from this crate. Also pins the checked-in header to
//! what cbindgen generates, so the header a C programmer reads is the code's.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `target/<profile>`, found from the test executable's own path (`target/<profile>/deps/x`).
fn profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("exe");
    exe.parent()
        .and_then(Path::parent)
        .expect("target/<profile>/deps/<exe>")
        .to_path_buf()
}

/// Builds the shared library (a no-op when cargo already has) and returns its directory.
fn shared_library_dir() -> PathBuf {
    let dir = profile_dir();
    let release = dir.file_name().is_some_and(|n| n == "release");
    let target = dir.parent().expect("target dir");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut build = Command::new(cargo);
    build
        .args(["build", "-p", "radioctl-capi", "--target-dir"])
        .arg(target)
        .current_dir(manifest());
    if release {
        build.arg("--release");
    }
    let status = build.status().expect("cargo build");
    assert!(status.success(), "cargo build -p radioctl-capi failed");
    assert!(
        dir.join("libradioctl_capi.so").exists() || dir.join("libradioctl_capi.dylib").exists(),
        "no shared library in {}",
        dir.display()
    );
    dir
}

// Tests [@ANCHOR: radioctl_capi_abi_version]
// Tests [@ANCHOR: radioctl_capi_version]
// Tests [@ANCHOR: radioctl_capi_last_error]
// Tests [@ANCHOR: radioctl_capi_runtime_new]
// Tests [@ANCHOR: radioctl_capi_runtime_free]
// Tests [@ANCHOR: radioctl_capi_rig_open_virtual]
// Tests [@ANCHOR: radioctl_capi_rig_open_tcp]
// Tests [@ANCHOR: radioctl_capi_rig_open_serial]
// Tests [@ANCHOR: radioctl_capi_rig_close]
// Tests [@ANCHOR: radioctl_capi_rig_get_frequency]
// Tests [@ANCHOR: radioctl_capi_rig_set_frequency]
// Tests [@ANCHOR: radioctl_capi_rig_get_mode]
// Tests [@ANCHOR: radioctl_capi_rig_set_mode]
// Tests [@ANCHOR: radioctl_capi_rig_get_ptt]
// Tests [@ANCHOR: radioctl_capi_rig_set_ptt]
// Tests [@ANCHOR: radioctl_capi_rig_permit_transmit]
// Tests [@ANCHOR: radioctl_capi_rig_revoke_transmit]
// Tests [@ANCHOR: radioctl_capi_rig_transmit_permitted]
// Tests [@ANCHOR: radioctl_capi_rig_link_up]
// Tests [@ANCHOR: radioctl_capi_keep_connected]
#[test]
fn the_c_program_passes_against_the_shared_library() {
    let lib = shared_library_dir();
    let out_dir = std::env::temp_dir().join(format!("radioctl-capi-ctest-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).expect("temp dir");
    let exe = out_dir.join("capi_test");
    let compile = Command::new("cc")
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-O1",
        ])
        .arg("-I")
        .arg(manifest().join("include"))
        .arg(manifest().join("tests/c/capi_test.c"))
        .arg("-L")
        .arg(&lib)
        .arg(format!("-Wl,-rpath,{}", lib.display()))
        .arg("-lradioctl_capi")
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("cc is installed");
    assert!(
        compile.status.success(),
        "the C test did not compile:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let rigfile = manifest().join("../../rigs/elecraft-kx3.toml");
    let run = Command::new(&exe).arg(&rigfile).output().expect("run");
    let _ = std::fs::remove_dir_all(&out_dir);
    assert!(
        run.status.success(),
        "the C test failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "OK");
}

#[test]
fn the_checked_in_header_is_what_cbindgen_generates() {
    let generated = include_str!(concat!(env!("OUT_DIR"), "/radioctl.h"));
    let checked_in =
        std::fs::read_to_string(manifest().join("include/radioctl.h")).expect("header");
    assert!(
        checked_in == generated,
        "include/radioctl.h is out of date: copy the generated header from the build's OUT_DIR"
    );
    // The header must be usable from C++ and must not drag in a runtime header.
    assert!(generated.contains("extern \"C\""));
    for line in generated.lines().filter(|l| l.starts_with("#include")) {
        assert!(
            line == "#include <stdint.h>" || line == "#include <stddef.h>",
            "unexpected include: {line}"
        );
    }
}

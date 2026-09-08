// SPDX-License-Identifier: LGPL-3.0-or-later
// Compiles vendored ft8_lib (MIT, see vendor/ft8_lib/LICENSE-MIT) plus
// shim.c, our own thin C wrapper giving Rust an opaque-pointer API.

fn main() {
    let dir = "vendor/ft8_lib";
    println!("cargo:rerun-if-changed={dir}");
    println!("cargo:rerun-if-changed=windows_stpcpy_compat.c");

    let mut build = cc::Build::new();
    build
        .include(dir)
        .file(format!("{dir}/ft8/constants.c"))
        .file(format!("{dir}/ft8/crc.c"))
        .file(format!("{dir}/ft8/decode.c"))
        .file(format!("{dir}/ft8/encode.c"))
        .file(format!("{dir}/ft8/ldpc.c"))
        .file(format!("{dir}/ft8/message.c"))
        .file(format!("{dir}/ft8/text.c"))
        .file(format!("{dir}/fft/kiss_fft.c"))
        .file(format!("{dir}/fft/kiss_fftr.c"))
        .file(format!("{dir}/common/monitor.c"))
        .file(format!("{dir}/shim.c"))
        .flag_if_supported("-O3")
        .warnings(false);

    // Real, directly-observed cross-compile failure (2026-09-08, x86_64-pc-windows-gnu, this
    // dev box's own mingw-w64 GCC 14): message.c's own unpackgrid() calls stpcpy(), a
    // POSIX/glibc extension mingw-w64's own headers never declare at all -- confirmed directly
    // (grepped /usr/x86_64-w64-mingw32/include/string.h, no match), not gated behind a feature
    // macro the way some POSIX extensions are. GCC 14 treats a call to an undeclared function as
    // a hard, unconditional compile error ("implicit declaration of function 'stpcpy'") that
    // `.warnings(false)` above does NOT suppress (confirmed directly: tried `-std=gnu17` too,
    // same result) -- the real fix is making a real prototype visible before the call, not
    // fighting that diagnostic's severity. `-include` forces
    // windows_stpcpy_compat.h's declaration into every file this Build compiles; the matching
    // .c provides the actual definition. Both windows_stpcpy_compat.{h,c} live outside
    // vendor/ft8_lib since that tree is third-party (MIT) and not ours to edit.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        build
            .flag("-include")
            .flag("windows_stpcpy_compat.h")
            .file("windows_stpcpy_compat.c");
    }

    build.compile("ft8_ffi");
}

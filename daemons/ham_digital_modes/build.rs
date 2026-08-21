// SPDX-License-Identifier: LGPL-3.0-or-later
// Compiles vendored ft8_lib (MIT, see vendor/ft8_lib/LICENSE-MIT) plus
// shim.c, our own thin C wrapper giving Rust an opaque-pointer API.

fn main() {
    let dir = "vendor/ft8_lib";
    println!("cargo:rerun-if-changed={dir}");

    cc::Build::new()
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
        .warnings(false)
        .compile("ft8_ffi");
}

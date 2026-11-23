#!/bin/sh
# Swift side of the shared Rust core (hams_net_ffi, UniFFI).
# Usage: tools/swift_bindings.sh [OUT_DIR]   (default target/swift)
#  1. generates hams_net_ffi.swift + the C header/modulemap from the built library (any host);
#  2. on macOS: builds the host staticlib, compiles swift/hello.swift against it with swiftc and runs it (hello-call test);
#  3. with Xcode (xcodebuild works): builds aarch64-apple-ios and aarch64-apple-ios-sim staticlibs and an XCFramework
#     (OUT_DIR/HamsNetFfi.xcframework). Without Xcode this step prints SKIP; nothing else is hidden.
# Needs: cargo (rust-toolchain.toml), rustup targets aarch64-apple-ios, aarch64-apple-ios-sim for step 3.
set -eu
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
OUT="${1:-target/swift}"
mkdir -p "$OUT/gen"
cargo build --release --lib --bins ${CARGO_BUILD_JOBS:+-j "$CARGO_BUILD_JOBS"}
TD="${CARGO_TARGET_DIR:-target}"
case "$(uname -s)" in Darwin) ext=dylib ;; *) ext=so ;; esac
"$TD/release/uniffi-bindgen" generate --library "$TD/release/libhams_net_ffi.$ext" --language swift --out-dir "$OUT/gen"
echo "OK swift sources in $OUT/gen: $(ls "$OUT/gen" | tr '\n' ' ')"
if [ "$(uname -s)" != Darwin ]; then echo "SKIP hello test and XCFramework: need a macOS host"; exit 0; fi
# hello-call on the host (static library, module map from the generated header)
mkdir -p "$OUT/hello/include"
cp "$OUT/gen/hams_net_ffiFFI.h" "$OUT/hello/include/"
cp "$OUT/gen/hams_net_ffiFFI.modulemap" "$OUT/hello/include/module.modulemap"
swiftc -parse-as-library -O -I "$OUT/hello/include" -Xcc -fmodule-map-file="$OUT/hello/include/module.modulemap" \
  "$OUT/gen/hams_net_ffi.swift" swift/hello.swift "$TD/release/libhams_net_ffi.a" -o "$OUT/hello/hello"
"$OUT/hello/hello"
if ! xcodebuild -version >/dev/null 2>&1; then echo "SKIP XCFramework: Xcode not installed (xcodebuild needs a full Xcode, only the Command Line Tools are present)"; exit 0; fi
frameworks=""
for t in aarch64-apple-ios aarch64-apple-ios-sim; do
  cargo build --release --lib --target "$t" ${CARGO_BUILD_JOBS:+-j "$CARGO_BUILD_JOBS"}
  frameworks="$frameworks -library $TD/$t/release/libhams_net_ffi.a -headers $OUT/hello/include"
done
rm -rf "$OUT/HamsNetFfi.xcframework"
# shellcheck disable=SC2086
xcodebuild -create-xcframework $frameworks -output "$OUT/HamsNetFfi.xcframework"
echo "OK XCFramework $OUT/HamsNetFfi.xcframework"

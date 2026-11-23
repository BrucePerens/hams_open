#!/bin/sh
# Cross-builds the core's size probe for the OpenWrt/Linux musl, Android and iOS targets and records sizes.
# Usage: tools/cross_build.sh [target...]   (default: all). Output: target/cross_sizes.txt
# Targets that cannot be built here are listed as SKIP with the reason; nothing else is hidden.
# MIPSEL_LINKER: OpenWrt mipsel musl gcc (from the OpenWrt SDK for that target) to link the probe.
# ANDROID_NDK_HOME defaults to the newest NDK under /home/bruce/agent_work/tools/android/ndk (sdkmanager "ndk;28.2.13676358").
# Needs: rustup targets (see TARGETS), nightly + rust-src for the tier-3 mipsel target, the Android NDK
# (ANDROID_NDK_HOME) for aarch64-linux-android, a macOS host for aarch64-apple-ios.
set -u
cd "$(dirname "$0")/.." || exit 1
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.98.1}"
TARGETS="${*:-x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf mipsel-unknown-linux-musl aarch64-linux-android aarch64-apple-ios}"
OUT=target/cross_sizes.txt
mkdir -p target
: > "$OUT"
for t in $TARGETS; do
  case "$t" in
    *-unknown-linux-musl*)
      # pure Rust, self-contained musl: rust-lld links without a C cross toolchain
      var=CARGO_TARGET_$(echo "$t" | tr 'a-z-' 'A-Z_')_LINKER
      export "$var=rust-lld"
      export RUSTFLAGS="-C linker-flavor=ld.lld -C link-self-contained=yes"
      ;;
    aarch64-linux-android)
      ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$(ls -d /home/bruce/agent_work/tools/android/ndk/* 2>/dev/null | tail -1)}"
      if [ -z "${ANDROID_NDK_HOME:-}" ] || [ ! -d "$ANDROID_NDK_HOME" ]; then
        echo "SKIP $t no Android NDK (set ANDROID_NDK_HOME)" >> "$OUT"; continue
      fi
      bin=$(ls -d "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin)
      export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android24-clang"
      unset RUSTFLAGS
      ;;
    aarch64-apple-ios)
      if [ "$(uname -s)" != Darwin ]; then echo "SKIP $t needs a macOS host (Xcode linker)" >> "$OUT"; continue; fi
      unset RUSTFLAGS
      ;;
    *) unset RUSTFLAGS ;;
  esac
  if [ "$t" = mipsel-unknown-linux-musl ]; then
    # tier 3: no prebuilt std, build it from nightly's rust-src
    cmd="cargo +nightly build -Z build-std=std,panic_abort"
  else
    cmd="cargo build"
  fi
  what="--example probe"
  if [ "$t" = mipsel-unknown-linux-musl ] && [ -z "${MIPSEL_LINKER:-}" ]; then
    # no OpenWrt mipsel SDK on this box: prove the crate compiles, skip linking
    what=--lib
  elif [ "$t" = mipsel-unknown-linux-musl ]; then
    export CARGO_TARGET_MIPSEL_UNKNOWN_LINUX_MUSL_LINKER="$MIPSEL_LINKER"
    unset RUSTFLAGS
  fi
  if $cmd --release $what --target "$t" ${CARGO_BUILD_JOBS:+-j "$CARGO_BUILD_JOBS"} >"target/cross_$t.log" 2>&1; then
    if [ "$what" = --lib ]; then echo "OK $t lib compiles; no binary (set MIPSEL_LINKER to an OpenWrt mipsel musl gcc to link)" >> "$OUT"; continue; fi
    f="${CARGO_TARGET_DIR:-target}/$t/release/examples/probe"
    echo "OK $t $(wc -c < "$f") bytes" >> "$OUT"
  else
    echo "FAIL $t see target/cross_$t.log" >> "$OUT"
  fi
done
cat "$OUT"
grep -q '^FAIL' "$OUT" && exit 1
exit 0

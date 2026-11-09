# radiod_client fuzz targets

libFuzzer targets for the parsers that take datagrams from the network: `tlv_parse` (every `Entry` accessor),
`status_decode` (`ChannelStatus::decode` and its derived views) and `rtp_decode` (`RtpPacket::parse`, the PCM
decoders, `SeqTracker`). Each asserts only "no panic"; a rejected datagram is normal.

This crate is its own workspace root, so `cargo build --workspace` and CI never compile it. Run it with
`cargo +nightly fuzz run <target>`, or without a nightly toolchain or `cargo-fuzz` (the same instrumentation
cargo-fuzz passes, enabled on stable with `RUSTC_BOOTSTRAP`; `--target` keeps the flags off build scripts):

```
export RUSTC_BOOTSTRAP=1
export RUSTFLAGS="-Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 \
  -Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table \
  -Cllvm-args=-sanitizer-coverage-trace-compares --cfg fuzzing -Cdebug-assertions -Coverflow-checks"
cargo build --release --target x86_64-unknown-linux-gnu --bins
mkdir -p corpus/tlv_parse && target/x86_64-unknown-linux-gnu/release/tlv_parse -max_total_time=1500 corpus/tlv_parse
```

Keep the corpus and any `crash-*` file outside the repository until a crash has a regression test.

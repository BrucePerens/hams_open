// SPDX-License-Identifier: AGPL-3.0-or-later
//! Runs the `tools/codec2_riscv32_bench` firmware (bare-metal riscv32imc, no floating point) under QEMU and checks what it
//! reports: the UART text, the instruction counters, the stack high-water marks and the checksum of the encoded and decoded
//! frames, which must equal the one this host computes from the same codec. QEMU is an emulator; no hardware is touched.
//!
//! Needs `qemu-system-riscv32` and the rustup target `riscv32imc-unknown-none-elf` (`rustup target add ...`); without them the
//! test fails with that instruction rather than passing silently.

use ham_digital_modes::codec2_3200::{
    DecoderFixed, EncoderFixed, BYTES_PER_FRAME, SAMPLES_PER_FRAME,
};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FRAMES: usize = 8;

fn bench_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/codec2_riscv32_bench")
}

/// A deterministic voiced-speech-like signal: four harmonics of a gliding 120 Hz pitch plus a little noise, 16-bit, 8 kHz.
fn speech() -> Vec<i16> {
    let mut seed: u32 = 1;
    (0..FRAMES * SAMPLES_PER_FRAME)
        .map(|i| {
            let t = i as f64 / 8000.0;
            let glide = 1.0 + 0.02 * (2.0 * std::f64::consts::PI * 3.0 * t).sin();
            let v: f64 = [
                (120.0, 6000.0),
                (240.0, 4000.0),
                (360.0, 2500.0),
                (480.0, 1500.0),
            ]
            .iter()
            .map(|(f, a)| (2.0 * std::f64::consts::PI * f * t * glide).sin() * a)
            .sum();
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((seed >> 16) as f64 / 65536.0 - 0.5) * 1000.0;
            (v + noise) as i16
        })
        .collect()
}

/// The checksum the firmware prints: FNV-style over the encoded bytes of every frame, then over the decoded samples.
fn host_checksum(pcm: &[i16]) -> u32 {
    let mut enc = EncoderFixed::new();
    let mut dec = DecoderFixed::new();
    let mut ck: u32 = 0;
    let mut frames = Vec::new();
    for f in 0..FRAMES {
        let mut fr = [0i16; SAMPLES_PER_FRAME];
        fr.copy_from_slice(&pcm[f * SAMPLES_PER_FRAME..(f + 1) * SAMPLES_PER_FRAME]);
        let bits: [u8; BYTES_PER_FRAME] = enc.encode(&fr);
        for &b in &bits {
            ck = ck.wrapping_mul(16777619).wrapping_add(b as u32);
        }
        frames.push(bits);
    }
    for bits in &frames {
        for &s in &dec.decode(bits) {
            ck = ck.wrapping_mul(16777619).wrapping_add(s as u16 as u32);
        }
    }
    ck
}

fn number_after(out: &str, prefix: &str) -> u64 {
    let line = out
        .lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting {prefix:?} in the firmware output:\n{out}"));
    line[prefix.len()..]
        .split_whitespace()
        .next()
        .and_then(|w| w.parse().ok())
        .unwrap_or_else(|| panic!("no number after {prefix:?} in {line:?}"))
}

fn build_and_run(pcm: &[i16]) -> (String, Option<i32>) {
    let dir = bench_dir();
    let raw: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    // `speech_small.raw` is the file the `small` build embeds; it is not committed (see the bench's .gitignore).
    std::fs::write(dir.join("speech_small.raw"), raw).unwrap();
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("riscv32_bench");
    let mut build = Command::new(env!("CARGO"));
    build
        .args(["build", "--release", "--features", "small", "--target-dir"])
        .arg(&target_dir)
        .current_dir(&dir);
    // Coverage and wrapper variables would instrument or redirect this bare-metal build (see tools/check_codec2_no_std.sh).
    for var in [
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "LLVM_PROFILE_FILE",
        "CARGO_LLVM_COV",
        "CARGO_LLVM_COV_TARGET_DIR",
        "CARGO_LLVM_COV_SHOW_ENV",
        "CARGO_TARGET_DIR",
    ] {
        build.env_remove(var);
    }
    let built = build.output().expect("cannot run cargo");
    assert!(
        built.status.success(),
        "building the riscv32 bench failed (needs `rustup target add riscv32imc-unknown-none-elf`):\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let elf = target_dir.join("riscv32imc-unknown-none-elf/release/codec2_riscv32_bench");
    let mut child = Command::new("qemu-system-riscv32")
        .args([
            "-M",
            "virt",
            "-m",
            "64M",
            "-nographic",
            "-bios",
            "none",
            "-icount",
            "shift=0",
            "-kernel",
        ])
        .arg(&elf)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot run qemu-system-riscv32 (install qemu-system-misc)");
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the firmware did not exit within 120 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut out).unwrap();
    (out, status.code())
}

// Tests [@ANCHOR: codec2_riscv32_bench_main]
// Tests [@ANCHOR: codec2_riscv32_bench_uart_write_str]
// Tests [@ANCHOR: codec2_riscv32_bench_exit]
// Tests [@ANCHOR: codec2_riscv32_bench_paint_stack]
// Tests [@ANCHOR: codec2_riscv32_bench_stack_high_water]
// Tests [@ANCHOR: codec2_riscv32_bench_instret]
// Tests [@ANCHOR: codec2_riscv32_bench_mark]
#[test]
fn the_riscv32_firmware_runs_under_qemu_reports_its_counters_and_matches_the_host_checksum() {
    let pcm = speech();
    let (out, code) = build_and_run(&pcm);

    // exit(0): the test device ends QEMU with status 0.
    assert_eq!(code, Some(0), "firmware output:\n{out}");

    // The UART carries the report (write_str), one line per item.
    assert!(out.starts_with("sizeof EncoderFixed "), "{out}");
    assert_eq!(number_after(&out, "frames="), FRAMES as u64);

    // Stack high-water marks (paint_stack, stack_high_water): the codec's call tree uses some stack, but far less than the 64 MB.
    let enc_stack = number_after(&out, "peak stack during encode ");
    let dec_stack = number_after(&out, "peak stack during decode ");
    assert!(
        (256..60_000).contains(&enc_stack),
        "encode stack {enc_stack}"
    );
    assert!(
        (256..60_000).contains(&dec_stack),
        "decode stack {dec_stack}"
    );

    // Instruction counts (instret): an encoded frame is hundreds of thousands of instructions, the steady average never above the maximum.
    let line = out
        .lines()
        .find(|l| l.starts_with("ENCODE first-frame"))
        .unwrap();
    let nums: Vec<u64> = line
        .split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect();
    let (first, avg, max) = (nums[0], nums[1], nums[2]);
    assert!(first > 100_000 && avg > 100_000 && avg <= max, "{line}");
    let dline = out
        .lines()
        .find(|l| l.starts_with("DECODE first-frame"))
        .unwrap();
    let dn: Vec<u64> = dline
        .split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect();
    assert!(
        dn[0] > 100_000 && dn[1] > 100_000 && dn[1] <= dn[2],
        "{dline}"
    );

    // Per-stage marks (mark): the buckets split the frame, so every main stage is counted and together they do not exceed the frame.
    let enc_stage_sum: u64 = out
        .lines()
        .filter(|l| l.starts_with("  enc "))
        .map(|l| l.split_whitespace().last().unwrap().parse::<u64>().unwrap())
        .sum();
    assert!(
        enc_stage_sum > 0 && enc_stage_sum <= avg,
        "stages {enc_stage_sum} of {avg}"
    );
    assert!(
        number_after(&out, "  enc nlp: 512-pt transform ") > 0,
        "{out}"
    );
    assert!(number_after(&out, "  dec harmonic_amps(x2) ") > 0, "{out}");

    // The 32-bit build is bit-identical to the host build of the same codec.
    let line = out.lines().find(|l| l.starts_with("checksum ")).unwrap();
    let on_target = u32::from_str_radix(line["checksum ".len()..].trim(), 16).unwrap();
    assert_eq!(on_target, host_checksum(&pcm), "{line}");
}

// Data-structure anchors (ADR 0113) exercised by the tests above.
// Tests [@ANCHOR: main_uart_struct]

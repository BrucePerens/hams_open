// SPDX-License-Identifier: LGPL-3.0-or-later
//! The decisive real-hardware test for this whole investigation's own working hypothesis: does the
//! real DVSI AMBE3003 chip's output, when configured for its own **AMBE+2 half-rate** rate
//! (`PKT_RATET` Rate Index 33 "APCO Project 25 half-rate with FEC" / Index 34 "...with No FEC",
//! `0x21`/`0x22` -- found in DVSI's own USB-3000 Manual, not assumed), actually decode through this
//! crate's newly built `ambe_plus_2` codec with a pitch parameter that correlates with the known
//! true test-tone frequency?
//!
//! Two rates are tested, both genuinely matching this codec's own 49-data-bit / 72-total-bit frame
//! size (unlike RATET 27's 88/144-bit full-rate frame, which is a different rate entirely and not
//! re-litigated here -- see `AMBE_CHIP_VALIDATION_FINDINGS.md` sections 7-10 for that separate,
//! already-negative result):
//!
//! - **Rate Index 34 (2450 bps, No FEC)**: the chip returns the raw 49 data bits directly, with no
//!   FEC/whitening/interleave ambiguity at all -- the cleanest possible test. Two structured
//!   hypotheses for how those 49 bits are ordered are checked directly (this crate's own `d[]`
//!   FEC-codeword order vs. plain contiguous `b0..b8` fields), plus a hypothesis-agnostic sliding
//!   7-bit-window correlation scan (the same technique that found P25 full-rate's own Gray-coded
//!   `u2` pitch field) across all 43 possible window positions, both plain-binary and Gray-decoded.
//! - **Rate Index 33 (3600 bps, with FEC)**: the chip returns 72 bits. Both a direct
//!   `C0||C1||C2||C3` concatenation and Annex H's own TIA-specified deinterleave are tried; the
//!   discriminator is the fraction of frames whose `C0`/`C1` Golay-decode with zero corrected
//!   errors (real evidence of correct framing, not just a coincidence of the perfect Golay code --
//!   see `ambe_plus_2::decode`'s own doc comment on why a *single* zero-error frame isn't enough).
//!
//! # Safety
//! Only ever sends ordinary DVSI CONTROL/SPEECH UDP packets via AMBEServer3003, exactly like every
//! other committed `ambe_chip_*` harness in this crate -- never touches the serial/USB layer
//! directly (a UART BREAK to this chip requires a full host reboot to recover from).
//!
//! Run against the chip: `cargo run --release --features ambe_plus_2 --example ambe_chip_validate_ambe_plus_2 -- 192.168.10.189:2460`

use ham_digital_modes::ambe_plus_2::decode::{classify_b0, extract_raw_parameters, FrameKind, RawParameters};
use ham_digital_modes::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe_plus_2::{parse_frame, tables};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const SETTLING_FRAMES: usize = 80;
const CAPTURED_FRAMES: usize = 15;

// DVSI's own USB-3000 Manual: "APCO Project 25 half-rate with FEC (3600 bps) Rate Index 33" =
// 0x21, "...with No FEC (2450 bps) Rate Index 34" = 0x22 (Tables 10/11). This is TIA-102.BABA-1's
// own half-rate addendum -- i.e. AMBE+2 -- confirmed by the manual's own section title, not this
// investigation's guess.
const RATET_HALF_RATE_FEC: u8 = 33;
const RATET_HALF_RATE_NOFEC: u8 = 34;

const TEST_FREQS_HZ: [f64; 10] =
    [50.0, 80.0, 100.0, 160.0, 200.0, 250.0, 400.0, 500.0, 800.0, 1000.0];

fn build_control_ratet(index: u8) -> Vec<u8> {
    let payload = vec![FIELD_RATET, index];
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}

fn build_speech(samples: &[i16]) -> Vec<u8> {
    let mut payload = vec![0x00_u8, samples.len() as u8];
    for &s in samples {
        payload.extend_from_slice(&s.to_be_bytes());
    }
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_SPEECH);
    pkt.extend_from_slice(&payload);
    pkt
}

fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    data.get(4..4 + length).map(|payload| (ptype, payload))
}

fn test_tone(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (8000.0 * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
        .collect()
}

/// Retries a send+recv round trip on transient `WouldBlock` timeouts -- this investigation has
/// repeatedly hit intermittent UDP timeouts under sustained chip load (see
/// `AMBE_CHIP_VALIDATION_FINDINGS.md`), which previously crashed this harness outright.
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 256], pkt: &[u8]) -> usize {
    for attempt in 0..8 {
        sock.send(pkt).expect("send");
        match sock.recv(buf) {
            Ok(n) => return n,
            Err(e) if attempt < 7 => {
                eprintln!("retrying after {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => panic!("recv channel after retries: {e}"),
        }
    }
    unreachable!()
}

/// Captures one settled frame per test frequency at the currently-configured rate, returning
/// `(true_freq_hz, raw_bits, num_bits)` for each.
fn capture_settled_frames(sock: &UdpSocket) -> Vec<(f64, Vec<u8>, usize)> {
    let mut buf = [0u8; 256];
    let mut settled = Vec::new();
    for &freq in &TEST_FREQS_HZ {
        let samples = test_tone(freq);
        let mut num_bits = 0usize;
        let mut last_frame = Vec::new();
        for _ in 0..(SETTLING_FRAMES + CAPTURED_FRAMES) {
            let n = send_recv_retrying(sock, &mut buf, &build_speech(&samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
            num_bits = payload[1] as usize;
            let nbytes = num_bits.div_ceil(8);
            last_frame = payload[2..2 + nbytes].to_vec();
        }
        settled.push((freq, last_frame, num_bits));
    }
    settled
}

/// Unpacks a byte slice into individual MSB-first bits.
fn to_bits(bytes: &[u8], num_bits: usize) -> Vec<u8> {
    let mut bits = Vec::with_capacity(num_bits);
    for &byte in bytes {
        for i in (0..8).rev() {
            bits.push((byte >> i) & 1);
            if bits.len() == num_bits {
                return bits;
            }
        }
    }
    bits
}

/// Packs the low `width` bits (MSB-first) of `bits[start..start+width]` into a `u32`.
fn window_value(bits: &[u8], start: usize, width: usize) -> u32 {
    let mut v = 0u32;
    for &b in &bits[start..start + width] {
        v = (v << 1) | b as u32;
    }
    v
}

fn gray_to_binary(g: u32) -> u32 {
    let mut b = g;
    let mut shift = 1;
    while (g >> shift) != 0 {
        b ^= g >> shift;
        shift += 1;
    }
    b
}

fn correlation(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let cov: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let vx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let vy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    if vx == 0.0 || vy == 0.0 {
        return 0.0;
    }
    cov / (vx.sqrt() * vy.sqrt())
}

fn spearman(xs: &[f64], ys: &[f64]) -> f64 {
    // Fractional (average) rank, correctly handling ties -- a real, previously latent bug found
    // while reusing this exact function in a sibling exploratory tool
    // (`ambe_plus_2_erasure_frame_digit_correlation_scan.rs`, AMBE_CHIP_VALIDATION_FINDINGS.md
    // section 40): assigning distinct ranks 0..n-1 by stable-sort order, even when many values are
    // exactly equal, silently produces spurious near-perfect correlations whenever a bit window is
    // constant or near-constant and the *input ordering itself* correlates with the target. This
    // tool's own final verdict never relied on this scan (the real check is zero-error Golay
    // decode), so the practical exposure here was low, but the function itself was still wrong.
    fn ranks(v: &[f64]) -> Vec<f64> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&a, &b| v[a].total_cmp(&v[b]));
        let mut r = vec![0.0; v.len()];
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
                j += 1;
            }
            let avg_rank = (i + j) as f64 / 2.0;
            for &k in &idx[i..=j] {
                r[k] = avg_rank;
            }
            i = j + 1;
        }
        r
    }
    correlation(&ranks(xs), &ranks(ys))
}

/// Turns a 49-bit `d[]`-order (FEC-codeword order) or contiguous-`b0`-first raw bit vector into a
/// pitch estimate, if `b0` classifies as a real speech frame.
fn pitch_from_b0(b0: u32) -> Option<f64> {
    match classify_b0(b0) {
        FrameKind::Speech => Some(tables::W0_TABLE[b0 as usize]),
        _ => None,
    }
}

fn run_nofec_test(sock: &UdpSocket) {
    println!("\n=== RATET({RATET_HALF_RATE_NOFEC}) -- AMBE+2 half-rate, No FEC (2450 bps, 49-bit frame) ===");
    sock.send(&build_control_ratet(RATET_HALF_RATE_NOFEC)).expect("send RATET config");
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("RATET config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid RATET ack");
    println!("RATET({RATET_HALF_RATE_NOFEC}) config ack: type={ptype:#04x} payload={payload:02x?}");

    let settled = capture_settled_frames(sock);
    for (freq, _frame, num_bits) in &settled {
        if *num_bits != 49 {
            eprintln!("warning: {freq}Hz: chip returned {num_bits} bits, expected 49 for this rate");
        }
    }

    let true_freqs: Vec<f64> = settled.iter().map(|(f, _, _)| *f).collect();
    let bit_rows: Vec<Vec<u8>> = settled
        .iter()
        .map(|(_, frame, num_bits)| to_bits(frame, *num_bits))
        .collect();

    // Structured hypothesis (a): treat the 49 raw bits directly as this codec's own d[] (FEC-
    // codeword) order and run them through the real extract_raw_parameters/classify_b0 path.
    println!("\n-- Hypothesis (a): raw 49 bits = d[] (FEC-codeword) order --");
    for (i, (freq, _, _)) in settled.iter().enumerate() {
        if bit_rows[i].len() < 49 {
            continue;
        }
        let mut d: u64 = 0;
        for &b in &bit_rows[i][..49] {
            d = (d << 1) | b as u64;
        }
        let raw = extract_raw_parameters(d);
        let pitch = pitch_from_b0(raw.b0);
        println!("  {freq:>6}Hz: b0={} kind={:?} w0={:?}", raw.b0, classify_b0(raw.b0), pitch);
    }

    // Structured hypothesis (b): plain contiguous b0..b8 fields, MSB-first, in that bit-width
    // order (7,5,5,9,7,5,4,4,3 = 49), no FEC-codeword reordering at all.
    println!("\n-- Hypothesis (b): contiguous b0..b8 fields (7,5,5,9,7,5,4,4,3) --");
    const WIDTHS: [usize; 9] = [7, 5, 5, 9, 7, 5, 4, 4, 3];
    for (i, (freq, _, _)) in settled.iter().enumerate() {
        if bit_rows[i].len() < 49 {
            continue;
        }
        let b0 = window_value(&bit_rows[i], 0, WIDTHS[0]);
        let raw = RawParameters {
            b0,
            b1: window_value(&bit_rows[i], 7, WIDTHS[1]),
            b2: window_value(&bit_rows[i], 12, WIDTHS[2]),
            b3: window_value(&bit_rows[i], 17, WIDTHS[3]),
            b4: window_value(&bit_rows[i], 26, WIDTHS[4]),
            b5: window_value(&bit_rows[i], 33, WIDTHS[5]),
            b6: window_value(&bit_rows[i], 38, WIDTHS[6]),
            b7: window_value(&bit_rows[i], 42, WIDTHS[7]),
            b8: window_value(&bit_rows[i], 46, WIDTHS[8]),
        };
        let pitch = pitch_from_b0(raw.b0);
        println!("  {freq:>6}Hz: b0={} kind={:?} w0={:?}", raw.b0, classify_b0(raw.b0), pitch);
    }

    // Hypothesis-agnostic sliding 7-bit-window correlation scan, plain and Gray-decoded, against
    // true frequency -- the same technique that found P25 full-rate's own Gray-coded u2 pitch
    // field (AMBE_CHIP_VALIDATION_FINDINGS.md section 9).
    println!("\n-- Sliding 7-bit-window correlation scan (plain and Gray-decoded) --");
    let min_len = bit_rows.iter().map(|b| b.len()).min().unwrap_or(0);
    let mut best: Option<(usize, bool, f64)> = None;
    for start in 0..min_len.saturating_sub(6) {
        let plain: Vec<f64> = bit_rows.iter().map(|b| window_value(b, start, 7) as f64).collect();
        let gray: Vec<f64> = bit_rows
            .iter()
            .map(|b| gray_to_binary(window_value(b, start, 7)) as f64)
            .collect();
        let sp_plain = spearman(&true_freqs, &plain);
        let sp_gray = spearman(&true_freqs, &gray);
        println!("  bits[{start}..{}): plain spearman={sp_plain:.3}  gray spearman={sp_gray:.3}", start + 7);
        for (is_gray, sp) in [(false, sp_plain), (true, sp_gray)] {
            if best.is_none_or(|(_, _, b)| sp.abs() > b.abs()) {
                best = Some((start, is_gray, sp));
            }
        }
    }
    if let Some((start, is_gray, sp)) = best {
        println!(
            "\n  Best candidate: bits[{start}..{}), {}, |spearman|={:.3}",
            start + 7,
            if is_gray { "Gray-decoded" } else { "plain binary" },
            sp.abs()
        );
    }
}

fn run_fec_test(sock: &UdpSocket) {
    println!("\n=== RATET({RATET_HALF_RATE_FEC}) -- AMBE+2 half-rate, with FEC (3600 bps, 72-bit frame) ===");
    sock.send(&build_control_ratet(RATET_HALF_RATE_FEC)).expect("send RATET config");
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("RATET config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid RATET ack");
    println!("RATET({RATET_HALF_RATE_FEC}) config ack: type={ptype:#04x} payload={payload:02x?}");

    let settled = capture_settled_frames(sock);
    for (freq, _, num_bits) in &settled {
        if *num_bits != 72 {
            eprintln!("warning: {freq}Hz: chip returned {num_bits} bits, expected 72 for this rate");
        }
    }

    for (framing_name, deinterleave) in [("direct C0||C1||C2||C3", false), ("Annex H deinterleaved", true)] {
        println!("\n-- Framing hypothesis: {framing_name} --");
        let mut zero_error_count = 0usize;
        let mut total = 0usize;
        let mut freq_pitch: Vec<(f64, u32, Option<f64>)> = Vec::new();
        for (freq, frame, num_bits) in &settled {
            if *num_bits != 72 || frame.len() < 9 {
                continue;
            }
            let mut wire: u128 = 0;
            for &byte in frame.iter().take(9) {
                wire = (wire << 8) | byte as u128;
            }
            let logical = if deinterleave { interleaved_to_frame(wire) } else { wire };
            let parsed = parse_frame(logical);
            total += 1;
            if parsed.epsilon_c0 == 0 && parsed.epsilon_c1 == 0 {
                zero_error_count += 1;
            }
            let raw = extract_raw_parameters(parsed.d);
            freq_pitch.push((*freq, raw.b0, pitch_from_b0(raw.b0)));
        }
        println!("  zero-error frames: {zero_error_count}/{total}");
        for (freq, b0, pitch) in &freq_pitch {
            println!("    {freq:>6}Hz: b0={b0} kind={:?} w0={pitch:?}", classify_b0(*b0));
        }
    }
}

fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

/// Real-speech validation of the FEC rate (33), using the already-confirmed-correct "Annex H
/// deinterleaved" framing (see `run_fec_test`'s own diagnostic output) -- a more representative
/// test than synthetic tones alone. Returns `true` if every captured frame Golay-decoded with zero
/// corrected errors.
fn run_fec_real_speech_test(sock: &UdpSocket) -> bool {
    println!("\n=== RATET({RATET_HALF_RATE_FEC}) real recorded speech ===");
    sock.send(&build_control_ratet(RATET_HALF_RATE_FEC)).expect("send RATET config");
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("RATET config response");
    parse_packet(&buf[..n]).expect("valid RATET ack");

    let speech_files = [
        "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
        "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    ];
    let mut all_ok = true;
    for path in speech_files {
        let pcm = read_wav_mono_i16(path);
        let n_frames = (pcm.len() / FRAME_SAMPLES).min(400);
        let mut zero_error_count = 0usize;
        for i in 0..n_frames {
            let frame_samples = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
            let n = send_recv_retrying(sock, &mut buf, &build_speech(frame_samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
            let num_bits = payload[1] as usize;
            if num_bits != 72 || payload.len() < 2 + 9 {
                continue;
            }
            let frame_bytes = &payload[2..2 + 9];
            let mut wire: u128 = 0;
            for &byte in frame_bytes {
                wire = (wire << 8) | byte as u128;
            }
            let logical = interleaved_to_frame(wire);
            let parsed = parse_frame(logical);
            if parsed.epsilon_c0 == 0 && parsed.epsilon_c1 == 0 {
                zero_error_count += 1;
            }
        }
        println!("  {path}: {zero_error_count}/{n_frames} frames zero-error (Annex H deinterleaved)");
        if zero_error_count != n_frames {
            all_ok = false;
        }
    }
    all_ok
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    run_nofec_test(&sock);
    run_fec_test(&sock);
    let speech_ok = run_fec_real_speech_test(&sock);
    if !speech_ok {
        eprintln!("\nFAIL: at least one real-speech frame did not decode with zero errors on RATET(33).");
        std::process::exit(1);
    }
    println!("\nPASS: every real-speech frame decoded with zero errors on RATET({RATET_HALF_RATE_FEC}).");
}

// SPDX-License-Identifier: LGPL-3.0-or-later
//! Single-trial building block for sweeping the busy-history recovery frame count from a FRESH
//! process each time -- necessary because testing multiple (busy-history, recovery) trials within
//! one continuous connection was found to be confounded (each trial's starting decoder state
//! depends on the previous trial's own drift, not a clean common baseline; see
//! `p25_ratet27_recovery_frame_count_sweep.rs`'s own doc comment for that finding). This tool does
//! exactly one trial per process invocation -- RATEP config, capture R and a silence reference,
//! run the same deterministic 100-flip busy history, apply the standard [20 silence, 4 tone]
//! conditioning, then `recovery_n` extra frames through BOTH the encoder (live silent PCM) and
//! decoder (pre-captured silence channel frame) paths per Bruce, then `PRIME_REPEATS` more tone
//! decodes, then measures the confirmed-real (68, 103) pair's dB spectral distance against a
//! freshly-established baseline -- and prints ONLY that one number, for a bash loop to aggregate
//! across many fresh invocations per recovery_n value.
//!
//! Usage: `cargo run --release --example p25_ratet27_recovery_n_single_trial -- <host:port> <recovery_n>`
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const TEST_FREQ_HZ: f64 = 200.0;
const SETTLING_FRAMES: usize = 80;
const TOTAL_BITS: usize = 144;
const PRIME_REPEATS: usize = 4;
const SILENCE_CONDITION_REPEATS: usize = 20;
const BUSY_HISTORY_LEN: usize = 100;
const TARGET_PAIR: [usize; 2] = [68, 103];

fn build_control_ratep(rcw: [u16; 6]) -> Vec<u8> {
    let mut payload = vec![FIELD_RATEP];
    for v in rcw {
        payload.extend_from_slice(&v.to_be_bytes());
    }
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
const BITS_OFFSET: usize = 6;
fn flip_bits_in_packet(raw_packet: &[u8], positions: &[usize]) -> Vec<u8> {
    let mut pkt = raw_packet.to_vec();
    for &k in positions {
        pkt[BITS_OFFSET + k / 8] ^= 1 << (7 - (k % 8));
    }
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
fn digital_silence() -> Vec<i16> {
    vec![0i16; FRAME_SAMPLES]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let recovery_n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    // "both" (default) exercises encoder+decoder during recovery; "decode_only" matches the
    // originally-successful test (decode-side silence only, no live re-encoding).
    let mode = args.get(3).cloned().unwrap_or_else(|| "both".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid DVSI packet");

    let samples = test_tone(TEST_FREQ_HZ);
    let mut r: Vec<u8> = Vec::new();
    for i in 0..SETTLING_FRAMES {
        sock.send(&build_speech(&samples)).expect("send speech");
        let n = sock.recv(&mut buf).expect("recv channel");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        assert_eq!(payload[1] as usize, TOTAL_BITS);
        if i == SETTLING_FRAMES - 1 {
            r = buf[..n].to_vec();
        }
    }

    let silence_samples = digital_silence();
    let mut silence_r: Vec<u8> = Vec::new();
    for i in 0..SETTLING_FRAMES {
        sock.send(&build_speech(&silence_samples)).expect("send speech");
        let n = sock.recv(&mut buf).expect("recv channel");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        assert_eq!(payload[1] as usize, TOTAL_BITS);
        if i == SETTLING_FRAMES - 1 {
            silence_r = buf[..n].to_vec();
        }
    }

    let send_channel_get_pcm = |sock: &UdpSocket, buf: &mut [u8; 512], raw_packet: &[u8]| -> Vec<i16> {
        sock.send(raw_packet).expect("send channel");
        let n = sock.recv(buf).expect("recv speech");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decode) response");
        payload[2..].chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]])).collect()
    };
    let send_speech_get_channel = |sock: &UdpSocket, buf: &mut [u8; 512], samples: &[i16]| {
        sock.send(&build_speech(samples)).expect("send speech");
        let n = sock.recv(buf).expect("recv channel");
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL (encode) response");
    };

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(FRAME_SAMPLES);
    const DB_FLOOR: f64 = 1.0;
    let db_spectrum = |samples: &[i16]| -> Vec<f64> {
        let mut buf: Vec<Complex64> = samples.iter().map(|&s| Complex64::new(s as f64, 0.0)).collect();
        fft.process(&mut buf);
        buf[..FRAME_SAMPLES / 2 + 1].iter().map(|c| 20.0 * (c.norm().max(DB_FLOOR)).log10()).collect()
    };
    let spectral_distance = |a: &[f64], b: &[f64]| -> f64 {
        let sum_sq: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum();
        (sum_sq / a.len() as f64).sqrt()
    };

    let condition = |sock: &UdpSocket, buf: &mut [u8; 512]| {
        for _ in 0..SILENCE_CONDITION_REPEATS {
            send_speech_get_channel(sock, buf, &silence_samples);
        }
        for _ in 0..SILENCE_CONDITION_REPEATS {
            send_channel_get_pcm(sock, buf, &silence_r);
        }
        for _ in 0..PRIME_REPEATS {
            send_channel_get_pcm(sock, buf, &r);
        }
    };

    // Replicates p25_ratet27_pairflip_diagnose_hit.rs's EXACT preceding sequence (baseline
    // capture, a fresh flip test, a same-connection retest) before the busy history -- found to
    // matter: skipping straight to busy history from a "cleaner" state gave a different result
    // (6.13 dB) than reproducing the fuller sequence.
    condition(&sock, &mut buf);
    let baseline = send_channel_get_pcm(&sock, &mut buf, &r);
    let baseline_spectrum = db_spectrum(&baseline);

    let flipped = flip_bits_in_packet(&r, &TARGET_PAIR);

    condition(&sock, &mut buf);
    let _fresh_pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);

    condition(&sock, &mut buf);
    let _retest_pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);

    // Fixed-seed busy history, deterministic and identical across every fresh invocation. Matches
    // diagnose_hit.rs's EXACT structure: TWO separate rounds of BUSY_HISTORY_LEN flips with a
    // condition() + intermediate measurement between them, and the LCG state CONTINUING across
    // both rounds (not reset) -- the 11.03 dB recovery value was measured after 200 cumulative
    // busy flips, not 100.
    let mut lcg_state: u32 = 0xC0FFEE;
    let rand_bit = |lcg_state: &mut u32| -> usize {
        *lcg_state ^= *lcg_state << 13;
        *lcg_state ^= *lcg_state >> 17;
        *lcg_state ^= *lcg_state << 5;
        (*lcg_state as usize) % TOTAL_BITS
    };
    for _ in 0..BUSY_HISTORY_LEN {
        let a = rand_bit(&mut lcg_state);
        let b = rand_bit(&mut lcg_state);
        let busy_flip = flip_bits_in_packet(&r, &[a, b]);
        send_channel_get_pcm(&sock, &mut buf, &busy_flip);
    }
    condition(&sock, &mut buf);
    let _after_busy_pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);

    for _ in 0..BUSY_HISTORY_LEN {
        let a = rand_bit(&mut lcg_state);
        let b = rand_bit(&mut lcg_state);
        let busy_flip = flip_bits_in_packet(&r, &[a, b]);
        send_channel_get_pcm(&sock, &mut buf, &busy_flip);
    }

    condition(&sock, &mut buf);
    if mode == "both" {
        for _ in 0..recovery_n {
            send_speech_get_channel(&sock, &mut buf, &silence_samples);
        }
    }
    for _ in 0..recovery_n {
        send_channel_get_pcm(&sock, &mut buf, &silence_r);
    }
    for _ in 0..PRIME_REPEATS {
        send_channel_get_pcm(&sock, &mut buf, &r);
    }
    let pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
    let distance = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm));

    // Single-line, machine-parseable output for a bash loop to aggregate.
    println!("{recovery_n}\t{mode}\t{distance:.2}");
}

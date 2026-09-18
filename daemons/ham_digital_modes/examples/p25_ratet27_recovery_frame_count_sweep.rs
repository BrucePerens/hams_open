// SPDX-License-Identifier: LGPL-3.0-or-later
//! **SUPERSEDED** by `p25_ratet27_recovery_n_single_trial.rs`: this tool's design (multiple
//! (busy-history, recovery, measure) trials run back-to-back within ONE continuous connection) was
//! found to be methodologically confounded -- it produced high variance *within* a single N's own
//! repeated trials (e.g. N=0 gave `[4.28, 4.80, 10.51]` dB across 3 same-connection trials), proving
//! sequential trials sharing one connection do not share a common baseline. Kept as a documented
//! negative finding (see `AMBE_CHIP_VALIDATION_FINDINGS.md` §16), not as a working sweep tool -- use
//! `p25_ratet27_recovery_n_single_trial.rs` (one fresh process per measurement) instead.
//!
//! Per Bruce's suggestion: sweeps the number of extra silence frames used in the targeted
//! busy-history recovery pass (see `p25_ratet27_pairflip_diagnose_hit.rs`'s `condition()` doc
//! comment) to find which counts consistently recover a degraded effect, rather than assuming the
//! one value (3) that happened to work in the first test generalizes.
//!
//! For each candidate recovery-frame count N: replay the SAME deterministic 100-flip "busy
//! history" (a fixed-seed LCG, reset before each N so every N sees identical prior history), then
//! apply the standard [20 silence, 4 tone] conditioning, then N extra silence frames THROUGH BOTH
//! PATHS (N live-encoded silent PCM frames, discarding the channel output, plus N decodes of a
//! pre-captured silence channel frame -- per Bruce, priming needs to cover both the encoder and
//! the decoder, not decode-only), then 4 more tone decodes, then measure the confirmed-real
//! `(68, 103)` pair's dB spectral distance. A value consistently near the fresh-boot reference
//! (12.41 dB) across repeated trials at that N is a genuinely reliable recovery count; a value
//! that lands near the degraded floor (~4-5 dB) is not.
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
const RECOVERY_COUNTS_TO_TRY: [usize; 11] = [0, 1, 2, 3, 4, 5, 6, 8, 10, 15, 20];
const TRIALS_PER_COUNT: usize = 3;
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
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
    println!("RATEP(P25 FEC) config ack: type={ptype:#04x} payload={payload:02x?}");

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
    println!("captured reference frame R: {r:02x?}");

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
    println!("captured silence conditioning frame: {silence_r:02x?}");

    let send_channel_get_pcm = |sock: &UdpSocket, buf: &mut [u8; 512], raw_packet: &[u8]| -> Vec<i16> {
        sock.send(raw_packet).expect("send channel");
        let n = sock.recv(buf).expect("recv speech");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decode) response");
        payload[2..].chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]])).collect()
    };
    // Exercises the ENCODER with live silent PCM, discarding the resulting channel bits -- per
    // Bruce, priming needs to cover both paths, not just decode-side silence.
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
            send_channel_get_pcm(sock, buf, &silence_r);
        }
        for _ in 0..PRIME_REPEATS {
            send_channel_get_pcm(sock, buf, &r);
        }
    };

    condition(&sock, &mut buf);
    let baseline = send_channel_get_pcm(&sock, &mut buf, &r);
    let baseline_spectrum = db_spectrum(&baseline);

    condition(&sock, &mut buf);
    let flipped = flip_bits_in_packet(&r, &TARGET_PAIR);
    let fresh_pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
    let fresh_distance = spectral_distance(&baseline_spectrum, &db_spectrum(&fresh_pcm));
    println!("\nfresh-boot reference distance for {TARGET_PAIR:?}: {fresh_distance:.2} dB\n");

    let make_busy_history = |sock: &UdpSocket, buf: &mut [u8; 512]| {
        let mut lcg_state: u32 = 0xC0FFEE;
        for _ in 0..BUSY_HISTORY_LEN {
            lcg_state ^= lcg_state << 13;
            lcg_state ^= lcg_state >> 17;
            lcg_state ^= lcg_state << 5;
            let a = (lcg_state as usize) % TOTAL_BITS;
            lcg_state ^= lcg_state << 13;
            lcg_state ^= lcg_state >> 17;
            lcg_state ^= lcg_state << 5;
            let b = (lcg_state as usize) % TOTAL_BITS;
            let busy_flip = flip_bits_in_packet(&r, &[a, b]);
            send_channel_get_pcm(sock, buf, &busy_flip);
        }
    };

    println!("--- sweeping recovery-pass silence-frame count (target: {TARGET_PAIR:?}, fresh reference {fresh_distance:.2} dB) ---");
    println!("{} trials per count, {} busy flip-decodes before each trial\n", TRIALS_PER_COUNT, BUSY_HISTORY_LEN);
    for &recovery_n in &RECOVERY_COUNTS_TO_TRY {
        let mut distances = Vec::new();
        for _ in 0..TRIALS_PER_COUNT {
            make_busy_history(&sock, &mut buf);
            condition(&sock, &mut buf);
            for _ in 0..recovery_n {
                send_speech_get_channel(&sock, &mut buf, &silence_samples);
            }
            for _ in 0..recovery_n {
                send_channel_get_pcm(&sock, &mut buf, &silence_r);
            }
            for _ in 0..PRIME_REPEATS {
                send_channel_get_pcm(&sock, &mut buf, &r);
            }
            let pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
            distances.push(spectral_distance(&baseline_spectrum, &db_spectrum(&pcm)));
        }
        let avg = distances.iter().sum::<f64>() / distances.len() as f64;
        println!("recovery_n={recovery_n:>2}: distances={distances:.2?} avg={avg:.2} dB");
    }

    condition(&sock, &mut buf);
}

// SPDX-License-Identifier: LGPL-3.0-or-later
//! Captures real DVSI-chip RATET(27) channel frames (raw 144-bit CHAND payloads) across a variety
//! of stimulus signals, for offline analysis: deinterleave via the validated 12x12 transform
//! (`ambe::dvsi_p25fec::wire_format::natural_position`), split into the 8 FEC sub-blocks, and compute
//! the GF(2) rank of each block's observed values across many frames. A block whose wire bits are
//! genuinely the FEC codeword (no extra data-dependent modulation/whitening XORed in) should show
//! rank exactly 12 (Golay) / 11 (Hamming) / <=7 (raw `c7`) -- higher rank means something else
//! (e.g. a PRN sequence derived from other parameters, as `super::modulation` does for the textbook
//! IMBE encoder) is mixed into the wire bits, which this capture is designed to detect directly
//! rather than assume either way.
//!
//! Prints one hex line per captured frame (18 bytes = 144 bits, MSB-first within each byte, matching
//! `p25_ratet27_hamming_block_falsification_test.rs`'s own `BITS_OFFSET=6` convention) to stdout,
//! prefixed with the stimulus label -- meant to be redirected to a file and analyzed offline in
//! Python (zero further chip time needed after capture).
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_frames -- <host:port> > frames.tsv`
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 20;
const CAPTURE_FRAMES: usize = 10;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;

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
fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    data.get(4..4 + length).map(|payload| (ptype, payload))
}

fn sine(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (8000.0 * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
        .collect()
}
fn sawtooth(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            (6000.0 * (2.0 * phase - 1.0)) as i16
        })
        .collect()
}
fn amplitude_ramp(freq: f64, peak: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let env = peak * (n as f64 / FRAME_SAMPLES as f64);
            (env * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16
        })
        .collect()
}
/// Pseudo-random (LCG) full-band "noise" PCM, a cheap way to maximize the diversity of quantized
/// spectral-amplitude/pitch parameter values hit per frame -- much broader coverage per chip
/// round-trip than another pure tone, for the rank-of-observed-codewords analysis this capture
/// feeds (see this file's own module doc).
fn lcg_noise(seed: u64, peak: f64) -> Vec<i16> {
    let mut state = seed;
    (0..FRAME_SAMPLES)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let unit = ((state >> 33) as f64 / (1u64 << 31) as f64) - 1.0; // roughly [-1,1)
            (unit * peak) as i16
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid DVSI packet");

    let mut stimuli: Vec<(String, Vec<i16>)> = vec![
        ("sawtooth_200".to_string(), sawtooth(200.0)),
        ("ramp_200".to_string(), amplitude_ramp(200.0, 6000.0)),
        ("silence".to_string(), vec![0i16; FRAME_SAMPLES]),
    ];
    // Voice-range frequencies (period evenly divides 160, matching this crate's existing chip-check
    // convention) at both sine and sawtooth, to spread pitch-parameter coverage broadly.
    for &freq in &[50u32, 80, 100, 125, 160, 200, 250, 320, 400] {
        stimuli.push((format!("sine_{freq}"), sine(freq as f64)));
        stimuli.push((format!("sawtooth_{freq}"), sawtooth(freq as f64)));
    }

    // Retry wrapper around one send+recv round-trip: the chip's UDP link occasionally drops a
    // response under sustained load (a known, intermittent WouldBlock this session has hit
    // repeatedly), which previously crashed long-running sweeps outright.
    let send_recv_retrying = |sock: &UdpSocket, buf: &mut [u8; 512], pkt: &[u8]| -> usize {
        for attempt in 0..5 {
            sock.send(pkt).expect("send");
            match sock.recv(buf) {
                Ok(n) => return n,
                Err(e) if attempt < 4 => {
                    eprintln!("retrying after {e}");
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => panic!("recv channel after retries: {e}"),
            }
        }
        unreachable!()
    };
    let capture_frame =
        |sock: &UdpSocket, buf: &mut [u8; 512], label: &str, idx: usize, samples: &[i16]| {
            let n = send_recv_retrying(sock, buf, &build_speech(samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
            println!("{label}\t{idx}\t{hex}");
        };

    for (label, samples) in &stimuli {
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            capture_frame(&sock, &mut buf, label, i, samples);
        }
    }

    // Pseudo-random noise: a fresh random buffer every single frame (settling included), so each
    // capture is a genuinely distinct stimulus -- cheap, broad coverage of quantizer bins.
    const NOISE_CAPTURES: usize = 60;
    for i in 0..(SETTLING_FRAMES / 2 + NOISE_CAPTURES) {
        let samples = lcg_noise(0x9E3779B97F4A7C15_u64.wrapping_add(i as u64), 7000.0);
        if i < SETTLING_FRAMES / 2 {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        } else {
            capture_frame(&sock, &mut buf, "noise", i, &samples);
        }
    }
}

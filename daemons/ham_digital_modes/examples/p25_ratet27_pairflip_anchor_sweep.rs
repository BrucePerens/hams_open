// SPDX-License-Identifier: LGPL-3.0-or-later
//! Extends the single-bit-flip oracle (`p25_ratet27_bitflip_oracle.rs`) to PAIRS of bits, to
//! empirically discover FEC codeword boundaries among the 142 wire positions that single-flip
//! testing found no effect from (all except the two confirmed-unprotected bits, 131 and 143).
//!
//! A single bit flip inside any FEC-protected codeword (Golay(23,12) or Hamming(15,11), textbook
//! IMBE) is always correctable, hence the earlier all-clear. But Hamming(15,11) has minimum distance
//! 3 and corrects only 1 error -- so flipping TWO bits that share the same Hamming codeword should
//! change the decoded output (uncorrectable), while flipping two bits in different codewords (or two
//! bits sharing a Golay(23,12) codeword, which corrects up to 3 errors) should still show no effect.
//! A "changed" pair result is therefore a direct, convention-free proof that both bits belong to the
//! same Hamming codeword -- true regardless of any unknown interleave, byte order, or PN-modulation,
//! for the same reason the single-bit oracle worked: a wire bit's codeword membership is invariant
//! under any transformation applied identically to baseline and flipped frames.
//!
//! This is a bounded ANCHOR SWEEP, not the full C(142,2) exhaustive pairing (~10000 pairs, a long
//! background run) -- a handful of anchor positions spread across the frame, each tested against
//! every other candidate position. If an anchor is Hamming-protected, sweeping it against all others
//! should surface its ~14 real codeword partners directly (every "changed" result at once). If an
//! anchor is Golay-protected or otherwise never triggers a "changed" pair, that's inconclusive (not
//! proof of anything) but still narrows the search for a follow-up full sweep.
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
const BASELINE_REPEATS: usize = 5;
// Confirmed unprotected by the single-bit oracle -- excluded as candidates (pairing with a
// genuinely-inert bit can never show a change, so including them would waste round trips).
const KNOWN_UNPROTECTED: [usize; 2] = [131, 143];
const ANCHORS: [usize; 8] = [0, 18, 36, 54, 72, 90, 108, 126];

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
        let num_bits = payload[1] as usize;
        assert_eq!(num_bits, TOTAL_BITS, "expected 144 bits for RATEP P25 FEC");
        if i == SETTLING_FRAMES - 1 {
            r = buf[..n].to_vec();
        }
    }
    println!("captured reference frame R (raw packet, {} bytes): {r:02x?}", r.len());

    let send_channel_get_pcm = |sock: &UdpSocket, buf: &mut [u8; 512], raw_packet: &[u8]| -> Vec<i16> {
        sock.send(raw_packet).expect("send channel");
        let n = sock.recv(buf).expect("recv speech");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decode) response");
        payload[2..].chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]])).collect()
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

    for _ in 0..BASELINE_REPEATS - 1 {
        send_channel_get_pcm(&sock, &mut buf, &r);
    }
    let baseline_pcm = send_channel_get_pcm(&sock, &mut buf, &r);
    let baseline_spectrum = db_spectrum(&baseline_pcm);

    let mut floor_distances = Vec::new();
    for _ in 0..8 {
        for _ in 0..PRIME_REPEATS {
            send_channel_get_pcm(&sock, &mut buf, &r);
        }
        let pcm = send_channel_get_pcm(&sock, &mut buf, &r);
        floor_distances.push(spectral_distance(&baseline_spectrum, &db_spectrum(&pcm)));
    }
    let noise_floor = floor_distances.iter().cloned().fold(0.0_f64, f64::max);
    let threshold = (noise_floor * 5.0).max(5.0);
    println!("noise floor (max of 8 repeats): {noise_floor:.2} dB; using changed-decode threshold = {threshold:.2} dB");

    let candidates: Vec<usize> = (0..TOTAL_BITS).filter(|k| !KNOWN_UNPROTECTED.contains(k)).collect();

    println!("\n--- anchor sweep: {} anchors x {} candidates each ---", ANCHORS.len(), candidates.len() - 1);
    for &anchor in &ANCHORS {
        println!("\nanchor bit {anchor}:");
        let mut partners = Vec::new();
        for &other in &candidates {
            if other == anchor {
                continue;
            }
            for _ in 0..PRIME_REPEATS {
                send_channel_get_pcm(&sock, &mut buf, &r);
            }
            let flipped = flip_bits_in_packet(&r, &[anchor, other]);
            let pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
            let distance = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm));
            if distance > threshold {
                partners.push((other, distance));
            }
        }
        if partners.is_empty() {
            println!("  no partner found -- likely Golay-protected, or its true Hamming codeword wasn't in this sweep's candidate set (shouldn't happen; all 142 non-unprotected bits are candidates)");
        } else {
            println!("  {} partner(s) found (pairing with anchor changes decode): {partners:?}", partners.len());
        }
    }
    // Re-converge once more at the end, leaving the decoder in a clean state.
    for _ in 0..PRIME_REPEATS {
        send_channel_get_pcm(&sock, &mut buf, &r);
    }
}

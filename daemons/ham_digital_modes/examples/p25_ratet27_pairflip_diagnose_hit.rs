// SPDX-License-Identifier: LGPL-3.0-or-later
//! One-shot diagnostic: reproduces a single pair-flip "hit" from the full sweep and dumps the full
//! PCM and RMS energy of both the baseline and flipped decode, to check whether a "hit" reflects a
//! genuine different-but-tone-like decode (consistent with real codeword miscorrection) or a muted/
//! near-silent fallback (consistent with the decoder detecting an over-threshold frame error count
//! and rejecting the frame outright, per the concern raised during review) -- these look identical
//! under a plain dB-spectral-distance threshold but mean very different things for interpreting the
//! full sweep's results.
//!
//! **A real, significant complication found while verifying the full sweep's hits**: pair (8, 93)
//! reproduced identically across two fully independent fresh-boot runs of this tool, yet never even
//! registered in the full sweep's own first-pass results for that pair. The two runs differ in one
//! way: this tool always primes with the SAME short, fixed sequence from a fresh connection, while
//! the sweep tests thousands of different bit-pairs in sequence before reaching any given pair. That
//! implies the chip's decoder carries state beyond what a short re-priming burst resets -- something
//! that only converges to a truly canonical starting point via a longer, deliberate reset, not
//! whatever residual state a long, varied test sequence happens to leave behind. Per Bruce's
//! suggestion, this tool now conditions with digital silence (which earlier testing showed converges
//! decoder state to a small, near-fixed point, unlike a voiced tone's continuously-tracked phase)
//! before every priming step, not just once at the start -- forcing a canonical reset before each
//! test rather than relying on short-term re-priming to erase arbitrary prior history.
use rustfft::{num_complex::Complex64, FftPlanner};
use std::env;
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
// Conditioning: enough silence decodes to force convergence to a canonical, history-independent
// state (per the earlier finding that silence has no pitch/phase to track, unlike a voiced tone),
// before priming back up to the tone's own steady state for the actual test.
const SILENCE_CONDITION_REPEATS: usize = 20;

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
fn rms(samples: &[i16]) -> f64 {
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
    (sum_sq / samples.len() as f64).sqrt()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());
    // Accepts 2 or more bit positions (all args after the host), flipped together as one group --
    // extended beyond a fixed pair to test whether a 3-or-more-way flip within a suspected
    // redundancy/majority-vote group matches the same result as any 2-of-N subset.
    let positions: Vec<usize> = if args.len() > 3 {
        args[2..].iter().map(|s| s.parse().expect("bit position")).collect()
    } else {
        vec![
            args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8),
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(93),
        ]
    };

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

    // Also capture silence's own encoded channel frame, for decoder conditioning.
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

    // Forces a canonical, history-independent state before a test: decode silence repeatedly
    // (converges to a small, near-fixed point regardless of whatever came before), then prime with
    // R to converge to the tone's own steady state. Called before EVERY decode of interest, not
    // just once at the start.
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
    println!("\nbaseline PCM (unmodified R, after silence conditioning): rms={:.2}", rms(&baseline));
    println!("baseline first 20 samples: {:?}", &baseline[..20]);

    condition(&sock, &mut buf);
    let flipped = flip_bits_in_packet(&r, &positions);
    let pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
    let distance = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm));
    println!("\nflipped {positions:?} PCM (after silence conditioning): rms={:.2}", rms(&pcm));
    println!("flipped first 20 samples: {:?}", &pcm[..20]);
    println!("dB spectral distance from baseline: {distance:.2} dB (sweep's own threshold was 5.00 dB)");

    println!("\nrms ratio (flipped/baseline): {:.4}", rms(&pcm) / rms(&baseline).max(1.0));

    // Re-test from the SAME connection, but conditioned again from silence each time -- if
    // conditioning is what was missing, this retest should now agree with the first result.
    condition(&sock, &mut buf);
    let pcm2 = send_channel_get_pcm(&sock, &mut buf, &flipped);
    let distance2 = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm2));
    println!(
        "conditioned same-connection retest {positions:?}: rms={:.2}, distance={:.2} dB",
        rms(&pcm2),
        distance2
    );

    // Also try flipping each position alone, for comparison (each should show no significant
    // single-bit effect per the earlier single-bit oracle results).
    for &single in &positions {
        condition(&sock, &mut buf);
        let flipped_single = flip_bits_in_packet(&r, &[single]);
        let pcm_single = send_channel_get_pcm(&sock, &mut buf, &flipped_single);
        println!("single flip ({single}) alone: rms={:.2}", rms(&pcm_single));
    }

    condition(&sock, &mut buf);
}

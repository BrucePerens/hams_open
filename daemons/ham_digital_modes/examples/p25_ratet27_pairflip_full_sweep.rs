// SPDX-License-Identifier: LGPL-3.0-or-later
//! The full, exhaustive version of `p25_ratet27_pairflip_anchor_sweep.rs`'s 8-anchor sample: every
//! one of the C(142,2) = 10011 pairs among the 142 wire positions that showed no single-bit effect
//! (all except the two confirmed-unprotected bits, 131 and 143), tested against the real chip.
//!
//! **A real methodological problem found and fixed in this version**: the first attempt at this full
//! sweep (without silence conditioning, just 4x re-priming with the tone reference frame R before
//! each test) found hundreds of apparent "hits" that turned out to be spurious -- a per-hit
//! confirmation retest immediately filtered nearly all of them, down to 6. But manually verifying
//! those 6 against `p25_ratet27_pairflip_diagnose_hit.rs` found an even deeper problem: pair (8, 93),
//! which reproduced identically across two fully independent fresh-boot diagnostic runs, never
//! registered as a hit in the sweep at all -- meaning the chip's decoder carries state beyond what a
//! short re-priming burst resets, and a long, varied test sequence (thousands of prior different
//! bit-pair tests) leaves the decoder in a different residual state than a fresh boot does. Per
//! Bruce's suggestion, this version conditions with digital silence (which independently-verified
//! testing showed converges decoder state to a small, near-fixed point, unlike a voiced tone's
//! continuously-tracked phase) before EVERY test, not just relying on short re-priming with the tone
//! alone -- forcing a canonical, history-independent starting point each time. With this fix, hand-
//! verification found something real: {8, 92, 127} and {68, 103, 127} are each 3-way "majority vote"
//! style groups where any 2-of-3 (or all 3) flipped together produce byte-identical decoded output,
//! distinct from baseline, while any single member alone shows no effect -- exactly the signature of
//! a genuine redundancy/repetition-coded bit, not noise. The unconditioned sweep had already MISSED
//! one member of this confirmed structure ((92, 127)), directly demonstrating why this full run needed
//! to be redone with conditioning rather than trusting the first pass.
//!
//! **Two more bugs found and fixed before this version's first real run.** (1) The baseline itself
//! was captured via plain tone-only re-priming while every other decode used full silence
//! conditioning -- comparing two different decoder states, which inflated the measured noise floor
//! to ~21 dB and would have set the hit threshold (5x floor) far above even the confirmed real
//! {8,92,127}/{68,103,127} effects (~10-14 dB). Fixed by conditioning the baseline capture too. (2)
//! Even after that fix, a silence-then-tone "relock" transient turned out to have a heavier-tailed
//! noise floor than pure tone-only re-priming (occasional spikes up to ~9 dB, versus ~0.4 dB for
//! tone-only) -- high enough that the confirmed-real but weak (128, 139) pair (5.56 dB) could be
//! indistinguishable from an unlucky noise draw under a naive max-of-N threshold. Fixed by using the
//! MEDIAN of 20 calibration samples as the robust floor estimate (not swayed by rare outliers) and
//! reinstating a per-hit confirmation retest (an independent second draw): a genuine effect
//! reproduces reliably, a noise spike usually doesn't land above threshold twice in a row.
//!
//! Expected runtime: ~10000 pairs x up to 30 round trips (20 silence-conditioning decodes + 4
//! re-priming decodes + 1 flipped-frame decode, plus another 25 for confirmation on any hit) at this
//! protocol's per-frame UDP round-trip pace -- on the order of 2.5-4 hours, depending on how many
//! candidates need confirmation. Progress is printed periodically so a background run's log shows
//! live progress.
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

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
const KNOWN_UNPROTECTED: [usize; 2] = [131, 143];

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
        let num_bits = payload[1] as usize;
        assert_eq!(num_bits, TOTAL_BITS, "expected 144 bits for RATEP P25 FEC");
        if i == SETTLING_FRAMES - 1 {
            r = buf[..n].to_vec();
        }
    }
    println!("captured reference frame R (raw packet, {} bytes): {r:02x?}", r.len());

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

    // Retries on a UDP timeout (a real, observed transient hiccup over a long run) by resending
    // the same packet, rather than panicking the whole sweep over one dropped datagram.
    let send_channel_get_pcm = |sock: &UdpSocket, buf: &mut [u8; 512], raw_packet: &[u8]| -> Vec<i16> {
        for attempt in 0..5 {
            sock.send(raw_packet).expect("send channel");
            match sock.recv(buf) {
                Ok(n) => {
                    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
                    assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decode) response");
                    return payload[2..].chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]])).collect();
                }
                Err(e) if attempt < 4 => {
                    eprintln!("  (recv timeout, attempt {attempt}: {e} -- retrying)");
                }
                Err(e) => panic!("recv speech failed after 5 attempts: {e}"),
            }
        }
        unreachable!()
    };

    // Forces a canonical, history-independent state before a test: decode silence repeatedly
    // (converges to a small, near-fixed point regardless of whatever came before), then prime with
    // R to converge to the tone's own steady state.
    let condition = |sock: &UdpSocket, buf: &mut [u8; 512]| {
        for _ in 0..SILENCE_CONDITION_REPEATS {
            send_channel_get_pcm(sock, buf, &silence_r);
        }
        for _ in 0..PRIME_REPEATS {
            send_channel_get_pcm(sock, buf, &r);
        }
    };

    // Tried a Hann window here to reduce phase-dependent spectral leakage (the suspected cause of
    // the noise floor's wide spread) -- it changed the distribution's shape but didn't clearly
    // improve separation from the confirmed real effects, so reverted to plain rectangular FFT
    // (matching the already-verified diagnostic tool) rather than chase further DSP tuning. The
    // wide, heavy-tailed noise floor is accepted as a real property of this comparison method (see
    // the threshold rationale below) rather than something to eliminate before this run.
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

    // Baseline must be captured via the SAME conditioning protocol used for every subsequent
    // test -- an earlier version captured it via plain tone-only re-priming (matching the
    // unconditioned sweep) while every other decode used full silence conditioning, comparing two
    // different decoder states and inflating the noise floor to ~21 dB (versus ~0.5-1 dB expected),
    // which would have set the hit threshold far above even the confirmed real {8,92,127}/
    // {68,103,127} effects (~10-14 dB) -- caught before wasting the ~3 hour run on it.
    condition(&sock, &mut buf);
    let baseline_pcm = send_channel_get_pcm(&sock, &mut buf, &r);
    let baseline_spectrum = db_spectrum(&baseline_pcm);

    // 20 samples, not 8: a fresh silence-then-tone relock apparently has heavier-tailed noise than
    // pure tone-only re-priming did (observed up to ~9-21 dB spikes vs ~0.4 dB for tone-only), so a
    // plain max-of-N floor is dominated by rare outliers rather than the typical case. Using the
    // median as the robust floor estimate, with confirmation retesting (below) to catch the rare
    // case where a real, weak effect (the confirmed (128,139) pair measured at 5.56 dB, below some
    // observed noise spikes) needs a second independent draw to separate it from noise.
    let mut floor_distances = Vec::new();
    for _ in 0..20 {
        condition(&sock, &mut buf);
        let pcm = send_channel_get_pcm(&sock, &mut buf, &r);
        floor_distances.push(spectral_distance(&baseline_spectrum, &db_spectrum(&pcm)));
    }
    floor_distances.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_floor = floor_distances[floor_distances.len() / 2];
    let max_floor = floor_distances[floor_distances.len() - 1];
    // A multiplicative threshold (Nx median) doesn't work here: the noise floor's own median
    // (observed ~5 dB) times any reasonable multiplier already exceeds the confirmed real
    // {8,92,127}/{68,103,127} effects (~10-14 dB), which would never even reach confirmation. Use
    // an additive margin instead, low enough to admit real 10+ dB effects into the confirmation
    // step, accepting that this also admits a fair share of the noise tail -- confirmation (an
    // independent second draw) is what actually separates them, not the first-pass threshold alone.
    // This also means a weak real effect at or below the noise floor's own median (the previously,
    // separately confirmed (128, 139) pair at 5.56 dB) is not reliably detectable by this automated
    // pass; that's an accepted, documented limitation, not something this run needs to fix, since
    // that pair is already confirmed by direct, targeted testing regardless of this sweep's outcome.
    let threshold = (median_floor + 3.0).max(6.0);
    println!(
        "noise floor: median={median_floor:.2} dB, max={max_floor:.2} dB (of 20 conditioned repeats); using changed-decode threshold = {threshold:.2} dB"
    );
    println!("all 20 floor samples: {floor_distances:.2?}");

    let candidates: Vec<usize> = (0..TOTAL_BITS).filter(|k| !KNOWN_UNPROTECTED.contains(k)).collect();
    let total_pairs = candidates.len() * (candidates.len() - 1) / 2;
    println!("\n--- full exhaustive sweep (silence-conditioned): {} candidates, {total_pairs} pairs ---", candidates.len());

    let start = Instant::now();
    let mut tested = 0usize;
    let mut hits: Vec<(usize, usize, f64)> = Vec::new();
    for (idx_i, &a) in candidates.iter().enumerate() {
        for &b in &candidates[idx_i + 1..] {
            condition(&sock, &mut buf);
            let flipped = flip_bits_in_packet(&r, &[a, b]);
            let pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
            let distance = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm));
            tested += 1;
            if distance > threshold {
                // Confirm with a second, independently-conditioned draw: the noise floor's own
                // occasional spikes (up to ~9-21 dB observed) can exceed a real but weak effect
                // (the confirmed (128,139) pair measures only 5.56 dB), so a single measurement
                // above threshold isn't decisive on its own -- a genuine effect reproduces on an
                // independent redraw, a noise spike usually doesn't land above threshold twice.
                condition(&sock, &mut buf);
                let confirm_pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
                let confirm_distance = spectral_distance(&baseline_spectrum, &db_spectrum(&confirm_pcm));
                if confirm_distance > threshold {
                    hits.push((a, b, distance));
                    println!("  HIT: ({a}, {b}) distance={distance:.2} dB (confirmed: {confirm_distance:.2} dB)");
                } else {
                    println!("  (not confirmed: ({a}, {b}) first={distance:.2} dB, retest={confirm_distance:.2} dB)");
                }
            }
            if tested.is_multiple_of(200) {
                let elapsed = start.elapsed().as_secs_f64();
                let rate = tested as f64 / elapsed;
                let remaining = (total_pairs - tested) as f64 / rate;
                println!(
                    "  progress: {tested}/{total_pairs} pairs ({:.1}%), {:.1}s elapsed, ~{:.0}s remaining, {} hits so far",
                    100.0 * tested as f64 / total_pairs as f64,
                    elapsed,
                    remaining,
                    hits.len()
                );
            }
        }
    }

    println!("\n=== full sweep complete: {tested} pairs tested in {:.1}s ===", start.elapsed().as_secs_f64());
    println!("{} hits found (pairs whose joint flip changed decoded spectrum beyond threshold):", hits.len());
    for (a, b, d) in &hits {
        println!("  ({a}, {b}): {d:.2} dB");
    }
    if hits.is_empty() {
        println!("Clean negative across the full exhaustive space.");
    }

    condition(&sock, &mut buf);
}

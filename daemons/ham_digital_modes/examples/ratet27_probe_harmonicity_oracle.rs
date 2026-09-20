// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(clippy::needless_range_loop, clippy::type_complexity)]
//! Uses the *input speech itself* and the *chip's own output PCM* as independent oracles for which
//! harmonics are truly periodic (voiced), to test whether this crate's decoded per-harmonic voicing
//! (`decode_voicing_decisions`, spec-literal band order) is right -- `ratet27_diagnose_harmonic_
//! extension.rs` found harmonics 1-6 decode unvoiced in every predominantly-voiced frame, which is
//! backwards from ordinary voiced-speech acoustics.
//!
//! For frames with stable decoded pitch (`b0` within +-2 across the frame and both neighbors,
//! `f0 >= 95 Hz`, `l_hat >= 20`), measures a *harmonicity* per harmonic `k` on a Hann-windowed
//! 3-frame (480-sample) span of each stream: peak magnitude near `k*f0` versus the trough near
//! `(k+0.5)*f0`, in dB. A truly voiced harmonic shows a large positive dB; noise-like content shows
//! ~0 dB. Prints the mean per `k` for: the input speech, the chip's decoded PCM, this crate's float
//! PCM, and the fraction of frames for which decode labels harmonic `k` voiced.
//!
//! **Result (live chip, 57 stable-pitch frames): inconclusive, do not cite as evidence either way.**
//! Per-band agreement between decoded voicing and the chip-/input-derived voicing is ~0.5 (chance) under
//! every hypothesis tried (normal, reversed, complement, +-1/+-2 band shifts). The metric itself is weak:
//! float PCM, whose voicing is known, reads ~15 dB everywhere on it, and 480-sample windows barely
//! resolve 95-140 Hz harmonics. An independent decoder (mbelib) is the better discriminator.
//!
//! Usage: `cargo run --release --example ratet27_probe_harmonicity_oracle -- <host:port>`

use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 200;

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
fn build_channel(payload: &[u8]) -> Vec<u8> {
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CHANNEL);
    pkt.extend_from_slice(payload);
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
fn parse_speech_payload(payload: &[u8]) -> Vec<i16> {
    let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    payload[2..2 + count * 2]
        .chunks_exact(2)
        .map(|b| i16::from_be_bytes([b[0], b[1]]))
        .collect()
}
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 1024], pkt: &[u8]) -> usize {
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
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}
fn wire_bytes_to_c(bytes: &[u8; FRAME_BYTES]) -> [u32; 8] {
    let mut wire_frame_bits = [false; 144];
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for b in 0..8 {
            wire_frame_bits[byte_idx * 8 + b] = (byte >> (7 - b)) & 1 == 1;
        }
    }
    let raw = |block: Block| -> u32 {
        let members = block_wire_members(block);
        let mut received: u32 = 0;
        for (offset, &wire) in members.iter().enumerate() {
            if wire_frame_bits[wire] {
                received |= 1 << (members.len() - 1 - offset);
            }
        }
        received
    };
    [
        raw(Block::Golay { index: 0 }),
        raw(Block::Golay { index: 1 }),
        raw(Block::Golay { index: 2 }),
        raw(Block::Golay { index: 3 }),
        raw(Block::Hamming { index: 0 }),
        raw(Block::Hamming { index: 1 }),
        raw(Block::Hamming { index: 2 }),
        raw(Block::Raw),
    ]
}


const FFT_LEN: usize = 4096;
const SAMPLE_RATE: f64 = 8000.0;

fn hann_fft_magnitude(samples: &[f64]) -> Vec<f64> {
    let n = samples.len();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(FFT_LEN);
    let mut buf: Vec<Complex64> = samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n as f64 - 1.0)).cos();
            Complex64::new(s * w, 0.0)
        })
        .collect();
    buf.resize(FFT_LEN, Complex64::new(0.0, 0.0));
    fft.process(&mut buf);
    buf.iter().map(|c| c.norm()).collect()
}

fn bin(hz: f64) -> usize {
    ((hz * FFT_LEN as f64 / SAMPLE_RATE).round() as usize).min(FFT_LEN / 2 - 1)
}

/// Harmonicity in dB: peak within +-20% of f0 of `k*f0`, versus the minimum within +-15% of f0 of
/// `(k+0.5)*f0`.
fn harmonicity_db(mag: &[f64], f0: f64, k: u32) -> f64 {
    let kf = k as f64;
    let peak = (bin(kf * f0 - 0.2 * f0)..=bin(kf * f0 + 0.2 * f0)).map(|b| mag[b]).fold(0.0, f64::max);
    let trough = (bin((kf + 0.5) * f0 - 0.15 * f0)..=bin((kf + 0.5) * f0 + 0.15 * f0))
        .map(|b| mag[b])
        .fold(f64::INFINITY, f64::min);
    20.0 * ((peak + 1e-9) / (trough + 1e-9)).log10()
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let pcm = read_wav_mono_i16("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    let n_frames = (pcm.len() / FRAME_SAMPLES).min(N_FRAMES);
    let input_pcm: Vec<f64> = pcm[..n_frames * FRAME_SAMPLES].iter().map(|&s| s as f64).collect();

    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        channel_payloads.push(payload.to_vec());
    }

    let mut decoder = DecoderState::new_chip_wire();
    let mut params_per_frame: Vec<Option<(u32, u32, f64, Vec<bool>)>> = Vec::new(); // (b0, l_hat, f0_hz, voiced)
    let mut chip_pcm: Vec<f64> = Vec::new();
    let mut float_pcm: Vec<f64> = Vec::new();
    let mut params_decoder = DecoderState::new_chip_wire();
    for channel_payload in &channel_payloads {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);

        match params_decoder.decode_parameters(c) {
            Some(FrameOutcome::Decoded(p)) => {
                let f0 = p.omega0_tilde * SAMPLE_RATE / (2.0 * std::f64::consts::PI);
                params_per_frame.push(Some((p.bits.b0, p.l_hat, f0, p.voiced.clone())));
                params_decoder.advance_history(&p);
            }
            _ => params_per_frame.push(None),
        }

        let n = send_recv_retrying(&sock, &mut buf, &build_channel(channel_payload));
        let (_, payload) = parse_packet(&buf[..n]).expect("valid packet");
        chip_pcm.extend(parse_speech_payload(payload).iter().map(|&s| s as f64));
        match decoder.decode_frame(c) {
            Some(f) => float_pcm.extend(f.iter().copied()),
            None => float_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES)),
        }
    }

    const KMAX: usize = 24;
    let mut sum_input = [0.0f64; KMAX];
    let mut sum_chip = [0.0f64; KMAX];
    let mut sum_float = [0.0f64; KMAX];
    let mut voiced_count = [0usize; KMAX];
    let mut count = [0usize; KMAX];
    let mut frames_used = 0usize;
    // (decoded per-band voiced [k_hat], chip-derived per-band voiced, input-derived per-band voiced)
    let mut band_records: Vec<(Vec<bool>, Vec<bool>, Vec<bool>)> = Vec::new();

    for i in 1..n_frames.saturating_sub(1) {
        let (Some(a), Some(b), Some(c)) = (&params_per_frame[i - 1], &params_per_frame[i], &params_per_frame[i + 1])
        else {
            continue;
        };
        let (b0_prev, b0_cur, b0_next) = (a.0 as i32, b.0 as i32, c.0 as i32);
        if (b0_prev - b0_cur).abs() > 2 || (b0_next - b0_cur).abs() > 2 || b.2 < 95.0 || b.1 < 20 {
            continue;
        }
        frames_used += 1;
        let span = |pcm: &[f64]| pcm[(i - 1) * FRAME_SAMPLES..(i + 2) * FRAME_SAMPLES].to_vec();
        let mag_in = hann_fft_magnitude(&span(&input_pcm));
        let mag_chip = hann_fft_magnitude(&span(&chip_pcm));
        let mag_float = hann_fft_magnitude(&span(&float_pcm));
        for k in 1..=(KMAX as u32).min(b.1) {
            let idx = (k - 1) as usize;
            sum_input[idx] += harmonicity_db(&mag_in, b.2, k);
            sum_chip[idx] += harmonicity_db(&mag_chip, b.2, k);
            sum_float[idx] += harmonicity_db(&mag_float, b.2, k);
            if b.3[idx] {
                voiced_count[idx] += 1;
            }
            count[idx] += 1;
        }
        // Per-band (3 harmonics each, matching frequency_bands_count's l<=36 grouping) classification.
        use ham_digital_modes::ambe::float::tia_102_baba::vuv::frequency_bands_count;
        let k_hat = frequency_bands_count(b.1) as usize;
        let mut decoded_band = vec![false; k_hat];
        for l in 1..=b.1 {
            decoded_band[(frequency_bands_count(l) - 1) as usize] = b.3[(l - 1) as usize];
        }
        let classify = |mag: &[f64], thresh: f64| -> Vec<bool> {
            (1..=k_hat as u32)
                .map(|band| {
                    let mut vals: Vec<f64> = (1..=b.1)
                        .filter(|&l| frequency_bands_count(l) == band)
                        .map(|l| harmonicity_db(mag, b.2, l))
                        .collect();
                    vals.sort_by(|x, y| x.partial_cmp(y).unwrap());
                    vals[vals.len() / 2] > thresh
                })
                .collect()
        };
        band_records.push((decoded_band, classify(&mag_chip, 22.0), classify(&mag_in, 18.0)));
    }

    println!("{frames_used} stable-pitch frames used (b0 within +-2 across 3 frames, f0>=95Hz, l_hat>=20)");
    println!("k  | frac decoded voiced | harmonicity dB: input / chip / float");
    for idx in 0..KMAX {
        if count[idx] == 0 {
            continue;
        }
        let n = count[idx] as f64;
        println!(
            "{:2} | {:5.2}               | {:6.1} / {:6.1} / {:6.1}",
            idx + 1,
            voiced_count[idx] as f64 / n,
            sum_input[idx] / n,
            sum_chip[idx] / n,
            sum_float[idx] / n
        );
    }

    // Agreement of decoded band voicing (under each hypothesis) with chip-/input-derived voicing.
    println!("\nband-voicing agreement over {} frames (bands 1..=min(k_hat,8)):", band_records.len());
    for (name, oracle_idx) in [("chip-output", 1usize), ("input-speech", 2usize)] {
        println!("oracle = {name}");
        for hyp in ["normal", "reversed", "inverted(complement)", "shift+1", "shift-1", "shift+2", "shift-2"] {
            let (mut agree, mut total) = (0usize, 0usize);
            for rec in &band_records {
                let oracle = if oracle_idx == 1 { &rec.1 } else { &rec.2 };
                let k = rec.0.len();
                for band in 0..k.min(8) {
                    let src: Option<usize> = match hyp {
                        "normal" | "inverted(complement)" => Some(band),
                        "reversed" => Some(k - 1 - band),
                        "shift+1" => band.checked_sub(1),
                        "shift-1" => (band + 1 < k).then_some(band + 1),
                        "shift+2" => band.checked_sub(2),
                        "shift-2" => (band + 2 < k).then_some(band + 2),
                        _ => None,
                    };
                    let Some(src) = src else { continue };
                    let mut v = rec.0[src];
                    if hyp == "inverted(complement)" {
                        v = !v;
                    }
                    total += 1;
                    if v == oracle[band] {
                        agree += 1;
                    }
                }
            }
            println!("  {hyp:22} agreement {:.3} ({agree}/{total})", agree as f64 / total.max(1) as f64);
        }
        let base_rate: f64 = {
            let (mut t, mut n) = (0usize, 0usize);
            for rec in &band_records {
                let o = if oracle_idx == 1 { &rec.1 } else { &rec.2 };
                for band in 0..o.len().min(8) { n += 1; if o[band] { t += 1; } }
            }
            t as f64 / n.max(1) as f64
        };
        println!("  (oracle's own voiced fraction over these bands: {base_rate:.3})");
    }
}

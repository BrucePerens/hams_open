// SPDX-License-Identifier: LGPL-3.0-or-later
//! Tests the "the AMBE chip is bassy" hypothesis: the chip may add low-frequency content *below* the
//! coded bands (a sub-fundamental/low-harmonic boost), which `ratet27_diagnose_harmonic_extension.rs`
//! (it only looked above `L_hat*omega0`) could not see. Reports (1) the long-term average power
//! spectrum ratio chip/float in dB per band across all captured frames, and (2) for every decoded
//! frame, the fraction of energy below `0.8*f0` (below the first harmonic) for chip vs float.
//!
//! **Result (live chip, 200 frames)**: no low-side extension found. Chip tracks the *input* spectrum
//! within ~4 dB in every band and has 4.2% energy below 0.8*f0 vs float's 3.8%. The real discrepancy is
//! float being far too *bright*: vs the input, float is +9 dB at 2-3 kHz and +16 dB at 3-4 kHz while
//! chip is -4/-6 dB there. Suspect unvoiced synthesis level/scaling at high harmonics.
//!
//! Usage: `cargo run --release --example ratet27_diagnose_bass_boost -- <host:port>`

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

/// A real (not naive-DFT) FFT via rustfft, zero-padded to `fft_len` for finer bin resolution than
/// the raw 160-sample frame alone would give (160 samples at 8kHz gives only 50Hz bins; zero-padding
/// to 1024 gives ~7.8Hz bins, enough to cleanly separate harmonic lines from the AMBE pitch range).
fn fft_magnitude(samples: &[f64], fft_len: usize) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(fft_len);
    let mut buf: Vec<Complex64> = samples.iter().map(|&s| Complex64::new(s, 0.0)).collect();
    buf.resize(fft_len, Complex64::new(0.0, 0.0));
    fft.process(&mut buf);
    buf.iter().map(|c| c.norm()).collect()
}

/// Nearest FFT bin index for a frequency in Hz.
fn bin_for_hz(hz: f64, fft_len: usize, sample_rate: f64) -> usize {
    ((hz * fft_len as f64 / sample_rate).round() as usize).min(fft_len / 2)
}


fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send");
    let n = sock.recv(&mut buf).expect("resp");
    parse_packet(&buf[..n]).expect("valid packet");

    let pcm = read_wav_mono_i16("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    let n_frames = (pcm.len() / FRAME_SAMPLES).min(N_FRAMES);
    let mut payloads: Vec<Vec<u8>> = Vec::new();
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        payloads.push(payload.to_vec());
    }

    const FFT_LEN: usize = 512;
    const SR: f64 = 8000.0;
    let half = FFT_LEN / 2;
    let mut dec = DecoderState::new_chip_wire();
    let mut chip_psd = vec![0.0; half];
    let mut float_psd = vec![0.0; half];
    let mut sub_chip = Vec::new();
    let mut sub_float = Vec::new();
    let mut in_psd = vec![0.0; half];
    for (i, p) in payloads.iter().enumerate() {
        let mut wb = [0u8; FRAME_BYTES];
        wb.copy_from_slice(&p[p.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wb);
        let n = send_recv_retrying(&sock, &mut buf, &build_channel(p));
        let (_, payload) = parse_packet(&buf[..n]).expect("valid packet");
        let chip: Vec<f64> = parse_speech_payload(payload).iter().map(|&s| s as f64).collect();
        // Parameters (for f0) come from a separate params pass below; decode_frame gives float PCM.
        let float = dec.decode_frame(c).map(|f| f.to_vec()).unwrap_or_else(|| vec![0.0; FRAME_SAMPLES]);
        let input: Vec<f64> = pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES].iter().map(|&s| s as f64).collect();
        let hann = |v: &[f64]| -> Vec<f64> {
            v.iter().enumerate().map(|(k, &s)| s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / (v.len() as f64 - 1.0)).cos())).collect()
        };
        let cm = fft_magnitude(&hann(&chip), FFT_LEN);
        let fm = fft_magnitude(&hann(&float), FFT_LEN);
        let im = fft_magnitude(&hann(&input), FFT_LEN);
        for b in 0..half {
            chip_psd[b] += cm[b] * cm[b];
            float_psd[b] += fm[b] * fm[b];
            in_psd[b] += im[b] * im[b];
        }
    }
    // Per-frame sub-fundamental energy, using a params-only decoder.
    let mut pd = DecoderState::new_chip_wire();
    let mut chip_all: Vec<Vec<f64>> = Vec::new();
    let _ = &mut chip_all;
    let mut sock_buf = [0u8; 1024];
    for p in payloads.iter() {
        let mut wb = [0u8; FRAME_BYTES];
        wb.copy_from_slice(&p[p.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wb);
        if let Some(FrameOutcome::Decoded(params)) = pd.decode_parameters(c) {
            pd.advance_history(&params);
            let f0 = params.omega0_tilde * SR / (2.0 * std::f64::consts::PI);
            let n = send_recv_retrying(&sock, &mut sock_buf, &build_channel(p));
            let (_, payload) = parse_packet(&sock_buf[..n]).expect("valid packet");
            let chip: Vec<f64> = parse_speech_payload(payload).iter().map(|&s| s as f64).collect();
            let cm = fft_magnitude(&chip, FFT_LEN);
            let total: f64 = cm[..half].iter().map(|m| m * m).sum();
            let cut = bin_for_hz(0.8 * f0, FFT_LEN, SR);
            if total > 1e-9 {
                sub_chip.push(cm[..cut].iter().map(|m| m * m).sum::<f64>() / total);
            }
        }
    }
    // Float sub-fundamental fraction: re-decode with a fresh decoder producing PCM.
    let mut fd = DecoderState::new_chip_wire();
    let mut pd2 = DecoderState::new_chip_wire();
    for p in payloads.iter() {
        let mut wb = [0u8; FRAME_BYTES];
        wb.copy_from_slice(&p[p.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wb);
        let f0 = if let Some(FrameOutcome::Decoded(params)) = pd2.decode_parameters(c) {
            pd2.advance_history(&params);
            Some(params.omega0_tilde * SR / (2.0 * std::f64::consts::PI))
        } else { None };
        let pcm_f = fd.decode_frame(c);
        if let (Some(f0), Some(pcm_f)) = (f0, pcm_f) {
            let fm = fft_magnitude(&pcm_f, FFT_LEN);
            let total: f64 = fm[..half].iter().map(|m| m * m).sum();
            let cut = bin_for_hz(0.8 * f0, FFT_LEN, SR);
            if total > 1e-9 {
                sub_float.push(fm[..cut].iter().map(|m| m * m).sum::<f64>() / total);
            }
        }
    }

    println!("Long-term average power spectrum (dB, relative), per band:");
    println!("{:>12} {:>9} {:>9} {:>9} {:>11} {:>11}", "band Hz", "input", "chip", "float", "chip/float", "chip/input");
    let edges = [0.0, 100.0, 200.0, 300.0, 400.0, 500.0, 700.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0];
    let band = |psd: &[f64], lo: f64, hi: f64| -> f64 {
        let a = bin_for_hz(lo, FFT_LEN, SR);
        let b = bin_for_hz(hi, FFT_LEN, SR).max(a + 1);
        psd[a..b.min(half)].iter().sum::<f64>()
    };
    for w in edges.windows(2) {
        let (i, c, f) = (band(&in_psd, w[0], w[1]), band(&chip_psd, w[0], w[1]), band(&float_psd, w[0], w[1]));
        let db = |x: f64| 10.0 * x.max(1e-12).log10();
        println!("{:>5.0}-{:<5.0} {:>9.1} {:>9.1} {:>9.1} {:>+11.1} {:>+11.1}", w[0], w[1], db(i), db(c), db(f), db(c) - db(f), db(c) - db(i));
    }
    let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    println!("\nEnergy below 0.8*f0 (fraction of frame): chip {:.4}, float {:.4} over {} / {} decoded frames",
        avg(&sub_chip), avg(&sub_float), sub_chip.len(), sub_float.len());
}

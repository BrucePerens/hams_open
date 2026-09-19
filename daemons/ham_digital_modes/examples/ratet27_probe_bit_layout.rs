// SPDX-License-Identifier: LGPL-3.0-or-later
//! Bit-flip probe of the chip's real RATET(27) data-bit layout, using the chip's *decode* direction as an
//! oracle. For a real captured base frame, flips each of the 88 data bits (12 in each of `u0..u3`, 11 in each
//! of `u4..u6`, 7 in `u7`), re-encodes the FEC (`golay_encode`, `hamming_encode_chip`, `u7` raw), sends the
//! modified frame to the chip repeated `REPS` times (after `REPS` repeats of the unmodified base, so the
//! chip's own prediction history has settled), and records the steady-state change in per-band output
//! energy (dB, eight 500 Hz bands). The same procedure is run through this crate's own float decoder; a bit
//! whose chip response differs sharply from this crate's predicted response is one the TIA-102 Fig. 22
//! layout (`deprioritize_bits`) places wrongly for the real chip.
//!
//! **Result (live chip, base frames 45/84/97; per-band noise floor between identical repeats ~1.5 dB)**:
//! partially consistent with the TIA layout, not conclusive. In frame 84 (`L=46`) most higher-order
//! coefficient bits respond in the frequency range their block covers (block 4 -> 1.7-2.4 kHz, block 6 ->
//! 3.0-3.7 kHz, ...), but in frames 45 and 97 several low-order bits (e.g. `hoc[blk3,k2].0`) produce
//! +40-50 dB jumps at 3-4 kHz that no block-local coefficient should cause, so nonlinear/overflow effects
//! (or a differently-placed bit) confound the readings. `u3` (`g3`, rank-8) bits are unlabeled because
//! their natural-bit mapping is unresolved. Do not cite as a layout proof either way.
//!
//! Output, per (base frame, bit): chip delta vector, our delta vector, their correlation and the ratio of
//! their L2 norms. Bits are named `u<vector>.<bit>` with bit 0 the LSB of that vector's data field.
//!
//! Usage: `cargo run --release --example ratet27_probe_bit_layout -- <host:port> [base_frame_indexes...]`

use ham_digital_modes::ambe::float::ratet27::decode::DecoderState;
use ham_digital_modes::ambe::float::ratet27::ratet27_fec::{g3_decode, g3_encode, hamming_decode_chip, hamming_encode_chip};
use ham_digital_modes::ambe::float::ratet27::ratet27_wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::float::ratet27::bit_prioritization::{deprioritize_bits, extract_fundamental_frequency_quantizer};
use ham_digital_modes::ambe::float::ratet27::parameter_encoding::dequantize_fundamental_frequency;
use ham_digital_modes::ambe::float::ratet27::quantize::higher_order_coefficient_positions;
use ham_digital_modes::ambe::float::ratet27::tables::{gain_bit_allocation, higher_order_bit_allocation};
use ham_digital_modes::ambe::float::ratet27::vuv::{frequency_bands_count, harmonics_count};
use ham_digital_modes::ambe::general::fec::{golay_decode, golay_encode};
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


const REPS: usize = 8;
const BANDS: usize = 8;

fn c_to_wire_bytes(c: &[u32; 8]) -> [u8; FRAME_BYTES] {
    let blocks = [
        Block::Golay { index: 0 },
        Block::Golay { index: 1 },
        Block::Golay { index: 2 },
        Block::Golay { index: 3 },
        Block::Hamming { index: 0 },
        Block::Hamming { index: 1 },
        Block::Hamming { index: 2 },
        Block::Raw,
    ];
    let mut bits = [false; 144];
    for (i, &block) in blocks.iter().enumerate() {
        let members = block_wire_members(block);
        for (offset, &wire) in members.iter().enumerate() {
            bits[wire] = (c[i] >> (members.len() - 1 - offset)) & 1 == 1;
        }
    }
    let mut bytes = [0u8; FRAME_BYTES];
    for (i, &b) in bits.iter().enumerate() {
        if b {
            bytes[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    bytes
}

fn band_energies_db(frames: &[Vec<f64>]) -> [f64; BANDS] {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(FRAME_SAMPLES);
    let mut acc = [0.0f64; BANDS];
    for f in frames {
        let mut buf: Vec<Complex64> = f
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (FRAME_SAMPLES as f64 - 1.0)).cos();
                Complex64::new(s * w, 0.0)
            })
            .collect();
        fft.process(&mut buf);
        for (k, c) in buf.iter().enumerate().take(FRAME_SAMPLES / 2) {
            let hz = k as f64 * 8000.0 / FRAME_SAMPLES as f64;
            let band = ((hz / 500.0) as usize).min(BANDS - 1);
            acc[band] += c.norm_sqr();
        }
    }
    acc.map(|e| 10.0 * (e / frames.len() as f64 + 1.0).log10())
}

fn u_from_c(c: &[u32; 8]) -> [u32; 8] {
    [
        golay_decode(c[0]).0 as u32,
        golay_decode(c[1]).0 as u32,
        golay_decode(c[2]).0 as u32,
        golay_decode(c[3]).0 as u32,
        hamming_decode_chip(c[4] as u16).0 as u32,
        hamming_decode_chip(c[5] as u16).0 as u32,
        hamming_decode_chip(c[6] as u16).0 as u32,
        c[7],
    ]
}
fn c_from_u(u: &[u32; 8]) -> [u32; 8] {
    [
        golay_encode(u[0] as u16),
        golay_encode(u[1] as u16),
        golay_encode(u[2] as u16),
        golay_encode(u[3] as u16),
        hamming_encode_chip(u[4] as u16) as u32,
        hamming_encode_chip(u[5] as u16) as u32,
        hamming_encode_chip(u[6] as u16) as u32,
        u[7],
    ]
}

/// What this crate's TIA-102 layout says bit `bit` of `u[v]` means for the frame `u_base`: flips it and diffs
/// `deprioritize_bits`' output, returning e.g. `b2.3`, `gain[2].1`, `hoc[blk3,k5].0`, `b1.band4`.
fn tia_label(u_base: &[u32; 8], v: usize, bit: usize, width: usize) -> String {
    let b0 = extract_fundamental_frequency_quantizer(u_base);
    let l_hat = harmonics_count(dequantize_fundamental_frequency(b0));
    let k_hat = frequency_bands_count(l_hat);
    let gain_widths: [u8; 5] = std::array::from_fn(|i| gain_bit_allocation(l_hat, i as u32 + 2).unwrap().0);
    let higher_widths: Vec<u8> = higher_order_bit_allocation(l_hat).unwrap().iter().copied().filter(|&w| w > 0).collect();
    let positions = higher_order_coefficient_positions(l_hat).unwrap();
    let alloc = higher_order_bit_allocation(l_hat).unwrap();
    let nonzero_positions: Vec<(usize, usize)> =
        positions.iter().zip(alloc.iter()).filter(|(_, &w)| w > 0).map(|(&p, _)| p).collect();
    let _ = width;
    let a = deprioritize_bits(*u_base, k_hat, gain_widths, &higher_widths);
    let mut um = *u_base;
    um[v] ^= 1 << bit;
    let b = deprioritize_bits(um, k_hat, gain_widths, &higher_widths);
    let (Some(a), Some(b)) = (a, b) else { return "?".into() };
    if a.b0 != b.b0 {
        return format!("b0.{}", (a.b0 ^ b.b0).trailing_zeros());
    }
    if a.b2 != b.b2 {
        return format!("b2.{}", (a.b2 ^ b.b2).trailing_zeros());
    }
    if a.b1 != b.b1 {
        let band = k_hat - 1 - (a.b1 ^ b.b1).trailing_zeros();
        return format!("b1.band{}", band + 1);
    }
    for i in 0..5 {
        if a.gain_vector[i].0 != b.gain_vector[i].0 {
            return format!("gain[{}].{}", i + 2, (a.gain_vector[i].0 ^ b.gain_vector[i].0).trailing_zeros());
        }
    }
    for (j, (x, y)) in a.higher_order.iter().zip(b.higher_order.iter()).enumerate() {
        if x.0 != y.0 {
            let (blk, k) = nonzero_positions[j];
            return format!("hoc[blk{},k{}].{}", blk + 1, k, (x.0 ^ y.0).trailing_zeros());
        }
    }
    if a.sync_bit != b.sync_bit {
        return "sync".into();
    }
    "?".into()
}

fn corr(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        c += (x - ma) * (y - mb);
        va += (x - ma).powi(2);
        vb += (y - mb).powi(2);
    }
    if va < 1e-9 || vb < 1e-9 {
        0.0
    } else {
        c / (va.sqrt() * vb.sqrt())
    }
}
fn norm(a: &[f64]) -> f64 {
    a.iter().map(|x| x * x).sum::<f64>().sqrt()
}

struct Chip<'a> {
    sock: &'a UdpSocket,
    buf: [u8; 1024],
    header: Vec<u8>,
}
impl Chip<'_> {
    /// Sends `c` to the chip's decoder `REPS` times, returning the last two decoded frames.
    fn decode_repeated(&mut self, c: &[u32; 8]) -> Vec<Vec<f64>> {
        let wire = c_to_wire_bytes(c);
        // A CHANNEL payload: the same 18 data bytes the encoder responds with, preceded by whatever
        // header this chip's RATEP configuration uses (probed from a real response by the caller).
        let mut payload = self.header.clone();
        payload.extend_from_slice(&wire);
        let mut frames = Vec::new();
        for _ in 0..REPS {
            let n = send_recv_retrying(self.sock, &mut self.buf, &build_channel(&payload));
            let (ptype, p) = parse_packet(&self.buf[..n]).expect("valid packet");
            if ptype != TYPE_SPEECH {
                eprintln!("non-speech reply type {ptype}: {:02x?}", &self.buf[..n.min(24)]);
                frames.push(vec![0.0; FRAME_SAMPLES]);
                continue;
            }
            frames.push(parse_speech_payload(p).iter().map(|&s| s as f64).collect());
        }
        frames.split_off(REPS - 2)
    }
}

fn ours_repeated(c_base: &[u32; 8], c_mod: &[u32; 8]) -> ([f64; BANDS], [f64; BANDS]) {
    let mut d = DecoderState::new();
    let mut run = |c: &[u32; 8]| -> Vec<Vec<f64>> {
        let mut frames = Vec::new();
        for _ in 0..REPS {
            frames.push(d.decode_frame(*c).map(|f| f.to_vec()).unwrap_or_else(|| vec![0.0; FRAME_SAMPLES]));
        }
        frames.split_off(REPS - 2)
    };
    let base = band_energies_db(&run(c_base));
    let modified = band_energies_db(&run(c_mod));
    (base, modified)
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let bases: Vec<usize> = std::env::args().skip(2).filter_map(|a| a.parse().ok()).collect();
    let bases = if bases.is_empty() { vec![45, 84] } else { bases };
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();

    let pcm = read_wav_mono_i16("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    let mut chip = Chip { sock: &sock, buf: [0u8; 1024], header: Vec::new() };
    let widths: [usize; 8] = [12, 12, 12, 8, 11, 11, 11, 7]; // u3 is g3's 8-bit subspace

    for &base_idx in &bases {
        // Capture base frame by encoding real speech; preceding frames prime the encoder.
        let mut c_base = [0u32; 8];
        for i in base_idx.saturating_sub(3)..=base_idx {
            let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
            let (_, payload) = parse_packet(&buf[..n]).unwrap();
            let mut wb = [0u8; FRAME_BYTES];
            wb.copy_from_slice(&payload[payload.len() - FRAME_BYTES..]);
            chip.header = payload[..payload.len() - FRAME_BYTES].to_vec();
            assert_eq!(c_to_wire_bytes(&wire_bytes_to_c(&wb)), wb, "c<->wire round trip");
            eprintln!("payload len {} header {:02x?}", payload.len(), chip.header);
            c_base = wire_bytes_to_c(&wb);
        }
        let u_base = u_from_c(&c_base);
        let c_base_clean = c_base; // raw: g3 is not a standard Golay word, so re-encoding u3 would corrupt it
        println!("== base frame {base_idx}: u = {u_base:04x?}");
        let base_chip = band_energies_db(&chip.decode_repeated(&c_base_clean));
        println!("chip base band dB: {:.1?}", base_chip);
        println!("ours base band dB: {:.1?}", ours_repeated(&c_base_clean, &c_base_clean).0);
        let noise: Vec<f64> = {
            let a = band_energies_db(&chip.decode_repeated(&c_base_clean));
            let b = band_energies_db(&chip.decode_repeated(&c_base_clean));
            (0..BANDS).map(|i| b[i] - a[i]).collect()
        };
        println!("chip base-vs-base noise floor dB: {:.1?} (|.| mean {:.1})", noise, noise.iter().map(|x| x.abs()).sum::<f64>() / BANDS as f64);
        println!("vec.bit | chip delta dB per 500Hz band | corr(chip,ours) | |chip|/|ours|");
        for v in 0..8 {
            for bit in 0..widths[v] {
                let mut c_mod = c_base;
                if v == 3 {
                    c_mod[3] = g3_encode(g3_decode(c_base[3]).0 ^ (1 << bit));
                } else {
                    let mut u = u_base;
                    u[v] ^= 1 << bit;
                    c_mod[v] = c_from_u(&u)[v];
                }
                let base_now = band_energies_db(&chip.decode_repeated(&c_base_clean)); // fresh baseline right before
                let chip_mod = band_energies_db(&chip.decode_repeated(&c_mod));
                let chip_delta: Vec<f64> = (0..BANDS).map(|b| chip_mod[b] - base_now[b]).collect();
                let (ob, om) = ours_repeated(&c_base_clean, &c_mod);
                let ours_delta: Vec<f64> = (0..BANDS).map(|b| om[b] - ob[b]).collect();
                let label = if v == 3 { "g3(unmapped)".to_string() } else { tia_label(&u_base, v, bit, widths[v]) };
                println!(
                    "u{v}.{bit:<2} {label:16} | {:>5.1?} | {:5.2} | {:5.2}",
                    chip_delta,
                    corr(&chip_delta, &ours_delta),
                    norm(&chip_delta) / norm(&ours_delta).max(1e-9)
                );
            }
        }
    }
}

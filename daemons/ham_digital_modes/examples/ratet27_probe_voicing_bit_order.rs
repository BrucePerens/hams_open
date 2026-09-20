// SPDX-License-Identifier: LGPL-3.0-or-later
//! A follow-up to `ratet27_diagnose_harmonic_extension.rs`'s own striking finding: in every one of
//! 10 real, predominantly-voiced captured frames, harmonics 1-6 (bands 1-2) decoded as UNVOICED while
//! higher harmonics decoded voiced -- backwards from ordinary voiced-speech acoustics (voicing
//! normally concentrates at low frequencies, falling off at high ones, not the reverse). This tests
//! the most direct explanation: `decode_voicing_decisions`'s own `b1_tilde` bit order (Eq. 49,
//! transcribed as band 1 = MSB of a `k_hat`-bit field) was never itself validated against the real
//! chip -- only the *position* of `b1`'s bits within the prioritized bitstream was (via
//! `deprioritize_bits`'s own chip-confirmed layout). If DVSI's real encoder packs band `k_hat` as the
//! MSB instead (reversed from the spec's own literal convention), every band's voicing decision would
//! be systematically swapped end-to-end -- exactly the observed pattern.
//!
//! Re-synthesizes each captured chip frame twice: once with this crate's own (spec-literal) voicing,
//! once with the per-band bit order reversed (`k -> k_hat + 1 - k`), keeping every other decoded
//! parameter (omega0_tilde, reconstructed amplitudes, errors) identical. Reports both the resulting
//! voicing pattern (does reversing make it look like ordinary speech?) and each version's own
//! envelope correlation against the real chip PCM, to check directly whether the reversal is real
//! rather than merely plausible-looking.
//!
//! **Result (live chip, 200 frames)**: reversed voicing scores envelope correlation `0.6215` vs `0.6293`
//! for the spec-literal order -- no improvement, so no evidence for a reversal. Envelope correlation is
//! insensitive to voicing, though, so this is "unsupported", not "ruled out"; the always-unvoiced
//! harmonics 1-6 pattern remains unexplained.
//!
//! Usage: `cargo run --release --example ratet27_probe_voicing_bit_order -- <host:port>`

use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::float::tia_102_baba::synthesis::SynthesisState;
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

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let dx = x - mean_a;
        let dy = y - mean_b;
        cov += dx * dy;
        var_a += dx * dx;
        var_b += dy * dy;
    }
    if var_a <= 1e-9 || var_b <= 1e-9 {
        return 0.0;
    }
    cov / (var_a.sqrt() * var_b.sqrt())
}
fn frame_rms(pcm: &[f64]) -> Vec<f64> {
    pcm.chunks(FRAME_SAMPLES)
        .filter(|c| c.len() == FRAME_SAMPLES)
        .map(|c| (c.iter().map(|&s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt())
        .collect()
}

/// Reverses the per-band voicing bit order: band `k` (1-indexed) takes the decision this crate's own
/// spec-literal decode assigned to band `k_hat + 1 - k` instead.
fn reverse_band_order(voiced_per_harmonic: &[bool], l_hat: u32, k_hat: u32) -> Vec<bool> {
    use ham_digital_modes::ambe::float::tia_102_baba::vuv::frequency_bands_count;
    // Reconstruct the per-band array this frame's voiced_per_harmonic was expanded from, by taking
    // one representative harmonic per band (the highest harmonic in each band, arbitrary but
    // consistent), then re-expand with band index reversed.
    let mut per_band = vec![false; k_hat as usize];
    for l in 1..=l_hat {
        let kappa_l = frequency_bands_count(l);
        per_band[(kappa_l - 1) as usize] = voiced_per_harmonic[(l - 1) as usize];
    }
    let reversed_band = |k: u32| per_band[(k_hat - k) as usize];
    (1..=l_hat)
        .map(|l| {
            let kappa_l = frequency_bands_count(l);
            reversed_band(kappa_l)
        })
        .collect()
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

    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        channel_payloads.push(payload.to_vec());
    }

    // Print a handful of real voicing patterns, normal vs band-reversed, for a direct look.
    let mut params_decoder = DecoderState::new_chip_wire();
    let mut shown = 0;
    for channel_payload in &channel_payloads {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);
        if let Some(FrameOutcome::Decoded(params)) = params_decoder.decode_parameters(c) {
            let voiced_count = params.voiced.iter().filter(|&&v| v).count();
            let frac = voiced_count as f64 / params.voiced.len().max(1) as f64;
            if (0.3..=0.9).contains(&frac) && params.l_hat >= 8 && shown < 8 {
                let normal: String = params.voiced.iter().map(|&v| if v { '1' } else { '0' }).collect();
                let reversed = reverse_band_order(&params.voiced, params.l_hat, params.k_hat);
                let reversed_str: String = reversed.iter().map(|&v| if v { '1' } else { '0' }).collect();
                println!("l_hat={}, k_hat={}", params.l_hat, params.k_hat);
                println!("  normal:   [{normal}]");
                println!("  reversed: [{reversed_str}]");
                shown += 1;
            }
            params_decoder.advance_history(&params);
        }
    }

    // Full re-synthesis pass: normal vs band-reversed voicing, both compared to the real chip PCM.
    let mut normal_decoder = DecoderState::new_chip_wire();
    let mut reversed_decoder_params = DecoderState::new_chip_wire();
    let mut reversed_synth = SynthesisState::new();
    let mut chip_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut normal_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut reversed_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);

    for channel_payload in &channel_payloads {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);

        let n = send_recv_retrying(&sock, &mut buf, &build_channel(channel_payload));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decoded PCM) response");
        let chip_frame_pcm = parse_speech_payload(payload);
        chip_pcm.extend(chip_frame_pcm.iter().map(|&s| s as f64));

        match normal_decoder.decode_frame(c) {
            Some(frame_pcm) => normal_pcm.extend(frame_pcm.iter().copied()),
            None => normal_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES)),
        }

        match reversed_decoder_params.decode_parameters(c) {
            Some(FrameOutcome::Decoded(params)) => {
                let reversed_voiced = reverse_band_order(&params.voiced, params.l_hat, params.k_hat);
                match reversed_synth.synthesize_frame(
                    &params.reconstructed_amplitudes,
                    params.omega0_tilde,
                    &reversed_voiced,
                    &params.errors,
                ) {
                    Some(frame_pcm) => reversed_pcm.extend(frame_pcm.iter().copied()),
                    None => reversed_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES)),
                }
                reversed_decoder_params.advance_history(&params);
            }
            Some(FrameOutcome::Repeat) => {
                match reversed_synth.synthesize_repeated_frame() {
                    Some(frame_pcm) => reversed_pcm.extend(frame_pcm.iter().copied()),
                    None => reversed_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES)),
                }
            }
            Some(FrameOutcome::Mute) => {
                reversed_pcm.extend(reversed_synth.synthesize_comfort_frame().iter().copied());
            }
            None => reversed_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES)),
        }
    }

    let chip_envelope = frame_rms(&chip_pcm);
    let normal_envelope = frame_rms(&normal_pcm);
    let reversed_envelope = frame_rms(&reversed_pcm);
    let len_normal = chip_envelope.len().min(normal_envelope.len());
    let len_reversed = chip_envelope.len().min(reversed_envelope.len());
    println!(
        "\nchip-vs-normal-voicing envelope correlation:   {:.4}",
        correlation(&chip_envelope[..len_normal], &normal_envelope[..len_normal])
    );
    println!(
        "chip-vs-reversed-voicing envelope correlation: {:.4}",
        correlation(&chip_envelope[..len_reversed], &reversed_envelope[..len_reversed])
    );
}

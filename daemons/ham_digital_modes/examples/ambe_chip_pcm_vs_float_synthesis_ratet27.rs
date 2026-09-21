// SPDX-License-Identifier: LGPL-3.0-or-later
//! The first real chip-PCM-vs-synthesized-PCM comparison for RATET(27) -- a gap `night_shift_todo/
//! medium/ambe-fixed-point-port-f0c555a5.md` explicitly flagged as open: no example or test in this
//! codebase had ever compared this crate's own synthesized PCM against the *chip's own* decoded PCM,
//! only against the *input* recording (`tests/ambe_real_speech_round_trip.rs`'s own sanity check).
//!
//! **A previously-unknown chip capability, confirmed by `examples/p25_ratet27_probe_decode_
//! direction.rs`**: every existing RATET(27) example only ever drives the chip's *encode* direction
//! (send `TYPE_SPEECH` PCM, read back `TYPE_CHANNEL` bits). Sending a real, chip-produced
//! `TYPE_CHANNEL` payload back to the chip as an *outgoing* packet works: the chip decodes it and
//! replies with a `TYPE_SPEECH` packet in the exact same wire shape (`[sample_count_be_u16,
//! ...i16 samples...]`) a real speech send uses. This harness interleaves both directions per
//! frame, in the same temporal order a real full-duplex round trip would use, so both the chip's own
//! decoder state and this crate's own `DecoderState` advance in lockstep against the identical
//! stream: for each real speech frame, (1) send it to the chip and capture the encoded channel bits,
//! (2) immediately send those same bits back to get the chip's own reference PCM, (3) feed the same
//! bits through `ambe::float::tia_102_baba::decode::DecoderState::decode_frame` for this crate's own PCM.
//!
//! **Why the metric isn't sample-exact SNR**: MBE synthesis has real, deliberate randomness --
//! `voiced_synthesis`'s own phase dither and `unvoiced_synthesis`'s own noise generator. Two
//! independently-correct decoders (the chip's DSP firmware, this crate's own Rust) draw different
//! noise/dither sequences from different internal state, so their PCM will *never* sample-match on
//! unvoiced content even if both are working correctly -- only the voiced, harmonic-synthesis
//! portion is expected to track closely, since it's driven by the same deterministic reconstructed
//! amplitudes and fundamental frequency. This harness reports a segmental correlation coefficient
//! per frame (after a one-time cross-correlation lag search, since the chip almost certainly carries
//! some algorithmic decode latency) rather than a single global SNR number, and separates
//! predominantly-voiced frames from the rest so a low unvoiced-frame number doesn't get misread as a
//! decoder bug.
//!
//! **Update: root-caused and fixed.** The first real run showed a moderate envelope correlation
//! (`~0.42-0.45`) but near-zero raw-sample correlation (`~0.03`), with a wide lag search finding no
//! better alignment than `lag=0` and an inconsistent (not constant-factor) RMS ratio between the two
//! streams. `examples/ratet27_diagnose_synthesis_mismatch.rs` traced this to a real bug in
//! `DecoderState::decode_parameters`: it was demodulating `c[1..6]` via a spec-theoretical
//! `modulate_code_vectors` step this codebase's own `AMBE_CHIP_VALIDATION_FINDINGS.md` section 23
//! had *already* established the real DVSI chip never applies (a GF(2) rank analysis of ~2200 real
//! captured frames showing they're plain, unwhitened FEC codewords on the wire) -- that finding had
//! never been reconciled with `decode_parameters`'s own implementation until this round. Removing
//! the demodulation (and the matching modulation step from `encode_code_vectors`, to keep this
//! crate's own encode/decode round-trip self-consistent) moved the best alignment from a nonsensical
//! `lag=-318` samples (`-39.75ms`) to `lag=-4` samples (`-0.5ms`), and frame-level envelope
//! correlation from `~0.45` to `~0.63`. A smaller, real amplitude-scale gap remains (float PCM still
//! runs several times louder than the chip's own) -- see `night_shift_todo/medium/
//! ambe-fixed-point-port-f0c555a5.md` in hams_com for the current state of that follow-up.
//!
//! Usage: `cargo run --release --example ambe_chip_pcm_vs_float_synthesis_ratet27 -- <host:port>`

use ham_digital_modes::ambe::float::tia_102_baba::decode::DecoderState;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 200; // 4 seconds at 20ms/frame -- enough for a real lag search plus segments.
const MAX_LAG_SAMPLES: i32 = 2400; // +-15 frames -- widened after +-480 found near-zero correlation.

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
/// Sends a previously-captured `TYPE_CHANNEL` payload straight back as an outgoing packet -- the
/// decode-direction capability `p25_ratet27_probe_decode_direction.rs` confirmed works.
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
/// Writes a minimal mono 16-bit 8kHz PCM WAV -- so both decoded streams can be listened to directly,
/// a far richer diagnostic than any single correlation number for telling "garbage" apart from
/// "recognizable speech that's merely out of sync" apart from "wrong scale but otherwise correct".
fn write_wav_mono_i16(path: &str, samples: &[f64]) {
    let pcm: Vec<i16> = samples.iter().map(|&s| s.round().clamp(-32768.0, 32767.0) as i16).collect();
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes()); // byte rate = sample_rate * block_align
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out).unwrap_or_else(|e| panic!("write {path}: {e}"));
}
/// The same raw pre-FEC `c_hat_0..c_hat_7` extraction `ambe_fixed_chip_validate_ratet27.rs` uses,
/// confirmed correct there (`Decoded: 3291/3320` real frames).
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

/// Pearson correlation coefficient over two equal-length real slices, `0` if either has zero
/// variance (a genuinely silent segment correlates with nothing meaningfully).
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

    let mut float_decoder = DecoderState::new_chip_wire();
    let mut chip_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut float_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut float_decode_failures = 0usize;

    // **Two separate passes, not interleaved per frame.** An earlier version of this harness sent
    // an encode request immediately followed by a decode request, frame by frame -- but the chip's
    // own decoder almost certainly carries frame-to-frame continuity state (phase, noise generator)
    // the same way this crate's own `DecoderState`/`SynthesisState` do, and there is no reason to
    // assume that state survives an *encode* request landing on the same UDP session in between.
    // Interleaving could silently reset the chip's own decoder continuity every single frame,
    // which would explain a moderate envelope correlation (per-frame gross amplitude reconstruction
    // doesn't depend on continuity) alongside poor raw-sample/phase correlation (continuity-
    // dependent phase tracking would never match). Capturing every channel payload first (an
    // encode-only pass, identical in shape to every other already-validated encode-only example in
    // this codebase), then decoding all of them in one uninterrupted decode-only pass, isolates
    // whether interleaving itself was the problem.
    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        channel_payloads.push(payload.to_vec());
    }

    for channel_payload in &channel_payloads {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);

        let n = send_recv_retrying(&sock, &mut buf, &build_channel(channel_payload));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decoded PCM) response");
        let chip_frame_pcm = parse_speech_payload(payload);
        chip_pcm.extend(chip_frame_pcm.iter().map(|&s| s as f64));

        match float_decoder.decode_frame(c) {
            Some(frame_pcm) => float_pcm.extend(frame_pcm.iter().copied()),
            None => {
                float_decode_failures += 1;
                float_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES));
            }
        }
    }

    println!(
        "Captured {n_frames} real frames' worth of PCM from both the chip's own decoder and this \
         crate's float synthesis ({float_decode_failures} float decode failures, e.g. an \
         unrecoverable first-frame repeat)."
    );

    // Lag search: cross-correlate over a real window to find the chip's own algorithmic decode
    // delay before comparing sample-for-sample -- the two streams are not expected to be
    // sample-aligned even if the decode is otherwise correct.
    let mut best_lag = 0i32;
    let mut best_corr = f64::NEG_INFINITY;
    let search_len = chip_pcm.len().min(float_pcm.len()) - (2 * MAX_LAG_SAMPLES as usize);
    for lag in -MAX_LAG_SAMPLES..=MAX_LAG_SAMPLES {
        let (chip_slice, float_slice) = if lag >= 0 {
            (&chip_pcm[lag as usize..lag as usize + search_len], &float_pcm[..search_len])
        } else {
            (&chip_pcm[..search_len], &float_pcm[(-lag) as usize..(-lag) as usize + search_len])
        };
        let c = correlation(chip_slice, float_slice);
        if c > best_corr {
            best_corr = c;
            best_lag = lag;
        }
    }
    println!(
        "Best alignment: lag={best_lag} samples ({:.2} ms), whole-buffer correlation at that lag = {best_corr:.4}",
        best_lag as f64 * 1000.0 / 8000.0
    );

    // Per-frame segmental correlation at the best lag, split by whether the segment looks
    // predominantly voiced (a real, simple proxy: high sample-to-sample similarity / low zero
    // crossing rate would be a better classifier, but RMS-based voicing is a real proxy that needs
    // no new dependency and is good enough to separate "the deterministic part should track
    // closely" from "the random part should not").
    let mut voiced_corrs = Vec::new();
    let mut other_corrs = Vec::new();
    let offset = best_lag.unsigned_abs() as usize;
    let usable_frames = ((chip_pcm.len().min(float_pcm.len()) - offset) / FRAME_SAMPLES).saturating_sub(1);
    for i in 0..usable_frames {
        let (chip_start, float_start) = if best_lag >= 0 {
            (offset + i * FRAME_SAMPLES, i * FRAME_SAMPLES)
        } else {
            (i * FRAME_SAMPLES, offset + i * FRAME_SAMPLES)
        };
        let chip_seg = &chip_pcm[chip_start..chip_start + FRAME_SAMPLES];
        let float_seg = &float_pcm[float_start..float_start + FRAME_SAMPLES];
        let rms: f64 = (chip_seg.iter().map(|&s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt();
        let corr = correlation(chip_seg, float_seg);
        if rms > 200.0 {
            voiced_corrs.push(corr);
        } else {
            other_corrs.push(corr);
        }
    }
    let avg = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    println!(
        "Higher-energy (RMS > 200, predominantly voiced/loud) segments: {} frames, mean correlation {:.4}",
        voiced_corrs.len(),
        avg(&voiced_corrs)
    );
    println!(
        "Lower-energy (unvoiced/silence, where synthesis noise/dither legitimately differs) segments: \
         {} frames, mean correlation {:.4}",
        other_corrs.len(),
        avg(&other_corrs)
    );

    // A much coarser, phase-insensitive sanity check: does the per-frame RMS *envelope* (one value
    // per 20ms frame, ignoring fine sample alignment entirely) track between the two streams? If
    // this is also near zero, the problem is upstream of fine alignment (a real synthesis/amplitude
    // bug, or a wrong PCM scale/sign), not just an imperfect lag search.
    let frame_rms = |pcm: &[f64]| -> Vec<f64> {
        pcm.chunks(FRAME_SAMPLES)
            .filter(|c| c.len() == FRAME_SAMPLES)
            .map(|c| (c.iter().map(|&s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt())
            .collect()
    };
    let chip_envelope = frame_rms(&chip_pcm);
    let float_envelope = frame_rms(&float_pcm);
    let envelope_len = chip_envelope.len().min(float_envelope.len());
    println!(
        "Frame-level RMS envelope correlation (phase-insensitive, no lag applied): {:.4}",
        correlation(&chip_envelope[..envelope_len], &float_envelope[..envelope_len])
    );
    println!("First 15 frame RMS values -- chip: {:?}", &chip_envelope[..15.min(chip_envelope.len())]);
    println!("First 15 frame RMS values -- float: {:?}", &float_envelope[..15.min(float_envelope.len())]);

    // A frame-level (not sample-level) lag search over the envelopes themselves -- much cheaper and
    // more robust than a raw-sample search, and directly tests whether the near-zero raw-sample
    // correlation above is an alignment problem (a real algorithmic decode delay bigger than the
    // +-480-sample/+-3-frame window already tried) versus a genuine amplitude/shape mismatch.
    const MAX_FRAME_LAG: i32 = 40; // +-800ms, generous for any plausible codec delay.
    let mut best_frame_lag = 0i32;
    let mut best_frame_corr = f64::NEG_INFINITY;
    let env_search_len = envelope_len.saturating_sub(2 * MAX_FRAME_LAG as usize);
    if env_search_len > 10 {
        for lag in -MAX_FRAME_LAG..=MAX_FRAME_LAG {
            let (chip_slice, float_slice) = if lag >= 0 {
                (&chip_envelope[lag as usize..lag as usize + env_search_len], &float_envelope[..env_search_len])
            } else {
                (&chip_envelope[..env_search_len], &float_envelope[(-lag) as usize..(-lag) as usize + env_search_len])
            };
            let c = correlation(chip_slice, float_slice);
            if c > best_frame_corr {
                best_frame_corr = c;
                best_frame_lag = lag;
            }
        }
    }
    println!(
        "Best envelope alignment: lag={best_frame_lag} frames ({:.1} ms), envelope correlation at that lag = {best_frame_corr:.4}",
        best_frame_lag as f64 * 20.0
    );

    let out_dir = std::env::var("AMBE_RATET27_WAV_OUT_DIR").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    write_wav_mono_i16(&format!("{out_dir}/ratet27_chip_decoded.wav"), &chip_pcm);
    write_wav_mono_i16(&format!("{out_dir}/ratet27_float_decoded.wav"), &float_pcm);
    println!(
        "Wrote {out_dir}/ratet27_chip_decoded.wav and {out_dir}/ratet27_float_decoded.wav for direct \
         listening comparison (set AMBE_RATET27_WAV_OUT_DIR to change the output directory)."
    );
}

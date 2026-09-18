// SPDX-License-Identifier: LGPL-3.0-or-later
//! Replays DVSI's own real reference test speech (`in.dat`, bundled with the USB-3000 software
//! package's Linux client, `usb3k-linux.tar.gz`) through the real chip at RATET(33) (AMBE+2
//! half-rate with FEC -- the same rate `examples/ambe_chip_validate_ambe_plus_2.rs` already
//! validated live with synthetic tones), decoding every real encoded frame through this crate's own
//! from-spec `ambe_plus_2::decode` -- the first real-speech validation in this whole investigation,
//! not just synthetic tones.
//!
//! **A real bug this tool itself found and fixed while being written**: an earlier version
//! configured the chip with a RATEP custom word copied from the wrong section of DVSI's manual
//! (Figure 20's *full-rate* P25 example -- the same word already used for the still-unresolved
//! RATET-27 mystery) instead of the simple `RATET(33)` index this crate's own already-validated
//! harness uses. That mistake made the chip silently respond with 144-bit full-rate frames instead
//! of AMBE+2's own 72-bit ones, which looked like a real decode failure (0% Golay-clean) until the
//! actual returned bit count was checked directly -- fixed by using the same `RATET(33)` index
//! configuration already proven correct, not a custom RATEP word.
//!
//! **Uses a simple, fully synchronous encode-then-decode-immediately protocol per frame, not DVSI's
//! own pipelined (3-frame-lookahead) reference protocol.** An attempt to replicate that exact
//! pipeline (traced directly from DVSI's own bundled `usb3klinux.c` reference client) found that it
//! does not survive cleanly through AMBEServer3003's own UDP relay layer (responses arrived out of
//! the expected order) -- a real, disclosed limitation, not silently worked around. The synchronous
//! approach sacrifices an exact frame-for-frame PCM comparison against DVSI's own official `cmp.dat`
//! reference output (which reflects that pipeline's own real processing delay), but still gives the
//! two things that actually matter for validating this codec: the real Golay-decode success rate on
//! real speech, and a real, semantically plausible pitch trajectory across ~25 seconds of genuine
//! human voice -- not just synthetic test tones.
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example ambe_plus_2_dvsi_reference_replay --
//! 192.168.10.189:2460 <path-to-in.dat>`

use ham_digital_modes::ambe_plus_2::{decode as ap2_decode, interleave as ap2_interleave, parse_frame};
use std::fs;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const FRAME_SAMPLES: usize = 160;
const RATET_HALF_RATE_FEC: u8 = 33;

fn build_control_ratet(index: u8) -> Vec<u8> {
    let payload = vec![FIELD_RATET, index];
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let in_path = args.get(2).cloned().unwrap_or_else(|| "in.dat".to_string());

    let in_bytes = fs::read(&in_path).unwrap_or_else(|e| panic!("read {in_path}: {e}"));
    // Raw, headerless 16-bit PCM, native (little-endian, x86) byte order -- matching DVSI's own
    // `fread(sbuf, 2, 160, fpi)` directly into a `short*` buffer on the x86 reference platform.
    // Verified directly (not assumed): interpreting these bytes as little-endian gives realistic
    // speech RMS levels (hundreds to low thousands, with real quiet pauses); big-endian gives
    // near-full-scale-clipped RMS on every single window, which real speech never does.
    let in_samples: Vec<i16> = in_bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    println!("in.dat: {} samples ({:.2}s)", in_samples.len(), in_samples.len() as f64 / 8000.0);

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    sock.send(&build_control_ratet(RATET_HALF_RATE_FEC)).expect("send RATET config");
    let mut buf = [0u8; 512];
    let n = sock.recv(&mut buf).expect("RATET config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
    println!("RATET({RATET_HALF_RATE_FEC}) config ack: type={ptype:#04x} payload={payload:02x?}");

    let num_frames = in_samples.len() / FRAME_SAMPLES;
    let mut zero_error = 0usize;
    let mut speech = 0usize;
    let mut erasure = 0usize;
    let mut silence = 0usize;
    let mut tone = 0usize;
    let mut pitch_trace: Vec<u32> = Vec::new();
    let mut wrong_bit_count = 0usize;

    for frame_idx in 0..num_frames {
        let start = frame_idx * FRAME_SAMPLES;
        let frame = &in_samples[start..start + FRAME_SAMPLES];

        sock.send(&build_speech(frame)).expect("send speech");
        let n = sock.recv(&mut buf).expect("recv channel response");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        let num_bits = payload[1] as usize;
        if num_bits != 72 {
            wrong_bit_count += 1;
            continue;
        }
        let nbytes = num_bits.div_ceil(8);
        let bits = &payload[2..2 + nbytes];

        let mut wire: u128 = 0;
        for &b in bits.iter().take(9) {
            wire = (wire << 8) | b as u128;
        }
        let logical = ap2_interleave::interleaved_to_frame(wire);
        let parsed = parse_frame(logical);
        if parsed.epsilon_c0 == 0 && parsed.epsilon_c1 == 0 {
            zero_error += 1;
        }
        let raw_params = ap2_decode::extract_raw_parameters(parsed.d);
        match ap2_decode::classify_b0(raw_params.b0) {
            ap2_decode::FrameKind::Speech => {
                speech += 1;
                pitch_trace.push(raw_params.b0);
            }
            ap2_decode::FrameKind::Erasure => erasure += 1,
            ap2_decode::FrameKind::Silence => silence += 1,
            ap2_decode::FrameKind::Tone => tone += 1,
        }

        // Send the encoded bits straight back for decode (matching DVSI's own real
        // `send_channel_packet`'s own pass-through of the raw response bytes), synchronously --
        // keeps the chip's own decoder state exercised alongside the encoder's, even though this
        // tool doesn't compare the resulting PCM against cmp.dat (see module doc comment for why).
        let full_channel_pkt = buf[..n].to_vec();
        sock.send(&full_channel_pkt).expect("send channel for decode");
        let _ = sock.recv(&mut buf).expect("recv speech response");
    }

    println!("\n{num_frames} real frames processed ({wrong_bit_count} returned an unexpected bit count, skipped)");
    println!(
        "frame kinds: speech={speech} erasure={erasure} silence={silence} tone={tone}"
    );
    println!(
        "Golay zero-error frames: {zero_error}/{} ({:.1}%)",
        num_frames - wrong_bit_count,
        100.0 * zero_error as f64 / (num_frames - wrong_bit_count) as f64
    );
    println!("\nfirst 60 speech-frame b0 (pitch index) values, real DVSI reference speech:");
    for (i, b0) in pitch_trace.iter().take(60).enumerate() {
        print!("{b0:>3} ");
        if (i + 1) % 20 == 0 {
            println!();
        }
    }
    println!();
}

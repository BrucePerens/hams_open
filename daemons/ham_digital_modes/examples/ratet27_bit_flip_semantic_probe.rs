// SPDX-License-Identifier: LGPL-3.0-or-later
//! Empirically checks DVSI's real RATET(27) bit-to-parameter assignment against this crate's own
//! TIA Fig. 22 layout, using the chip itself as the oracle -- the follow-up
//! `ratet27_diagnose_gain_scale_mismatch.rs` flagged: `b0`'s own within-codeword bit order is
//! demonstrably correct (stable, plausible `l_hat` on real voiced speech), but `b2` barely correlates
//! with real chip loudness at all (`~0.18` on loud frames). A first hypothesis -- that this means
//! DVSI uses a different bit-prioritization scheme than TIA for everything past the fundamental
//! frequency -- turned out to be wrong once this probe was fixed to control for a real confound (see
//! **Result** below). Kept, corrected, as a permanent tool: this is the decisive way to check any
//! future bit-layout question directly against the real chip, not just for this one round.
//!
//! **Method**: since `ambe_fixed_chip_validate_ratet27.rs` already confirmed this crate's own
//! block-membership/FEC-decode assumptions succeed on 3291/3320 real frames with low corrected-error
//! counts, this crate's assumed bit-to-codeword-index order is either the identity or a genuine code
//! automorphism -- so a re-encoded, single-data-bit-flipped codeword is still a *valid* codeword the
//! chip will decode cleanly (not a corrupted frame the chip would reject or repeat). For each of the
//! 88 data bits (in this crate's own `u_hat_0..u_hat_7` numbering): prime the chip decoder with the
//! same real frames every time (so its own predictive state, e.g. `previous_m`, is identical across
//! every flip), decode the target frame's wire block to its data value, flip one bit, re-encode
//! (`golay_encode`/`hamming_encode`, or direct for the unprotected raw block), write the new codeword
//! back to its own wire positions (`block_wire_members`), and send -- followed by `N_FOLLOWUP` real
//! *unmodified* frames, to integrate the predictive decoder's own tail. The signed
//! `sum(log2(rms_flipped/rms_baseline))` across the target frame and its follow-ups reveals what that
//! bit actually controls on the real chip.
//!
//! **First run: confounded by the amplitude-smoothing clamp, not informative.** The first version
//! targeted a *loud* frame and used raw-sample correlation as the metric. Both were mistakes: Eq.
//! 115/116's own `tau_M` clamp resets to a fixed `20480` every clean frame and fires readily on loud
//! frames (`gamma_M` as low as `0.3`), which both compresses a gain bit's RMS effect and makes
//! flipping it *down* release the clamp nonlinearly (a `+243%` RMS outlier that had nothing to do
//! with bit significance); and raw-sample correlation against the dithered baseline turned out to be
//! dominated by `voiced_synthesis`'s phase dither and `unvoiced_synthesis`'s noise generator
//! regardless of which bit was flipped, clustering near zero for every bit and not discriminating
//! anything.
//!
//! **Corrected run: `b2`'s TIA layout is very likely right after all.** Retargeted to frame 111
//! (chip_rms ~395, inside the same stable `l_hat=30-41` voiced stretch, but *moderate* -- the clamp
//! is inactive), and switched to signed `sum(log2ratio)` rather than correlation. With this frame's
//! real `b2=39`, `GAIN_QUANTIZER_LEVELS` predicts each bit's own effect directly; the real chip's
//! measured response matched sign on 5 of 6 predicted bits, with two (`u0` bit 4 and bit 3) matching
//! in *both* sign and rough magnitude -- not plausibly noise:
//!
//! | bit (TIA position)    | predicted | observed |
//! |------------------------|----------:|---------:|
//! | bit5 (`u0` bit 5, MSB) |    -5.62  |   -1.59  |
//! | bit4 (`u0` bit 4)      |    +2.68  |   +3.16  |
//! | bit3 (`u0` bit 3)      |    +1.30  |   +1.03  |
//! | bit2 (`u5` bit 9)      |    -0.71  |   -0.09  |
//! | bit1 (`u5` bit 8)      |    -0.38  |   -0.02  |
//! | bit0 (`u7` bit 3, LSB) |    -0.20  |   +0.04  |
//!
//! (`u5`, not `u4`, because this frame's `l_hat=35` gives `k_hat=12` per `vuv::frequency_bands_count`
//! -- large enough that `b1`'s own `k_hat` voicing bits spill one bit past `u4`'s own 11 bits into
//! `u5`, pushing `b2`'s middle two bits out to `u5` bits 9 and 8 rather than `u4`. Recompute this
//! per-frame; it is not a fixed position.) The earlier weak `0.18`/`0.07` correlation was the clamp
//! decorrelating `b2` from RMS on the sample of loud/moderate frames tested, exactly as suspected --
//! not a real layout mismatch. See `tables::tests::gain_and_higher_order_bit_allocation_match_annex_
//! f_g_for_recurring_l_values` for the follow-up Annex F/G table spot-check this result motivated
//! (also clean). The residual chip/float PCM gap is most plausibly genuine inter-decoder variance.
//!
//! Usage: `cargo run --release --example ratet27_bit_flip_semantic_probe -- <host:port>`

use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::general::fec::{golay_decode, golay_encode, hamming_decode, hamming_encode};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_PRIME: usize = 20; // real priming frames before the flipped test frame.
const N_FOLLOWUP: usize = 3; // real unmodified frames sent after the flip, to integrate the
                              // predictive decoder's tail (rho ~0.7) and average over the
                              // amplitude-smoothing clamp's own frame-to-frame state.
// Frame 111 (chip_rms ~395, inside the l_hat=30-41 stable voiced stretch spanning frames 104-127)
// -- deliberately a *moderate*, not loud, frame: Eq. 115/116's own amplitude-smoothing clamp resets
// tau_M to a fixed 20480 every clean frame and easily fires on loud frames (gamma_M ~0.3-0.6),
// which both compresses a gain bit's RMS effect and makes flipping it *down* release the clamp
// nonlinearly -- exactly the kind of confound that made the first (loud-frame) run of this probe
// unreadable. A moderate frame keeps the clamp inactive so a real gain bit's effect on RMS isn't
// swamped by that nonlinearity.
const START_FRAME: usize = 91; // START_FRAME + N_PRIME (20) = 111, the moderate target frame.
const LENGTHS: [u8; 8] = [12, 12, 12, 12, 11, 11, 11, 7];

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
fn wire_bytes_to_wire_bits(bytes: &[u8; FRAME_BYTES]) -> [bool; 144] {
    let mut bits = [false; 144];
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for b in 0..8 {
            bits[byte_idx * 8 + b] = (byte >> (7 - b)) & 1 == 1;
        }
    }
    bits
}
fn wire_bits_to_wire_bytes(bits: &[bool; 144]) -> [u8; FRAME_BYTES] {
    let mut bytes = [0u8; FRAME_BYTES];
    for (byte_idx, byte) in bytes.iter_mut().enumerate() {
        let mut v = 0u8;
        for b in 0..8 {
            v = (v << 1) | (bits[byte_idx * 8 + b] as u8);
        }
        *byte = v;
    }
    bytes
}
/// Reads a block's own raw (pre-FEC) value from the wire bits, MSB-first over `block_wire_members`'s
/// own natural order -- the same convention `ambe_fixed_chip_validate_ratet27.rs`'s already-validated
/// `wire_bytes_to_c` uses.
fn read_block(wire_bits: &[bool; 144], block: Block) -> u32 {
    let members = block_wire_members(block);
    let mut value: u32 = 0;
    for (offset, &wire) in members.iter().enumerate() {
        if wire_bits[wire] {
            value |= 1 << (members.len() - 1 - offset);
        }
    }
    value
}
/// The exact inverse of [`read_block`]: writes `value`'s own bits (MSB-first) into `block`'s wire
/// positions, leaving every other wire bit untouched.
fn write_block(wire_bits: &mut [bool; 144], block: Block, value: u32) {
    let members = block_wire_members(block);
    for (offset, &wire) in members.iter().enumerate() {
        let bit = (value >> (members.len() - 1 - offset)) & 1 == 1;
        wire_bits[wire] = bit;
    }
}
fn block_for_index(index: usize) -> Block {
    match index {
        0..=3 => Block::Golay { index: index as u8 },
        4..=6 => Block::Hamming { index: (index - 4) as u8 },
        7 => Block::Raw,
        _ => unreachable!(),
    }
}
/// Flips bit `bit_in_block` (0 = LSB) of block `index`'s own *data* value (post-FEC-decode for
/// Golay/Hamming, direct for the unprotected raw block) and re-encodes, returning the new wire bits
/// for the whole frame with every other block untouched.
fn flip_data_bit(baseline_wire_bits: &[bool; 144], index: usize, bit_in_block: u8) -> [bool; 144] {
    let mut wire_bits = *baseline_wire_bits;
    let block = block_for_index(index);
    let raw = read_block(&wire_bits, block);
    let new_wire_value = match block {
        Block::Golay { .. } => {
            let (data, _) = golay_decode(raw);
            let flipped = data ^ (1 << bit_in_block);
            golay_encode(flipped)
        }
        Block::Hamming { .. } => {
            let (data, _) = hamming_decode(raw as u16);
            let flipped = data ^ (1 << bit_in_block);
            hamming_encode(flipped) as u32
        }
        Block::Raw => raw ^ (1 << bit_in_block),
    };
    write_block(&mut wire_bits, block, new_wire_value);
    wire_bits
}
fn rms(pcm: &[f64]) -> f64 {
    (pcm.iter().map(|&s| s * s).sum::<f64>() / pcm.len() as f64).sqrt()
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

    // Capture N_PRIME + 1 + N_FOLLOWUP real channel payloads once (encode direction): priming
    // frames, the target frame to perturb, and real unmodified follow-up frames sent after every
    // flip so the predictive decoder's own tail (rho ~0.7) and the amplitude-smoothing clamp's
    // frame-to-frame state both get integrated into the metric, not just the flipped frame alone.
    let total_frames = N_PRIME + 1 + N_FOLLOWUP;
    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(total_frames);
    for i in 0..total_frames {
        let frame_idx = START_FRAME + i;
        let frame = &pcm[frame_idx * FRAME_SAMPLES..(frame_idx + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        channel_payloads.push(payload.to_vec());
    }
    let priming = &channel_payloads[..N_PRIME];
    let followups = &channel_payloads[N_PRIME + 1..];
    let target_payload = &channel_payloads[N_PRIME];
    let header_bytes = target_payload[..target_payload.len() - FRAME_BYTES].to_vec();
    let mut target_wire_bytes = [0u8; FRAME_BYTES];
    target_wire_bytes.copy_from_slice(&target_payload[target_payload.len() - FRAME_BYTES..]);
    let baseline_wire_bits = wire_bytes_to_wire_bits(&target_wire_bytes);

    // Runs the same N_PRIME-frame priming sequence, sends `wire_bits` as the (N_PRIME+1)-th
    // decode-direction frame, then N_FOLLOWUP real *unmodified* frames -- returning per-frame RMS
    // for the target frame plus every follow-up, so a caller can integrate the predictive decoder's
    // own tail and average over the amplitude-smoothing clamp's frame-to-frame state, instead of
    // reading the target frame alone (which the clamp's own nonlinearity makes a poor bit-
    // significance readout by itself). The chip's own state is identical across every experiment
    // since the same priming sequence runs first every time.
    let mut run_experiment = |wire_bits: &[bool; 144]| -> Vec<Vec<f64>> {
        for p in priming {
            let n = send_recv_retrying(&sock, &mut buf, &build_channel(p));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_SPEECH);
        }
        let mut target_payload = header_bytes.clone();
        target_payload.extend_from_slice(&wire_bits_to_wire_bytes(wire_bits));
        let mut frames = Vec::with_capacity(1 + N_FOLLOWUP);
        for p in std::iter::once(&target_payload).chain(followups.iter()) {
            let n = send_recv_retrying(&sock, &mut buf, &build_channel(p));
            let (ptype, resp_payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_SPEECH);
            frames.push(parse_speech_payload(resp_payload).iter().map(|&s| s as f64).collect());
        }
        frames
    };

    println!(
        "Priming with {N_PRIME} real frames starting at input frame {START_FRAME}, probing the \
         flipped frame at index {}, then {N_FOLLOWUP} real unmodified follow-up frames.",
        START_FRAME + N_PRIME
    );
    let baseline_frames = run_experiment(&baseline_wire_bits);
    let baseline_rms: Vec<f64> = baseline_frames.iter().map(|f| rms(f)).collect();
    println!("Baseline per-frame RMS (target, then {N_FOLLOWUP} follow-ups): {baseline_rms:?}\n");

    // Signed, not absolute, log2(ratio) per frame -- a real gain-field bit should move RMS by
    // roughly a constant step *with a consistent sign* across positions (MSB flips it further than
    // an LSB), and the sign itself tells you which direction that bit's weight runs. Correlation
    // against the dithered baseline waveform was dropped entirely: `voiced_synthesis`'s phase dither
    // and `unvoiced_synthesis`'s noise generator dominate it regardless of which bit is flipped,
    // making it a poor discriminator (see this file's own doc comment on the first, inconclusive
    // run). RMS, summed signed across the target frame and its real follow-ups, is not immune to
    // dither either, but is a far more direct readout of a gain-field bit's actual effect.
    println!("{:>5} {:>4} {:>4} {:>12}", "block", "bit", "MSB#", "sum(log2ratio)");
    for (block_idx, &width) in LENGTHS.iter().enumerate() {
        for bit in (0..width).rev() {
            let msb_pos = width - 1 - bit; // 0 = MSB, matching this crate's own u-vector convention.
            let flipped_bits = flip_data_bit(&baseline_wire_bits, block_idx, bit);
            let flipped_frames = run_experiment(&flipped_bits);
            let flipped_rms: Vec<f64> = flipped_frames.iter().map(|f| rms(f)).collect();
            let sum_log2_ratio: f64 = baseline_rms
                .iter()
                .zip(flipped_rms.iter())
                .map(|(&b, &f)| (f.max(1.0) / b.max(1.0)).log2())
                .sum();
            println!("u{block_idx:<4} {bit:>4} {msb_pos:>4} {sum_log2_ratio:>12.3}");
        }
    }
}

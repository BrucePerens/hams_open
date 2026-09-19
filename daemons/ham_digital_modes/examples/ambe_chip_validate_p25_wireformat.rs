// SPDX-License-Identifier: LGPL-3.0-or-later
//! Falsification-test harness for DVSI AMBE3003's real, undocumented P25 wire-format bit order --
//! see `docs/references/AMBE_CHIP_VALIDATION_FINDINGS.md` section 3's "real next steps" (item 1) for
//! the full background and methodology this implements.
//!
//! Unlike the earlier `ambe_chip_validate_p25.rs`/`ambe_frame_diagnose.rs` harnesses (which compared
//! single captured frames or used correlation -- both shown, after the D-STAR investigation, to be too
//! weak a signal against a genuine *perfect* Golay(23,12,7) code, where literally every 23-bit pattern
//! decodes to *some* codeword within distance <=3), this harness captures MANY real chip-encoded
//! frames and scores each candidate wire-format hypothesis by how many of them Golay-decode with
//! *zero* corrected errors -- the real, decisive falsification test that found and fixed two real
//! framing bugs for D-STAR (see the findings doc section 5).
//!
//! Three families of hypotheses are tried, in increasing order of how much prior structure they
//! assume:
//!
//! 1. A **sliding-window scan**: does a contiguous 23-bit (Golay) or 15-bit (Hamming) window at some
//!    fixed bit offset land on a valid codeword across most captured frames, for each of the 4
//!    byte-order x per-byte-bit-direction combinations? Cheap, and catches an off-by-a-few-bits
//!    header/status-bit issue that a block-permutation search alone could miss.
//! 2. **All `8! = 40320` permutations of which of the 8 fixed-size contiguous blocks (`c0..c3` at 23
//!    bits, `c4..c6` at 15 bits, `c7` at 7 bits) occupies which position in the 144-bit frame**, per
//!    the findings doc's own "real next steps" list. Since this codec's own content-dependent PRN
//!    whitening (confirmed matching AMBETools' real P25 IMBE source, see finding doc item 3) means
//!    `c1..c6` are XORed with a data-dependent mask *derived from `c0`'s own recovered data bits*
//!    before they'll look like valid codewords, scoring is structured as: is the candidate `c0` a
//!    valid Golay codeword? If so, derive `u0` from its own top 12 bits (no search needed -- `c0` is
//!    never whitened, so a valid `c0` codeword directly gives the real data), regenerate the PRN via
//!    this crate's own `ambe::modulation::modulation_vectors`, dewhiten `c1..c6`, and *then* check for
//!    valid codewords. Both the dewhitened and raw (no-whitening-hypothesis) scores are tracked, since
//!    it is not yet known whether the chip's raw serial `CHANNEL` bytes for P25 include this whitening
//!    step at all (D-STAR's chip, once its own two framing bugs were fixed, turned out to already
//!    perform the real over-the-air block-interleave on its serial CHAND bytes directly).
//! 3. A **real, independently-sourced P25 Phase 1 IMBE bit interleave**: `szechyjs/dsd`'s
//!    `include/p25p1_const.h` (`iW`/`iX`/`iY`/`iZ`, fetched and read directly -- ISC-licensed, numeric
//!    values only transcribed here, no code copied) describes the genuine over-the-air dibit
//!    interleave a *real, independent* P25 Phase 1 demodulator uses, cross-checked against
//!    `szechyjs/mbelib`'s `imbe7200x4400.c` (`mbe_eccImbe7200x4400C0`/`Data`,
//!    `mbe_demodulateImbe7200x4400Data`) to confirm the resulting `imbe_fr[8][23]` block/bit-position
//!    convention, the data-bits-first-MSB-first Golay/Hamming layout, and the PRN whitening scope and
//!    formula all match this crate's own `src/ambe/` exactly. This is the same kind of primary-source
//!    cross-check that cracked D-STAR (`dsd`'s `dstar_const.h` + `mbelib`'s `ambe3600x2400.c`) -- worth
//!    trying directly, in case the AMBE3003's serial P25 `CHANNEL` bytes are, like its D-STAR CHAND
//!    bytes, already in real over-the-air bit order.
//!
//! Run live against the chip: `cargo run --release --example ambe_chip_validate_p25_wireformat --
//! 192.168.10.189 2460`. Frames are also saved to a capture file (`--save <path>`, default
//! `p25_wireformat_capture.tsv` in the crate root) so the (expensive-ish, though empirically well
//! under a minute in release) hypothesis search can be re-run without re-hitting the chip, via
//! `--replay <path>` in place of a live host/port.
//!
//! # Safety
//! Only ever sends ordinary DVSI CONTROL/SPEECH UDP packets, exactly like the other committed
//! `ambe_chip_validate_*` harnesses -- never touches the serial/USB layer directly, so the chip's
//! known UART-BREAK lockup hazard (see the findings doc section 6) cannot be triggered by this tool.

use ham_digital_modes::ambe::float::general::fec::{golay_encode, hamming_encode};
use ham_digital_modes::ambe::float::ratet27::modulation::modulation_vectors;
use ham_digital_modes::ambe::float::ratet27::pitch_refinement::RefinementFrame;
use ham_digital_modes::ambe::float::ratet27::{encode_frame as ambe_encode_frame, FrameState};
use std::collections::HashSet;
use std::f64::consts::PI;
use std::fs;
use std::io::Write as _;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const DISCARD_ONSET: usize = 10;
const FRAMES_PER_COMBO: usize = 30;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;

/// The task-confirmed P25 FEC rate-control-word (6 x u16, big-endian, DVSI CONTROL field 0x0A).
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
/// RATET index 27 (7200/4400/2800 bps), this codec's own established default rate -- captured too,
/// separately, to check whether it produces the same wire layout as the dedicated RATEP word above
/// (see findings doc section 1 for why index 27 is this exact codec's own rate).
const RATET_INDEX_27: u8 = 27;

/// Block sizes in bits, `c0..c7`, per this codec's own confirmed frame layout (findings doc item 3).
const BLOCK_SIZES: [u32; 8] = [23, 23, 23, 23, 15, 15, 15, 7];

const PRIMARY_TONES_HZ: [f64; 6] = [100.0, 150.0, 200.0, 300.0, 400.0, 600.0];
const PRIMARY_AMPLITUDES: [f64; 2] = [2000.0, 8000.0];
const SECONDARY_TONES_HZ: [f64; 3] = [100.0, 200.0, 400.0];
const SECONDARY_AMPLITUDES: [f64; 1] = [4000.0];

/// Cap on unique frames fed into the (comparatively) expensive 8!-permutation search, to keep the
/// worst case bounded regardless of how many were captured -- the cheap sliding-window and interleave
/// checks below still use the full unique set.
const MAX_FRAMES_FOR_PERMUTATION_SEARCH: usize = 200;

// ---------------------------------------------------------------------------------------------
// DVSI UDP packet framing (identical to ambe_chip_validate_dstar.rs / ambe_chip_validate_p25.rs).
// ---------------------------------------------------------------------------------------------

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

fn encode_frame(sock: &UdpSocket, samples: &[i16]) -> Option<[u8; 18]> {
    sock.send(&build_speech(samples)).ok()?;
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).ok()?;
    let (ptype, payload) = parse_packet(&buf[..n])?;
    if ptype != TYPE_CHANNEL || payload.len() < 2 {
        return None;
    }
    let num_bits = payload[1] as usize;
    if num_bits != 144 {
        eprintln!("warning: chip returned {num_bits} bits, expected 144");
        return None;
    }
    let bits = payload.get(2..20)?;
    let mut out = [0u8; 18];
    out.copy_from_slice(bits);
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Test-tone generation, continuous phase across frames (not frame-relative) for maximum bit
// diversity across captured frames regardless of whether a tone's period evenly divides 160
// samples -- sidesteps the inter-frame phase-drift confound the findings doc's D-STAR section hit.
// ---------------------------------------------------------------------------------------------

fn capture_combo(sock: &UdpSocket, freq: f64, amplitude: f64, tag: &str) -> Vec<[u8; 18]> {
    let omega = 2.0 * PI * freq / SAMPLE_RATE;
    let mut frames = Vec::with_capacity(FRAMES_PER_COMBO);
    let mut sample_index: u64 = 0;
    for i in 0..(DISCARD_ONSET + FRAMES_PER_COMBO) {
        let samples: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|n| {
                let t = (sample_index + n as u64) as f64;
                (amplitude * (omega * t).sin()).round() as i16
            })
            .collect();
        sample_index += FRAME_SAMPLES as u64;
        match encode_frame(sock, &samples) {
            Some(bytes) if i >= DISCARD_ONSET => frames.push(bytes),
            Some(_) => {}
            None => eprintln!("  {tag} {freq}Hz amp{amplitude}: no/bad response on frame {i}"),
        }
    }
    frames
}

fn capture_all(sock: &UdpSocket, tag: &str, tones: &[f64], amplitudes: &[f64]) -> Vec<[u8; 18]> {
    let mut all = Vec::new();
    for &freq in tones {
        for &amp in amplitudes {
            all.extend(capture_combo(sock, freq, amp, tag));
        }
    }
    all
}

fn save_capture(path: &str, sets: &[(&str, &[[u8; 18]])]) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    for (tag, frames) in sets {
        for frame in frames.iter() {
            writeln!(f, "{tag}\t{}", hex(frame))?;
        }
    }
    Ok(())
}

fn load_capture(path: &str) -> std::io::Result<Vec<(String, [u8; 18])>> {
    let contents = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in contents.lines() {
        let Some((tag, hexstr)) = line.split_once('\t') else {
            continue;
        };
        if hexstr.len() != 36 {
            continue;
        }
        let mut frame = [0u8; 18];
        for i in 0..18 {
            frame[i] = u8::from_str_radix(&hexstr[2 * i..2 * i + 2], 16).unwrap_or(0);
        }
        out.push((tag.to_string(), frame));
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Packs `c[0..8]` (widths 23,23,23,23,15,15,15,7 = 144 bits) MSB-first into 18 bytes -- the
/// "natural" convention `ambe_chip_validate_p25.rs` already uses for its own encoder's output.
fn pack_frame_msb_first(c: [u32; 8]) -> [u8; 18] {
    let mut bits: Vec<bool> = Vec::with_capacity(144);
    for (&val, &width) in c.iter().zip(BLOCK_SIZES.iter()) {
        for b in (0..width).rev() {
            bits.push((val >> b) & 1 == 1);
        }
    }
    let mut out = [0u8; 18];
    for (i, chunk) in bits.chunks(8).enumerate() {
        let mut byte = 0u8;
        for (j, &bit) in chunk.iter().enumerate() {
            if bit {
                byte |= 1 << (7 - j);
            }
        }
        out[i] = byte;
    }
    out
}

/// Self-test: generates real frames from this crate's own from-spec encoder (known ground truth --
/// `encode_frame`'s returned `c` is already fully modulated/whitened, see `ambe/mod.rs`'s
/// `encode_code_vectors`) and packs them with the identity block order at normal-byte/MSB-first, to
/// confirm this harness's own scoring pipeline (dewhitening + membership checks) actually recovers a
/// ~100% match rate when the ground truth is known -- a sanity check on the harness itself, run before
/// trusting a clean negative result against the real chip.
fn selftest_frames(count: usize) -> Vec<[u8; 18]> {
    let sample_rate = SAMPLE_RATE;
    let tone_hz = 200.0;
    let omega0_hat = 2.0 * PI * tone_hz / sample_rate;
    let total_samples = count * FRAME_SAMPLES + 400;
    let raw: Vec<f64> = (0..total_samples)
        .map(|n| 8000.0 * (2.0 * PI * tone_hz * n as f64 / sample_rate).sin())
        .collect();
    let mut state = FrameState::initial();
    let mut out = Vec::with_capacity(count);
    for frame_idx in 0..count {
        let center = frame_idx * FRAME_SAMPLES + FRAME_SAMPLES / 2 + 110;
        let frame = RefinementFrame::new(&raw, center);
        let (c, next_state) = ambe_encode_frame(&frame, omega0_hat, 0.001, &state, false)
            .expect("a clean stationary tone should always encode");
        out.push(pack_frame_msb_first(c));
        state = next_state;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Bit-order hypothesis machinery.
// ---------------------------------------------------------------------------------------------

/// Expands an 18-byte frame into its 144-bit stream under one of the 4 byte-order x per-byte
/// bit-direction hypotheses.
fn frame_to_bits(frame: &[u8; 18], reverse_bytes: bool, lsb_first: bool) -> [u8; 144] {
    let mut bytes = *frame;
    if reverse_bytes {
        bytes.reverse();
    }
    let mut bits = [0u8; 144];
    for (i, &byte) in bytes.iter().enumerate() {
        for b in 0..8 {
            bits[i * 8 + b] = if lsb_first { (byte >> b) & 1 } else { (byte >> (7 - b)) & 1 };
        }
    }
    bits
}

/// Extracts `len` bits starting at `start` as an MSB-first integer (matching `fec::golay_encode`'s
/// own "low N bits meaningful, MSB-first" convention).
fn extract(bits: &[u8; 144], start: usize, len: usize) -> u32 {
    let mut v = 0u32;
    for i in 0..len {
        v = (v << 1) | bits[start + i] as u32;
    }
    v
}

const BYTE_BIT_HYPOTHESES: [(bool, bool); 4] = [(false, false), (false, true), (true, false), (true, true)];

fn hypothesis_name(reverse_bytes: bool, lsb_first: bool) -> &'static str {
    match (reverse_bytes, lsb_first) {
        (false, false) => "normal bytes, MSB-first",
        (false, true) => "normal bytes, LSB-first",
        (true, false) => "reversed bytes, MSB-first",
        (true, true) => "reversed bytes, LSB-first",
    }
}

// ---------------------------------------------------------------------------------------------
// Valid-codeword bitmaps (O(1) exact-match membership -- no need for golay_decode's own O(4096)
// brute-force search at all here, since "zero corrected errors" is exactly "is a valid codeword").
// ---------------------------------------------------------------------------------------------

fn build_golay_bitmap() -> Vec<u64> {
    let mut bm = vec![0u64; (1 << 23) / 64];
    for data in 0u32..4096 {
        let cw = golay_encode(data as u16) as usize;
        bm[cw >> 6] |= 1u64 << (cw & 63);
    }
    bm
}

fn build_hamming_bitmap() -> Vec<u64> {
    let mut bm = vec![0u64; (1 << 15) / 64];
    for data in 0u32..2048 {
        let cw = hamming_encode(data as u16) as usize;
        bm[cw >> 6] |= 1u64 << (cw & 63);
    }
    bm
}

fn golay_valid(bm: &[u64], cw: u32) -> bool {
    let cw = cw as usize & 0x7F_FFFF;
    (bm[cw >> 6] >> (cw & 63)) & 1 == 1
}

fn hamming_valid(bm: &[u64], cw: u32) -> bool {
    let cw = cw as usize & 0x7FFF;
    (bm[cw >> 6] >> (cw & 63)) & 1 == 1
}

/// Scores one candidate `[c0..c7]` extraction (already bit-order-hypothesized) against the
/// falsification test. Returns `None` if `c0` itself isn't even a valid codeword (the cheap, common
/// case -- `c0` is never whitened per Eq. 86, so a valid `c0` is meaningful all by itself). Otherwise
/// returns `(raw_c1c2c3_all_valid, dewhitened_c1c2c3_all_valid, dewhitened_all_seven_valid)`.
fn score_candidate(c: [u32; 8], golay_bm: &[u64], hamming_bm: &[u64]) -> Option<(bool, bool, bool)> {
    if !golay_valid(golay_bm, c[0]) {
        return None;
    }
    let u0 = (c[0] >> 11) & 0x0FFF;
    let raw4 = golay_valid(golay_bm, c[1]) && golay_valid(golay_bm, c[2]) && golay_valid(golay_bm, c[3]);
    let m = modulation_vectors(u0);
    let (d1, d2, d3) = (c[1] ^ m[1], c[2] ^ m[2], c[3] ^ m[3]);
    let dew4 = golay_valid(golay_bm, d1) && golay_valid(golay_bm, d2) && golay_valid(golay_bm, d3);
    let (h4, h5, h6) = (c[4] ^ m[4], c[5] ^ m[5], c[6] ^ m[6]);
    let dew7 = dew4 && hamming_valid(hamming_bm, h4) && hamming_valid(hamming_bm, h5) && hamming_valid(hamming_bm, h6);
    Some((raw4, dew4, dew7))
}

// ---------------------------------------------------------------------------------------------
// Test 1: sliding-window diagnostic.
// ---------------------------------------------------------------------------------------------

fn sliding_window_diagnostic(unique_frames: &[[u8; 18]], golay_bm: &[u64], hamming_bm: &[u64]) {
    println!("\n-- Sliding-window diagnostic ({} unique frames) --", unique_frames.len());
    for &(reverse_bytes, lsb_first) in &BYTE_BIT_HYPOTHESES {
        let bitstreams: Vec<[u8; 144]> =
            unique_frames.iter().map(|f| frame_to_bits(f, reverse_bytes, lsb_first)).collect();
        let mut best_golay = (0usize, 0usize);
        for offset in 0..=(144 - 23) {
            let count = bitstreams.iter().filter(|b| golay_valid(golay_bm, extract(b, offset, 23))).count();
            if count > best_golay.1 {
                best_golay = (offset, count);
            }
        }
        let mut best_hamming = (0usize, 0usize);
        for offset in 0..=(144 - 15) {
            let count = bitstreams.iter().filter(|b| hamming_valid(hamming_bm, extract(b, offset, 15))).count();
            if count > best_hamming.1 {
                best_hamming = (offset, count);
            }
        }
        println!(
            "  {}: best 23-bit-window offset {} ({}/{} valid Golay), best 15-bit-window offset {} ({}/{} valid Hamming)",
            hypothesis_name(reverse_bytes, lsb_first),
            best_golay.0,
            best_golay.1,
            unique_frames.len(),
            best_hamming.0,
            best_hamming.1,
            unique_frames.len(),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Test 2: the 8! block-order permutation search.
// ---------------------------------------------------------------------------------------------

fn generate_permutations() -> Vec<[u8; 8]> {
    let mut out = Vec::with_capacity(40320);
    let mut arr = [0u8, 1, 2, 3, 4, 5, 6, 7];
    fn permute(arr: &mut [u8; 8], k: usize, out: &mut Vec<[u8; 8]>) {
        if k == arr.len() {
            out.push(*arr);
            return;
        }
        for i in k..arr.len() {
            arr.swap(k, i);
            permute(arr, k + 1, out);
            arr.swap(k, i);
        }
    }
    permute(&mut arr, 0, &mut out);
    out
}

struct PermHit {
    reverse_bytes: bool,
    lsb_first: bool,
    perm: [u8; 8],
    raw4: usize,
    dew4: usize,
    dew7: usize,
    total: usize,
}

fn permutation_search(unique_frames: &[[u8; 18]], golay_bm: &[u64], hamming_bm: &[u64]) -> Vec<PermHit> {
    let frames: Vec<[u8; 18]> = if unique_frames.len() > MAX_FRAMES_FOR_PERMUTATION_SEARCH {
        unique_frames[..MAX_FRAMES_FOR_PERMUTATION_SEARCH].to_vec()
    } else {
        unique_frames.to_vec()
    };
    println!(
        "\n-- 8!-permutation search ({} frames used, capped at {}) --",
        frames.len(),
        MAX_FRAMES_FOR_PERMUTATION_SEARCH
    );
    let perms = generate_permutations();
    println!("  generated {} permutations", perms.len());

    let mut hits = Vec::new();
    let start = Instant::now();
    for &(reverse_bytes, lsb_first) in &BYTE_BIT_HYPOTHESES {
        let bitstreams: Vec<[u8; 144]> = frames.iter().map(|f| frame_to_bits(f, reverse_bytes, lsb_first)).collect();
        for perm in &perms {
            let mut id_start = [0u32; 8];
            let mut offset = 0u32;
            for &id in perm.iter() {
                id_start[id as usize] = offset;
                offset += BLOCK_SIZES[id as usize];
            }
            let mut raw4 = 0usize;
            let mut dew4 = 0usize;
            let mut dew7 = 0usize;
            for bits in &bitstreams {
                let c0 = extract(bits, id_start[0] as usize, 23);
                if !golay_valid(golay_bm, c0) {
                    continue;
                }
                let c: [u32; 8] = std::array::from_fn(|id| extract(bits, id_start[id] as usize, BLOCK_SIZES[id] as usize));
                if let Some((r4, d4, d7)) = score_candidate(c, golay_bm, hamming_bm) {
                    if r4 {
                        raw4 += 1;
                    }
                    if d4 {
                        dew4 += 1;
                    }
                    if d7 {
                        dew7 += 1;
                    }
                }
            }
            if raw4 >= 2 || dew4 >= 2 {
                hits.push(PermHit {
                    reverse_bytes,
                    lsb_first,
                    perm: *perm,
                    raw4,
                    dew4,
                    dew7,
                    total: bitstreams.len(),
                });
            }
        }
    }
    let elapsed = start.elapsed();
    println!("  search took {:.2}s ({} hits above threshold)", elapsed.as_secs_f64(), hits.len());
    if elapsed > Duration::from_secs(180) {
        eprintln!("  WARNING: permutation search took over 3 minutes -- see task's speed constraint.");
    }
    hits.sort_by(|a, b| (b.dew4, b.raw4).cmp(&(a.dew4, a.raw4)));
    hits
}

// ---------------------------------------------------------------------------------------------
// Test 3: the real, independently-sourced dsd/mbelib P25 Phase 1 IMBE OTA interleave.
// ---------------------------------------------------------------------------------------------

// Numeric values only, transcribed from szechyjs/dsd's ISC-licensed `include/p25p1_const.h`
// (fetched directly from GitHub) -- ISC is a permissive license (not GPL), and per this project's
// own established position on the mbelib tables (findings doc section 4), cross-referencing numeric
// table *values* like this is fine; the interleave-application logic below is freshly written, not
// copied from dsd's own C source.
const DSD_IW: [u8; 72] = [
    0, 2, 4, 1, 3, 5, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2,
    5, 1, 3, 6, 0, 2, 5, 1, 3, 6, 0, 2, 5, 1, 3, 7, 0, 2, 5, 1, 3, 7, 0, 2, 5, 1, 4, 7, 0, 3, 5, 2, 4, 7,
];
const DSD_IX: [u8; 72] = [
    22, 20, 10, 20, 18, 0, 20, 18, 8, 18, 16, 13, 18, 16, 6, 16, 14, 11, 16, 14, 4, 14, 12, 9, 14, 12, 2, 12, 10, 7,
    12, 10, 0, 10, 8, 5, 10, 8, 13, 8, 6, 3, 8, 6, 11, 6, 4, 1, 6, 4, 9, 4, 2, 6, 4, 2, 7, 2, 0, 4, 2, 0, 5, 0, 13,
    2, 0, 21, 3, 21, 11, 0,
];
const DSD_IY: [u8; 72] = [
    1, 3, 5, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2, 4, 1, 3, 6, 0, 2, 5, 1, 3,
    6, 0, 2, 5, 1, 3, 6, 0, 2, 5, 1, 3, 6, 0, 2, 5, 1, 3, 7, 0, 2, 5, 1, 4, 7, 0, 3, 5, 2, 4, 7, 1, 3, 5,
];
const DSD_IZ: [u8; 72] = [
    21, 19, 1, 21, 19, 9, 19, 17, 14, 19, 17, 7, 17, 15, 12, 17, 15, 5, 15, 13, 10, 15, 13, 3, 13, 11, 8, 13, 11, 1,
    11, 9, 6, 11, 9, 14, 9, 7, 4, 9, 7, 12, 7, 5, 2, 7, 5, 10, 5, 3, 0, 5, 3, 8, 3, 1, 5, 3, 1, 6, 1, 14, 3, 1, 22,
    4, 22, 12, 1, 22, 20, 2,
];

/// Groups a 144-bit stream into 72 "dibits" (consecutive bit pairs) and scatters them into an
/// `[c0..c7]` block array per `dsd`'s own real OTA interleave schedule, cross-checked against
/// `mbelib::imbe7200x4400.c`'s `imbe_fr[block][bitpos]` convention (bit weight `2^bitpos` within each
/// block, i.e. index 22 is a Golay block's MSB) -- confirmed to match this crate's own
/// `fec::golay_encode`/`hamming_encode` MSB-first packed-integer convention exactly.
/// `bit1_is_first` picks which physical bit of each pair plays the demodulator's own "bit 1" (dsd's
/// `(dibit >> 1) & 1`) role, since no independent evidence yet fixes that convention for this wire.
fn apply_dsd_interleave(bits: &[u8; 144], bit1_is_first: bool) -> [u32; 8] {
    let mut block_bits = [[0u8; 23]; 8];
    for j in 0..72 {
        let a = bits[2 * j];
        let b = bits[2 * j + 1];
        let (bit1, bit0) = if bit1_is_first { (a, b) } else { (b, a) };
        block_bits[DSD_IW[j] as usize][DSD_IX[j] as usize] = bit1;
        block_bits[DSD_IY[j] as usize][DSD_IZ[j] as usize] = bit0;
    }
    std::array::from_fn(|id| {
        let size = BLOCK_SIZES[id] as usize;
        (0..size).fold(0u32, |acc, i| acc | ((block_bits[id][i] as u32) << i))
    })
}

fn dsd_interleave_test(unique_frames: &[[u8; 18]], golay_bm: &[u64], hamming_bm: &[u64]) {
    println!("\n-- dsd/mbelib real P25 Phase 1 IMBE OTA interleave test ({} unique frames) --", unique_frames.len());
    for &(reverse_bytes, lsb_first) in &BYTE_BIT_HYPOTHESES {
        let bitstreams: Vec<[u8; 144]> =
            unique_frames.iter().map(|f| frame_to_bits(f, reverse_bytes, lsb_first)).collect();
        for &bit1_is_first in &[false, true] {
            let mut raw4 = 0usize;
            let mut dew4 = 0usize;
            let mut dew7 = 0usize;
            for bits in &bitstreams {
                let c = apply_dsd_interleave(bits, bit1_is_first);
                if let Some((r4, d4, d7)) = score_candidate(c, golay_bm, hamming_bm) {
                    if r4 {
                        raw4 += 1;
                    }
                    if d4 {
                        dew4 += 1;
                    }
                    if d7 {
                        dew7 += 1;
                    }
                }
            }
            println!(
                "  {}, dibit bit1-first={}: raw c1-3 match {}/{}, dewhitened c1-3 match {}/{}, dewhitened all-7 match {}/{}",
                hypothesis_name(reverse_bytes, lsb_first),
                bit1_is_first,
                raw4,
                bitstreams.len(),
                dew4,
                bitstreams.len(),
                dew7,
                bitstreams.len(),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Test 4: the REAL, official TIA-102.BAAA-A Table 5-1 ("Interleaving Schedule for Voice Word") --
// the actual published standard's own voice-frame interleave, as opposed to the third-party `dsd`
// reconstruction tried in test 3. Transcribed programmatically (not by eye) from the real PDF text
// (`reference/ambe/TIA-102-BAAA-A_Project_25_FDMA_CAI.pdf` in `hams_com`, fetched via two
// independent extraction methods -- `pdftotext -layout` regex-parsed, and PyMuPDF's own
// word-position extraction -- which agreed exactly on all 72 symbols, and both independently
// confirmed each of the 8 blocks' own bit-index set is the exact expected contiguous range with no
// gaps or duplicates). Unlike D-STAR's third-party `dsd` table, this is the actual standard's own
// text, Table 5-1, giving each of the 72 transmitted dibit symbols' own two bits (`Bit 1`/`Bit 0`)
// directly as `c_X(i)` -- codeword `X`, bit index `i` (`22`=MSB for a Golay block, `14`=MSB for a
// Hamming block, `6`=MSB for `c_7`, matching this crate's own MSB-first convention exactly, already
// confirmed independently). No bit1/bit0-order ambiguity here (unlike the `dsd` dibit-convention
// question in test 3): the standard names Bit 1 and Bit 0 explicitly per symbol.
// ---------------------------------------------------------------------------------------------

const TIA_BLOCK: [usize; 144] = [
    0, 1, 2, 3, 4, 5, 1, 0, 3, 2, 5, 4, 0, 1, 2, 3, 4, 6, 1, 0, 3, 2, 6, 4, 0, 1, 2, 3, 4, 6, 1, 0,
    3, 2, 6, 4, 0, 1, 2, 3, 4, 6, 1, 0, 3, 2, 6, 4, 0, 1, 2, 3, 4, 6, 1, 0, 3, 2, 6, 4, 0, 1, 2, 3,
    4, 6, 1, 0, 3, 2, 6, 5, 0, 1, 2, 3, 5, 6, 1, 0, 3, 2, 6, 5, 0, 1, 2, 3, 5, 6, 1, 0, 3, 2, 6, 5,
    0, 1, 2, 3, 5, 6, 1, 0, 3, 2, 7, 5, 0, 1, 2, 3, 5, 7, 1, 0, 3, 2, 7, 5, 0, 1, 2, 4, 5, 7, 1, 0,
    4, 3, 7, 5, 0, 2, 3, 4, 5, 7, 2, 1, 4, 3, 7, 5,
];
const TIA_INDEX: [usize; 144] = [
    22, 21, 20, 19, 10, 1, 20, 21, 18, 19, 0, 9, 20, 19, 18, 17, 8, 14, 18, 19, 16, 17, 13, 7, 18,
    17, 16, 15, 6, 12, 16, 17, 14, 15, 11, 5, 16, 15, 14, 13, 4, 10, 14, 15, 12, 13, 9, 3, 14, 13,
    12, 11, 2, 8, 12, 13, 10, 11, 7, 1, 12, 11, 10, 9, 0, 6, 10, 11, 8, 9, 5, 14, 10, 9, 8, 7, 13,
    4, 8, 9, 6, 7, 3, 12, 8, 7, 6, 5, 11, 2, 6, 7, 4, 5, 1, 10, 6, 5, 4, 3, 9, 0, 4, 5, 2, 3, 6, 8,
    4, 3, 2, 1, 7, 5, 2, 3, 0, 1, 4, 6, 2, 1, 0, 14, 5, 3, 0, 1, 13, 22, 2, 4, 0, 22, 21, 12, 3, 1,
    21, 22, 11, 20, 0, 2,
];

/// Deinterleaves wire bits (already extracted under a byte/bit-order hypothesis) into `[c0..c7]`
/// via the real, official Table 5-1: wire bit `2*s` is symbol `s`'s Bit 1, wire bit `2*s+1` is its
/// Bit 0, and `TIA_BLOCK[i]`/`TIA_INDEX[i]` give which codeword and bit index wire bit `i` belongs to.
fn apply_tia_interleave(bits: &[u8; 144]) -> [u32; 8] {
    let mut block_bits = [[0u8; 23]; 8];
    for i in 0..144 {
        block_bits[TIA_BLOCK[i]][TIA_INDEX[i]] = bits[i];
    }
    std::array::from_fn(|id| {
        let size = BLOCK_SIZES[id] as usize;
        (0..size).fold(0u32, |acc, i| acc | ((block_bits[id][i] as u32) << i))
    })
}

fn tia_interleave_test(unique_frames: &[[u8; 18]], golay_bm: &[u64], hamming_bm: &[u64]) {
    println!("\n-- Real TIA-102.BAAA-A Table 5-1 interleave test ({} unique frames) --", unique_frames.len());
    for &(reverse_bytes, lsb_first) in &BYTE_BIT_HYPOTHESES {
        let bitstreams: Vec<[u8; 144]> =
            unique_frames.iter().map(|f| frame_to_bits(f, reverse_bytes, lsb_first)).collect();
        let mut raw4 = 0usize;
        let mut dew4 = 0usize;
        let mut dew7 = 0usize;
        for bits in &bitstreams {
            let c = apply_tia_interleave(bits);
            if let Some((r4, d4, d7)) = score_candidate(c, golay_bm, hamming_bm) {
                if r4 {
                    raw4 += 1;
                }
                if d4 {
                    dew4 += 1;
                }
                if d7 {
                    dew7 += 1;
                }
            }
        }
        println!(
            "  {}: raw c1-3 match {}/{}, dewhitened c1-3 match {}/{}, dewhitened all-7 match {}/{}",
            hypothesis_name(reverse_bytes, lsb_first),
            raw4,
            bitstreams.len(),
            dew4,
            bitstreams.len(),
            dew7,
            bitstreams.len(),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let replay_idx = args.iter().position(|a| a == "--replay");
    let save_idx = args.iter().position(|a| a == "--save");
    let save_path = save_idx
        .map(|i| args[i + 1].clone())
        .unwrap_or_else(|| "p25_wireformat_capture.tsv".to_string());

    let mut tagged_frames: Vec<(String, [u8; 18])> = Vec::new();

    if args.iter().any(|a| a == "--selftest") {
        println!("Self-test mode: generating frames from this crate's own encoder (known ground truth), no chip contact.");
        for frame in selftest_frames(40) {
            tagged_frames.push(("selftest_own_encoder".to_string(), frame));
        }
    } else if let Some(idx) = replay_idx {
        let path = &args[idx + 1];
        println!("Replaying captured frames from {path}");
        tagged_frames = load_capture(path).unwrap_or_else(|e| panic!("failed to load {path}: {e}"));
    } else {
        let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189".to_string());
        let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2460);
        let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
        sock.connect((host.as_str(), port)).unwrap_or_else(|e| panic!("connect to {host}:{port}: {e}"));
        sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        // Primary capture: the task-confirmed P25 FEC RATEP word.
        sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
        let mut buf = [0u8; 256];
        let n = sock.recv(&mut buf).expect("RATEP config response");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
        println!("RATEP(P25 FEC) config ack: type={ptype:#04x} payload={payload:02x?}");
        let primary = capture_all(&sock, "ratep_p25_fec", &PRIMARY_TONES_HZ, &PRIMARY_AMPLITUDES);
        println!("Captured {} frames under RATEP(P25 FEC).", primary.len());

        // Secondary capture: RATET index 27 -- same 144-bit rate, different config path, checked in
        // case the two configs yield a different wire layout (a real, separate finding either way).
        sock.send(&build_control_ratet(RATET_INDEX_27)).expect("send RATET config");
        let n = sock.recv(&mut buf).expect("RATET config response");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
        println!("RATET(27) config ack: type={ptype:#04x} payload={payload:02x?}");
        let secondary = capture_all(&sock, "ratet_27", &SECONDARY_TONES_HZ, &SECONDARY_AMPLITUDES);
        println!("Captured {} frames under RATET(27).", secondary.len());

        save_capture(&save_path, &[("ratep_p25_fec", &primary), ("ratet_27", &secondary)])
            .unwrap_or_else(|e| eprintln!("warning: failed to save capture to {save_path}: {e}"));
        println!("Saved capture to {save_path} (replay with --replay {save_path}).");

        for (tag, frame) in primary.into_iter().map(|f| ("ratep_p25_fec".to_string(), f)) {
            tagged_frames.push((tag, frame));
        }
        for (tag, frame) in secondary.into_iter().map(|f| ("ratet_27".to_string(), f)) {
            tagged_frames.push((tag, frame));
        }
    }

    let golay_bm = build_golay_bitmap();
    let hamming_bm = build_hamming_bitmap();

    for tag in ["ratep_p25_fec", "ratet_27", "selftest_own_encoder"] {
        let raw: Vec<[u8; 18]> = tagged_frames.iter().filter(|(t, _)| t == tag).map(|(_, f)| *f).collect();
        if raw.is_empty() {
            continue;
        }
        let unique: Vec<[u8; 18]> = raw.iter().copied().collect::<HashSet<_>>().into_iter().collect();
        println!(
            "\n=========================================================\nConfig: {tag} -- {} frames captured, {} unique\n=========================================================",
            raw.len(),
            unique.len()
        );

        sliding_window_diagnostic(&unique, &golay_bm, &hamming_bm);
        dsd_interleave_test(&unique, &golay_bm, &hamming_bm);
        tia_interleave_test(&unique, &golay_bm, &hamming_bm);
        let hits = permutation_search(&unique, &golay_bm, &hamming_bm);

        if hits.is_empty() {
            println!("\n  No block-order permutation scored above the noise threshold for {tag}.");
        } else {
            println!("\n  Top permutation-search hits for {tag} (sorted by dewhitened c1-3 match count):");
            for hit in hits.iter().take(10) {
                println!(
                    "    {}, perm(slot->block id)={:?}: raw4={}/{}, dew4={}/{}, dew7(all Golay+Hamming)={}/{}",
                    hypothesis_name(hit.reverse_bytes, hit.lsb_first),
                    hit.perm,
                    hit.raw4,
                    hit.total,
                    hit.dew4,
                    hit.total,
                    hit.dew7,
                    hit.total,
                );
            }
        }
    }

    println!(
        "\nDone. If any hypothesis above scored near 100% exact match, that's a real, decisive finding \
         -- write it up in docs/references/AMBE_CHIP_VALIDATION_FINDINGS.md (append-only) per the task's \
         instructions and stop for review before touching any production src/ambe/ code. If nothing \
         scored meaningfully above chance, that's the honest negative result to write up instead."
    );
}

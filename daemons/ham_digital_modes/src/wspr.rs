// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]

//! WSPR (Weak Signal Propagation Reporter, K1JT) message encode and
//! audio synthesis, plus the bit-packing primitives (`pack_call()`/
//! `pack_grid4_power()` and their own inverses, `unpack_call()`/
//! `unpack_grid4_power()` below) that both this module's own encoder
//! and the sibling decode modules (`wspr_decode.rs`'s sequential
//! decoder, `wspr_sync.rs`'s sync search) share. Real research, not
//! guessed: every publicly available WSPR *decoder* traces back to the
//! same K1JT/K9AN reference implementation, GPLv3-licensed (confirmed
//! directly: WSJT-X's own wsprd, and every fork of it found while
//! researching this) -- incompatible with vendoring into this
//! proprietary, trade-secret-licensed codebase, the same reasoning
//! docs/proposals/DIGITAL_MODES.md already applied to FT8's decode side.
//! This module's own decode-side comment used to say decode was
//! deferred entirely for that reason; it has since been built as an
//! independently-written implementation instead (not vendored from any
//! GPL source) -- see `wspr_decode.rs`'s and `wspr_sync.rs`'s own doc
//! comments for the full trace of that decision and the real license
//! research (`libfec` vs. the actually-relevant, unlicensed `libfano`)
//! behind it. Encode remains a much smaller, fully specified, standard
//! algorithm with no low-SNR statistical component at all, implemented
//! here in full.
//!
//! The protocol facts below (bit-packing layout, the K=32 rate-1/2
//! convolutional code's generator polynomials, the 162-symbol sync
//! vector, the interleaving scheme, symbol timing) were extracted by
//! reading (not copying) github.com/ast/wsprd's `wsprsim_utils.c` and
//! `fano.c` -- protocol/format facts, not copyrightable expression, and
//! necessarily identical across every independent WSPR implementation
//! for interop, the same way this codebase's psk31.rs treats the fixed
//! Varicode table. Every numeric constant below (polynomials, sync
//! vector, symbol timing) was cross-checked against Wikipedia's
//! independently-maintained WSPR protocol page. Scope: Type 1 messages
//! only (a standard callsign, a 4-character grid locator, and a power
//! level in dBm -- e.g. "K6BP EN50 33") -- Type 2 (compound prefix/
//! suffix callsigns) and Type 3 (hashed callsign + 6-character grid)
//! are real, documented message types this doesn't attempt.

pub(crate) const WSPR_SYMBOL_RATE_HZ: f64 = 12000.0 / 8192.0; // 1.4648... baud
const WSPR_TONE_SPACING_HZ: f64 = WSPR_SYMBOL_RATE_HZ;
pub(crate) const WSPR_NUM_SYMBOLS: usize = 162;

// K=32, rate 1/2 convolutional code (the "Layland-Lushbaugh" polynomials
// WSPR itself uses -- verified directly against fano.c's #ifdef LL block,
// the variant WSJT-X actually builds with).
pub(crate) const POLY1: u32 = 0xf2d0_5351;
pub(crate) const POLY2: u32 = 0xe461_3c47;

// The fixed 162-symbol sync vector every WSPR receiver expects, exactly
// as published (protocol constant, not implementation-specific).
#[rustfmt::skip]
pub(crate) const SYNC_VECTOR: [u8; 162] = [
    1,1,0,0,0,0,0,0,1,0, 0,0,1,1,1,0,0,0,1,0,
    0,1,0,1,1,1,1,0,0,0, 0,0,0,0,1,0,0,1,0,1,
    0,0,0,0,0,0,1,0,1,1, 0,0,1,1,0,1,0,0,0,1,
    1,0,1,0,0,0,0,1,1,0, 1,0,1,0,1,0,1,0,0,1,
    0,0,1,0,1,1,0,0,0,1, 1,0,1,0,1,0,0,0,1,0,
    0,0,0,0,1,0,0,1,0,0, 1,1,1,0,1,1,0,0,1,1,
    0,1,0,0,0,1,1,1,0,0, 0,0,0,1,0,1,0,0,1,1,
    0,0,0,0,0,0,0,1,1,0, 1,0,1,1,0,0,0,1,1,0,
    0,0,
];

fn callsign_char_code(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b' ' => Some(36),
        b'A'..=b'Z' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn grid_char_code(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b' ' => Some(36),
        b'A'..=b'Z' => Some(c - b'A'),
        _ => None,
    }
}

/// Packs a standard callsign (1-2 letter prefix, one digit, 1-3 letter
/// suffix -- e.g. "K6BP", "W1AW", "KA9GRZ") into WSPR's 28-bit `n` field.
/// Callsigns with a digit as their 3rd character (e.g. "KA9GRZ", a
/// 2-letter prefix) are used as-is, right-padded with spaces to 6
/// characters; callsigns with the digit as their 2nd character (e.g.
/// "K6BP", "W1AW", a 1-letter prefix) get an implicit leading space --
/// both real, standard formats, not a simplification of one at the
/// expense of the other.
pub(crate) fn pack_call(callsign: &str) -> Option<u32> {
    let callsign = callsign.to_ascii_uppercase();
    let bytes = callsign.as_bytes();
    if bytes.len() > 6 || bytes.is_empty() {
        return None;
    }
    let mut call6 = [b' '; 6];
    if bytes.len() > 2 && bytes[2].is_ascii_digit() {
        call6[..bytes.len()].copy_from_slice(bytes);
    } else if bytes.len() > 1 && bytes[1].is_ascii_digit() {
        call6[1..1 + bytes.len()].copy_from_slice(bytes);
    } else {
        return None; // not a standard-format callsign this function handles
    }

    let codes: Vec<u32> = call6
        .iter()
        .map(|&c| callsign_char_code(c))
        .collect::<Option<Vec<u8>>>()?
        .into_iter()
        .map(|v| v as u32)
        .collect();
    let mut n = codes[0];
    n = n * 36 + codes[1];
    n = n * 10 + codes[2];
    n = n * 27 + codes[3].checked_sub(10)?;
    n = n * 27 + codes[4].checked_sub(10)?;
    n = n * 27 + codes[5].checked_sub(10)?;
    Some(n)
}

/// Packs a 4-character grid locator and a power level in dBm into
/// WSPR's 22-bit `m` field.
pub(crate) fn pack_grid4_power(grid4: &str, power_dbm: i32) -> Option<u32> {
    let bytes = grid4.to_ascii_uppercase();
    let bytes = bytes.as_bytes();
    if bytes.len() != 4 {
        return None;
    }
    let g: Vec<i64> = bytes
        .iter()
        .map(|&c| grid_char_code(c))
        .collect::<Option<Vec<u8>>>()?
        .into_iter()
        .map(|v| v as i64)
        .collect();
    let m = (179 - 10 * g[0] - g[2]) * 180 + 10 * g[1] + g[3];
    let m = m * 128 + power_dbm as i64 + 64;
    if !(0..(1 << 22)).contains(&m) {
        return None;
    }
    Some(m as u32)
}

/// Inverts `pack_call()`: given a real, decoded 28-bit `n` field,
/// recovers the original callsign string. Exact inverse by
/// construction (mixed-radix unpacking in reverse order, same base
/// values `pack_call()` used to pack), not a re-derivation -- see
/// `pack_call()`'s own doc comment for the two accepted callsign
/// layouts (digit at position 1 or 2) this also has to distinguish on
/// the way back out, which it does the same way `pack_call()` does:
/// by checking for the pad space `pack_call()` would have inserted at
/// position 0, not by re-guessing from the digit's own position.
pub(crate) fn unpack_call(n: u32) -> Option<String> {
    let mut n = n as i64;
    let c5 = (n % 27) as u32 + 10;
    n /= 27;
    let c4 = (n % 27) as u32 + 10;
    n /= 27;
    let c3 = (n % 27) as u32 + 10;
    n /= 27;
    let c2 = (n % 10) as u32;
    n /= 10;
    let c1 = (n % 36) as u32;
    n /= 36;
    let c0 = n as u32;
    if c0 > 36 {
        return None; // n was outside the range any real pack_call() output could produce.
    }
    let codes = [c0, c1, c2, c3, c4, c5];
    let mut chars = [0u8; 6];
    for (slot, &code) in chars.iter_mut().zip(codes.iter()) {
        *slot = match code {
            0..=9 => b'0' + code as u8,
            36 => b' ',
            10..=35 => b'A' + (code as u8 - 10),
            _ => return None,
        };
    }
    let call6 = std::str::from_utf8(&chars).ok()?;
    let trimmed = call6.trim_end();
    let call = trimmed.strip_prefix(' ').unwrap_or(trimmed);
    if call.is_empty() {
        return None;
    }
    Some(call.to_string())
}

/// Inverts `pack_grid4_power()`: given a real, decoded 22-bit `m`
/// field, recovers the original 4-character grid locator and power
/// level in dBm. Exact inverse by construction, same reasoning as
/// `unpack_call()` above. The grid field's own two letter/two digit
/// layout (not itself re-derivable from the packed value alone, since
/// `grid_char_code()` maps letters and digits to overlapping small
/// integers) is assumed here as a fixed, standard WSPR/Maidenhead
/// convention -- position 0/1 are always letters, 2/3 always digits --
/// the same real-world convention `pack_grid4_power()`'s own caller is
/// already expected to follow, not something decoded from the bits.
/// The grid-letter bounds here (`0..18`, Maidenhead fields A-R) are
/// deliberately tighter than `pack_grid4_power()`'s own `grid_char_
/// code()` allows on the way in (which accepts any A-Z, not just A-R)
/// -- this is the intended asymmetry, not a bug: `pack_grid4_power`
/// trusts a real caller to pass a standard grid square, `unpack_grid4_
/// power` has to actively reject values a real encode could never have
/// produced, since it's the fail-fast boundary for a possibly-wrong
/// decode (`ZZ99`-shaped input packs fine; its own packed output
/// correctly fails to unpack).
pub(crate) fn unpack_grid4_power(m: u32) -> Option<(String, i32)> {
    let m = m as i64;
    let power_dbm = (m % 128) - 64;
    // WSPR's real, documented power range is 0-60 dBm -- rejecting
    // anything outside it is real signal, not an inferred guess: a
    // wrong-but-confident decode (see wspr_sync.rs's own WsprMessage
    // Error::UnpackFailed) most often shows up as exactly this kind of
    // syntactically-valid-looking but out-of-range power level, since a
    // garbage 22-bit m still produces SOME grid/power pair -- this
    // check is the real gate that catches it, not a formality.
    if !(0..=60).contains(&power_dbm) {
        return None;
    }
    let gv = m / 128;
    let sum = 179 - (gv / 180);
    let g1g3 = gv % 180;
    let g0 = sum / 10;
    let g2 = sum % 10;
    let g1 = g1g3 / 10;
    let g3 = g1g3 % 10;
    if !(0..18).contains(&g0)
        || !(0..18).contains(&g1)
        || !(0..10).contains(&g2)
        || !(0..10).contains(&g3)
    {
        return None; // m was outside the range any real pack_grid4_power() output could produce.
    }
    let grid = format!(
        "{}{}{}{}",
        (b'A' + g0 as u8) as char,
        (b'A' + g1 as u8) as char,
        g2,
        g3
    );
    Some((grid, power_dbm as i32))
}

/// Combines `unpack_call()`/`unpack_grid4_power()` with the real
/// 50-bit-payload layout `wspr_encode_symbols()`'s own `data[]`
/// construction defines (`n`'s 28 bits first, then `m`'s 22 bits,
/// both MSB-first) to turn a decoded bitset (as `wspr_decode.rs`'s
/// `sequential_decode_with_confidence_gate()` returns it -- bit `i` =
/// trellis depth `i`, matching `data[]`'s own MSB-first bit-consumption
/// order) back into the original `(callsign, grid4, power_dbm)`. The
/// real entry point step 3's sync search wires into (see `wspr_sync.
/// rs`'s own doc comment for the full decode path this is the last
/// step of).
pub fn unpack_wspr_message(decoded_bits: u128) -> Option<(String, String, i32)> {
    let mut n: u32 = 0;
    for i in 0..28 {
        let bit = ((decoded_bits >> i) & 1) as u32;
        n = (n << 1) | bit;
    }
    let mut m: u32 = 0;
    for i in 28..50 {
        let bit = ((decoded_bits >> i) & 1) as u32;
        m = (m << 1) | bit;
    }
    let call = unpack_call(n)?;
    let (grid4, power_dbm) = unpack_grid4_power(m)?;
    Some((call, grid4, power_dbm))
}

/// Bit-reversal permutation of the low 8 bits of each index 0..256,
/// keeping only results < 162, in the order they're produced -- WSPR's
/// standard interleaving scheme (an 8-bit bit-reversal permutation,
/// documented independently of any one implementation; `u8::reverse_bits`
/// is Rust's own standard-library primitive for it, not a
/// reimplementation of any specific reference's bit-twiddling).
pub(crate) fn interleave_permutation() -> [usize; WSPR_NUM_SYMBOLS] {
    let mut perm = [0usize; WSPR_NUM_SYMBOLS];
    let mut p = 0;
    for i in 0u32..256 {
        let j = (i as u8).reverse_bits() as usize;
        if j < WSPR_NUM_SYMBOLS {
            perm[p] = j;
            p += 1;
        }
    }
    debug_assert_eq!(p, WSPR_NUM_SYMBOLS);
    perm
}

pub(crate) fn parity(x: u32) -> u8 {
    (x.count_ones() & 1) as u8
}

/// K=32, rate-1/2 convolutional encode of an 11-byte (88-bit) message
/// (the 50 real payload bits plus 31 zero tail bits already packed in,
/// matching WSPR's own fixed frame size), producing 176 output bits
/// (2 per input bit): the first from POLY1, the second from POLY2.
pub(crate) fn convolutional_encode(data: &[u8; 11]) -> [u8; 176] {
    let mut out = [0u8; 176];
    let mut state: u32 = 0;
    let mut out_idx = 0;
    for &byte in data {
        for i in (0..8).rev() {
            let bit = (byte >> i) & 1;
            state = (state << 1) | (bit as u32);
            out[out_idx] = parity(state & POLY1);
            out[out_idx + 1] = parity(state & POLY2);
            out_idx += 2;
        }
    }
    out
}

/// Encodes a standard Type 1 WSPR message ("CALLSIGN GRID4 POWER_DBM")
/// into the 162 four-ary channel symbols (values 0-3) WSPR transmits.
/// Returns None for a callsign/grid this function's own documented
/// scope doesn't cover (Type 2/3 messages, malformed input) rather than
/// guessing at a result.
pub fn wspr_encode_symbols(
    callsign: &str,
    grid4: &str,
    power_dbm: i32,
) -> Option<[u8; WSPR_NUM_SYMBOLS]> {
    let n = pack_call(callsign)?;
    let m = pack_grid4_power(grid4, power_dbm)?;

    let mut data = [0u8; 11];
    data[0] = ((n >> 20) & 0xFF) as u8;
    data[1] = ((n >> 12) & 0xFF) as u8;
    data[2] = ((n >> 4) & 0xFF) as u8;
    data[3] = (((n & 0x0F) << 4) + ((m >> 18) & 0x0F)) as u8;
    data[4] = ((m >> 10) & 0xFF) as u8;
    data[5] = ((m >> 2) & 0xFF) as u8;
    data[6] = ((m & 0x03) << 6) as u8;
    // data[7..11] stay 0 -- the 31 convolutional-encoder tail bits.

    let channel_bits = convolutional_encode(&data);
    let perm = interleave_permutation();

    let mut interleaved = [0u8; WSPR_NUM_SYMBOLS];
    for (p, &target) in perm.iter().enumerate() {
        interleaved[target] = channel_bits[p];
    }

    let mut symbols = [0u8; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        symbols[i] = 2 * interleaved[i] + SYNC_VECTOR[i];
    }
    Some(symbols)
}

/// Synthesizes the continuous-phase 4-FSK audio (real i16 PCM, mono)
/// WSPR transmits for a given symbol sequence: 4 tones spaced
/// `WSPR_TONE_SPACING_HZ` (~1.4648 Hz) apart starting at `base_hz`, each
/// held for one symbol period (~682.7 ms), with the carrier phase
/// integrated continuously across symbol boundaries (no phase reset --
/// what "continuous-phase" FSK means, and what keeps WSPR's own
/// emission narrow and clean rather than clicking at every tone change).
pub fn wspr_modulate(symbols: &[u8], base_hz: f64, sample_rate: u32) -> Vec<i16> {
    let samples_per_symbol = (sample_rate as f64 / WSPR_SYMBOL_RATE_HZ).round() as usize;
    let mut out = Vec::with_capacity(symbols.len() * samples_per_symbol);
    let mut phase = 0.0f64;
    for &sym in symbols {
        let freq = base_hz + (sym as f64) * WSPR_TONE_SPACING_HZ;
        let phase_inc = 2.0 * std::f64::consts::PI * freq / sample_rate as f64;
        for _ in 0..samples_per_symbol {
            let sample = phase.cos();
            out.push(
                (sample * i16::MAX as f64)
                    .round()
                    .clamp(i16::MIN as f64, i16::MAX as f64) as i16,
            );
            phase += phase_inc;
            if phase > 2.0 * std::f64::consts::PI {
                phase -= 2.0 * std::f64::consts::PI;
            }
        }
    }
    out
}

/// High-level: message fields in, PCM audio out. `None` if the message
/// is outside this function's documented Type-1-only scope.
pub fn wspr_encode_audio(
    callsign: &str,
    grid4: &str,
    power_dbm: i32,
    base_hz: f64,
    sample_rate: u32,
) -> Option<Vec<i16>> {
    let symbols = wspr_encode_symbols(callsign, grid4, power_dbm)?;
    Some(wspr_modulate(&symbols, base_hz, sample_rate))
}

/// Wraps mono 16-bit PCM samples in a standard 44-byte-header WAV file
/// (RIFF/WAVE, uncompressed PCM, one "fmt " and one "data" chunk, no
/// extension fields) -- the same minimal shape
/// `digital_decoder.rs`'s own `read_wav_mono_i16()` test helper already
/// expects (`data` chunk literally at byte offset 36).
fn wrap_mono_i16_as_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_bytes = samples.len() * 2;
    let byte_rate = sample_rate * 2; // mono, 16-bit: 1 channel * 2 bytes/sample
    let mut out = Vec::with_capacity(44 + data_bytes);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat = 1 (PCM)
    out.extend_from_slice(&1u16.to_le_bytes()); // NumChannels = 1 (mono)
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // BlockAlign = channels * bytes/sample
    out.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Real, tested, verified-ground-truth `.wav` fixture generation for
/// WSPR decode work -- resolves `WSPR_DECODE_IMPLEMENTATION_PLAN.md`
/// step 5's "test against real, known WSPR audio captures ... confirm
/// decoded messages match the known answer" the same way this
/// codebase's own FT8 tests already do (`digital_decoder.rs`'s
/// `ft8_decode_fires_at_a_real_utc_slot_boundary` calls `ft8sim`
/// on demand rather than shipping a static binary fixture in git):
/// generated fresh, in-process, from `wspr_encode_symbols()`/
/// `wspr_modulate()`, which are themselves already verified bit-for-bit
/// against the real K1JT/K9AN reference encoder (`wsprsim`, see this
/// module's own `matches_the_real_k1jt_reference_encoder_end_to_end`
/// test) -- not a new, independently-trusted encode path. `wsprsim`
/// itself (confirmed directly: `wsprsim --help`, and `-o` writing a
/// tested run) only ever writes a `.c2` complex-baseband file, never a
/// `.wav`, so there is no way to get a WSJT-X-reference-tool-generated
/// `.wav` directly the way `ft8sim` provides for FT8 -- this is a real,
/// documented gap in WSJT-X's own tooling, not a shortcut taken here.
///
/// Deliberately clean (no injected noise, no channel simulation): this
/// proves a future decoder's basic protocol correctness (sync
/// detection, symbol timing, bit unpacking) against a known-exact
/// signal -- it is NOT a substitute for real low-SNR robustness
/// testing, which the decode implementation itself will still need
/// (e.g. against `wsprsim -s <snr> -o *.c2`-generated captures, or real
/// WSJT-X sample recordings) once that multi-day DSP project is
/// actually underway. Returns `None` for the same out-of-scope
/// messages `wspr_encode_audio()` already refuses.
pub fn wspr_encode_wav_bytes(
    callsign: &str,
    grid4: &str,
    power_dbm: i32,
    base_hz: f64,
    sample_rate: u32,
) -> Option<Vec<u8>> {
    let samples = wspr_encode_audio(callsign, grid4, power_dbm, base_hz, sample_rate)?;
    Some(wrap_mono_i16_as_wav(&samples, sample_rate))
}

/// A small, deterministic (seeded) PRNG -- splitmix64 -- used only to
/// generate reproducible AWGN test fixtures below. Not cryptographic and
/// not used anywhere outside test/fixture generation; deliberately
/// self-contained rather than pulling in the `rand` crate for this one
/// narrow, test-only need (this crate otherwise has zero runtime
/// dependencies beyond `libm`).
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform f64 in (0, 1] -- excludes 0 so Box-Muller's ln() below
    /// never sees it (ln(0) is -inf, which would poison the result).
    fn next_open01(&mut self) -> f64 {
        let bits = self.next_u64() >> 11; // top 53 bits: full f64 mantissa precision
        ((bits as f64) + 1.0) / ((1u64 << 53) as f64 + 1.0)
    }

    /// Standard normal (mean 0, stddev 1) via the Box-Muller transform.
    fn next_gaussian(&mut self) -> f64 {
        let u1 = self.next_open01();
        let u2 = self.next_open01();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

/// Adds white Gaussian noise to `samples` at the given target SNR (dB),
/// measured as 10*log10(mean-square signal power / mean-square noise
/// power) over this signal's own full sample-rate bandwidth -- NOT
/// WSJT-X's own 2500 Hz-reference-bandwidth SNR convention. `wsprd`'s
/// own printed SNR column uses that different convention and will
/// therefore report a different, not-directly-comparable number for the
/// same nominal `snr_db` passed in here; treat `wsprd`'s own output as
/// the real, empirical ground truth for what a given fixture actually
/// decodes as, not this function's input parameter (see
/// `WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s own recorded noise-ladder
/// results for the real correspondence). Deterministic given `seed` --
/// two calls with the same seed produce byte-identical noise, so a
/// fixture (and any regression that depends on it) is reproducible.
pub fn add_awgn(samples: &[i16], snr_db: f64, seed: u64) -> Vec<i16> {
    let signal_power: f64 =
        samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len() as f64;
    let noise_power = signal_power / 10f64.powf(snr_db / 10.0);
    let noise_stddev = noise_power.sqrt();

    let mut rng = SplitMix64(seed);
    samples
        .iter()
        .map(|&s| {
            let noisy = s as f64 + rng.next_gaussian() * noise_stddev;
            noisy.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16
        })
        .collect()
}

/// Same as `wspr_encode_wav_bytes()`, but with AWGN injected at `snr_db`
/// (see `add_awgn()`'s own doc comment for the exact power-ratio
/// definition and why it won't numerically match `wsprd`'s own printed
/// SNR column). Exists specifically to build the noise ladder
/// `WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s build order calls for: real,
/// varied-SNR fixtures with recorded `wsprd` reference decodes, needed
/// before either the Fano decoder's own soft-decision metric table or
/// the sync search can be built or validated against anything real --
/// `wspr_encode_wav_bytes()`'s own clean output alone proves nothing
/// about low-SNR robustness, which is the actual hard part of WSPR decode.
pub fn wspr_encode_wav_bytes_with_noise(
    callsign: &str,
    grid4: &str,
    power_dbm: i32,
    base_hz: f64,
    sample_rate: u32,
    snr_db: f64,
    seed: u64,
) -> Option<Vec<u8>> {
    let clean = wspr_encode_audio(callsign, grid4, power_dbm, base_hz, sample_rate)?;
    let noisy = add_awgn(&clean, snr_db, seed);
    Some(wrap_mono_i16_as_wav(&noisy, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_vector_has_the_documented_symbol_count() {
        assert_eq!(SYNC_VECTOR.len(), WSPR_NUM_SYMBOLS);
        assert!(SYNC_VECTOR.iter().all(|&b| b == 0 || b == 1));
    }

    #[test]
    fn pack_call_matches_independently_computed_expected_values() {
        // "K9AN EN50 33" is the exact worked example used in the
        // reference implementation's own type-detection comment
        // ("Type 1 message: K9AN EN50 33"). Expected values below were
        // computed independently in Python from this module's own
        // documented formula (a from-scratch second implementation,
        // not read back from this file), catching arithmetic bugs in
        // the Rust port that a range check alone wouldn't.
        assert_eq!(pack_call("K9AN"), Some(259205804));
        assert_eq!(pack_call("K6BP"), Some(259147538));
        assert_eq!(pack_grid4_power("EN50", 33), Some(3104097));
        assert_eq!(pack_grid4_power("CM87", 30), Some(3495390));
    }

    #[test]
    fn unpack_call_and_unpack_grid4_power_recover_the_real_reference_verified_values() {
        // Not just a self-consistent pack/unpack round trip -- inverts
        // the exact reference-implementation-verified integers the test
        // above already pins (259205804/259147538/3104097/3495390),
        // so a bug that happened to cancel out in a naive round trip
        // (e.g. an inverse that's wrong in a way that still composes to
        // the identity) can't hide here.
        assert_eq!(unpack_call(259205804), Some("K9AN".to_string()));
        assert_eq!(unpack_call(259147538), Some("K6BP".to_string()));
        assert_eq!(unpack_grid4_power(3104097), Some(("EN50".to_string(), 33)));
        assert_eq!(unpack_grid4_power(3495390), Some(("CM87".to_string(), 30)));
    }

    #[test]
    fn pack_and_unpack_round_trip_for_both_callsign_layouts_and_several_grids() {
        for (call, grid, power) in [
            ("K6BP", "CM87", 30), // 1-letter prefix -- pack_call's implicit-leading-space layout.
            ("W1AW", "FN31", 37), // 1-letter prefix.
            ("K9AN", "EN50", 33), // 1-letter prefix.
            ("KA9GRZ", "DM79", 20), // 2-letter prefix, digit at position 2 -- pack_call's other layout.
        ] {
            let n = pack_call(call).unwrap();
            let m = pack_grid4_power(grid, power).unwrap();
            assert_eq!(
                unpack_call(n).as_deref(),
                Some(call),
                "callsign round trip for {call}"
            );
            assert_eq!(
                unpack_grid4_power(m),
                Some((grid.to_string(), power)),
                "grid/power round trip for {grid} {power}"
            );
        }
    }

    #[test]
    fn unpack_wspr_message_recovers_the_original_message_from_a_real_encoded_bitset() {
        // Builds the exact bitset sequential_decode_with_confidence_
        // gate() would hand unpack_wspr_message() for a real, correctly
        // decoded "K6BP CM87 30" -- bit i = trellis depth i, matching
        // wspr_encode_symbols()'s own data[] MSB-first bit-consumption
        // order (same convention wspr_decode.rs's own test helper of a
        // similar name uses).
        let n = pack_call("K6BP").unwrap();
        let m = pack_grid4_power("CM87", 30).unwrap();
        let mut data = [0u8; 11];
        data[0] = ((n >> 20) & 0xFF) as u8;
        data[1] = ((n >> 12) & 0xFF) as u8;
        data[2] = ((n >> 4) & 0xFF) as u8;
        data[3] = (((n & 0x0F) << 4) + ((m >> 18) & 0x0F)) as u8;
        data[4] = ((m >> 10) & 0xFF) as u8;
        data[5] = ((m >> 2) & 0xFF) as u8;
        data[6] = ((m & 0x03) << 6) as u8;

        let mut decoded_bits: u128 = 0;
        for i in 0..50 {
            let byte = data[i / 8];
            let bit = (byte >> (7 - (i % 8))) & 1;
            if bit == 1 {
                decoded_bits |= 1u128 << i;
            }
        }

        assert_eq!(
            unpack_wspr_message(decoded_bits),
            Some(("K6BP".to_string(), "CM87".to_string(), 30))
        );
    }

    #[test]
    fn matches_the_real_k1jt_reference_encoder_end_to_end() {
        // Not an independently-computed cross-check like the test above
        // -- this is the actual reference implementation itself.
        // `wsjtx` (2.7.0, Debian's real package, installed for this
        // verification) ships `wsprsim`, the real K1JT/K9AN tool this
        // module's own doc comment already identified as the origin of
        // every WSPR decoder. Run for real against the exact worked
        // example this module already used ("K9AN EN50 33"):
        //   wsprsim -cd "K9AN EN50 33"
        // printed both the packed data and the 162 channel symbols. The
        // packed-data bytes (`F7 32 AA CB D7 58 40 00 00 00 00`) decode
        // to the identical 28-bit call and 22-bit grid/power integers
        // the test above already asserts (259205804, 3104097) -- this
        // test instead pins the full 162-symbol channel sequence
        // wsprsim printed, which additionally exercises the
        // convolutional encoder, interleaving and sync-vector merge
        // this module's own from-scratch implementation performs after
        // packing, none of which the packed-data comparison alone
        // touches.
        let reference_symbols: [u8; WSPR_NUM_SYMBOLS] = [
            3, 3, 2, 0, 0, 2, 0, 2, 1, 0, 0, 0, 1, 3, 1, 0, 2, 2, 3, 0, 2, 3, 0, 1, 1, 1, 1, 2, 0,
            2, 0, 2, 0, 0, 3, 2, 2, 3, 0, 1, 2, 2, 0, 2, 2, 2, 1, 0, 1, 1, 2, 2, 1, 3, 0, 3, 0, 0,
            0, 1, 1, 0, 3, 2, 2, 0, 2, 3, 3, 2, 1, 0, 3, 2, 1, 0, 3, 0, 0, 3, 2, 0, 3, 0, 1, 3, 0,
            0, 0, 3, 1, 2, 1, 2, 1, 2, 2, 0, 1, 2, 0, 2, 0, 0, 1, 0, 2, 1, 0, 2, 3, 3, 1, 2, 3, 3,
            2, 2, 1, 1, 0, 1, 2, 0, 0, 1, 3, 3, 2, 2, 0, 0, 2, 3, 2, 1, 2, 0, 3, 3, 0, 2, 2, 0, 2,
            2, 0, 3, 1, 0, 3, 2, 3, 1, 2, 2, 0, 3, 1, 2, 2, 2,
        ];
        let symbols = wspr_encode_symbols("K9AN", "EN50", 33).expect("K9AN/EN50/33 must encode");
        assert_eq!(
            symbols, reference_symbols,
            "this module's channel symbols must match the real wsprsim reference output bit-for-bit"
        );
    }

    #[test]
    fn pack_call_handles_both_one_and_two_letter_prefixes() {
        assert!(pack_call("K6BP").is_some()); // digit at index 1
        assert!(pack_call("W1AW").is_some()); // digit at index 1
        assert!(pack_call("KA9GRZ").is_some()); // digit at index 2
        assert!(pack_call("").is_none());
        assert!(pack_call("TOOLONGCALL").is_none());
    }

    #[test]
    fn pack_grid4_power_stays_within_its_documented_22_bit_field() {
        let m = pack_grid4_power("EN50", 33).expect("EN50/33 should pack");
        assert!(m < (1u32 << 22));
        assert!(pack_grid4_power("XY", 33).is_none()); // wrong length
    }

    #[test]
    fn interleave_permutation_is_a_true_bijection_over_162_symbols() {
        let perm = interleave_permutation();
        let mut seen = [false; WSPR_NUM_SYMBOLS];
        for &target in perm.iter() {
            assert!(target < WSPR_NUM_SYMBOLS);
            assert!(
                !seen[target],
                "index {} produced twice by the interleaver",
                target
            );
            seen[target] = true;
        }
        assert!(
            seen.iter().all(|&s| s),
            "interleaver did not cover all 162 symbol slots"
        );
    }

    #[test]
    fn convolutional_encoder_is_deterministic_and_all_zero_tail_still_moves_state() {
        // A basic sanity/regression check: the same input always
        // produces the same output (no hidden global state), and the
        // encoder state actually changes as data bits are consumed (a
        // constant-output encoder would indicate POLY1/POLY2 got
        // shifted or zeroed by mistake).
        let data = [0xFFu8, 0x00, 0xAA, 0x55, 0x01, 0x02, 0x04, 0, 0, 0, 0];
        let out1 = convolutional_encode(&data);
        let out2 = convolutional_encode(&data);
        assert_eq!(out1, out2);
        assert!(
            out1.contains(&1),
            "encoder output is all zero -- polynomials likely wrong"
        );
        assert!(
            out1.contains(&0),
            "encoder output is all one -- polynomials likely wrong"
        );
    }

    #[test]
    fn encode_symbols_produces_valid_162_symbol_sequence_for_a_real_message() {
        let symbols =
            wspr_encode_symbols("K6BP", "CM87", 30).expect("valid Type 1 message should encode");
        assert_eq!(symbols.len(), WSPR_NUM_SYMBOLS);
        assert!(
            symbols.iter().all(|&s| s <= 3),
            "every WSPR symbol must be a 2-bit (0-3) tone index"
        );
        // The sync bit is always embedded in the symbol's LSB (symbol =
        // 2*data_bit + sync_bit) -- confirm that relationship holds for
        // every symbol against the known sync vector, not just that
        // values are in range.
        for i in 0..WSPR_NUM_SYMBOLS {
            assert_eq!(
                symbols[i] & 1,
                SYNC_VECTOR[i],
                "symbol {} lost its sync bit",
                i
            );
        }
    }

    #[test]
    fn out_of_scope_message_types_return_none_not_a_guess() {
        // A compound/prefixed callsign this module's documented scope
        // doesn't attempt (e.g. "PJ4/K1ABC") -- pack_call's own digit-
        // position heuristic can't classify it as a standard callsign,
        // and it correctly refuses rather than silently mis-packing it.
        assert!(pack_call("PJ4/K1ABC").is_none());
    }

    /// Mirrors `digital_decoder.rs`'s own `read_wav_mono_i16()` test
    /// helper exactly (same 44-byte-header, `data` chunk at offset 36
    /// assumption) so this test proves compatibility with the real
    /// consumer, not just with itself.
    fn read_wav_mono_i16(bytes: &[u8]) -> Vec<i16> {
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(
            &bytes[36..40],
            b"data",
            "expected a standard 44-byte-header PCM WAV"
        );
        bytes[44..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    #[test]
    fn wav_fixture_round_trips_to_the_exact_same_pcm_samples_modulate_produced() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let sample_rate = 12000u32;
        let expected_samples = wspr_modulate(&symbols, 1500.0, sample_rate);

        let wav_bytes = wspr_encode_wav_bytes("K6BP", "CM87", 30, 1500.0, sample_rate)
            .expect("K6BP/CM87/30 is a valid Type 1 message");
        let parsed_samples = read_wav_mono_i16(&wav_bytes);

        assert_eq!(
            parsed_samples, expected_samples,
            "round-tripping through the WAV header must not alter a single PCM sample"
        );
    }

    #[test]
    fn wav_fixture_header_matches_the_documented_44_byte_mono_pcm_shape() {
        let sample_rate = 12000u32;
        let wav_bytes = wspr_encode_wav_bytes("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();

        assert_eq!(&wav_bytes[0..4], b"RIFF");
        assert_eq!(&wav_bytes[8..12], b"WAVE");
        assert_eq!(&wav_bytes[12..16], b"fmt ");
        assert_eq!(
            u16::from_le_bytes([wav_bytes[20], wav_bytes[21]]),
            1,
            "AudioFormat must be 1 (PCM)"
        );
        assert_eq!(
            u16::from_le_bytes([wav_bytes[22], wav_bytes[23]]),
            1,
            "NumChannels must be 1 (mono)"
        );
        assert_eq!(
            u32::from_le_bytes([wav_bytes[24], wav_bytes[25], wav_bytes[26], wav_bytes[27]]),
            sample_rate
        );
        assert_eq!(
            u16::from_le_bytes([wav_bytes[34], wav_bytes[35]]),
            16,
            "BitsPerSample must be 16"
        );
        assert_eq!(&wav_bytes[36..40], b"data");

        let declared_data_size =
            u32::from_le_bytes([wav_bytes[40], wav_bytes[41], wav_bytes[42], wav_bytes[43]])
                as usize;
        assert_eq!(
            declared_data_size,
            wav_bytes.len() - 44,
            "declared data chunk size must match the actual payload length"
        );

        let declared_riff_size =
            u32::from_le_bytes([wav_bytes[4], wav_bytes[5], wav_bytes[6], wav_bytes[7]]) as usize;
        assert_eq!(
            declared_riff_size,
            wav_bytes.len() - 8,
            "declared RIFF chunk size must match (file length - 8)"
        );
    }

    #[test]
    fn wav_fixture_returns_none_for_the_same_out_of_scope_messages_as_the_underlying_encoder() {
        assert!(wspr_encode_wav_bytes("PJ4/K1ABC", "EN50", 33, 1500.0, 12000).is_none());
    }

    #[test]
    fn add_awgn_is_deterministic_given_the_same_seed() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let clean = wspr_modulate(&symbols, 1500.0, 12000);
        let noisy1 = add_awgn(&clean, -20.0, 42);
        let noisy2 = add_awgn(&clean, -20.0, 42);
        assert_eq!(
            noisy1, noisy2,
            "same seed must produce byte-identical noise for a reproducible fixture"
        );
    }

    #[test]
    fn add_awgn_different_seeds_produce_different_noise() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let clean = wspr_modulate(&symbols, 1500.0, 12000);
        let noisy_a = add_awgn(&clean, -20.0, 1);
        let noisy_b = add_awgn(&clean, -20.0, 2);
        assert_ne!(
            noisy_a, noisy_b,
            "different seeds must not coincidentally produce identical noise"
        );
    }

    #[test]
    fn add_awgn_actually_adds_more_measured_noise_as_the_target_snr_drops() {
        // Guards against the easiest bug in this function to write and
        // never notice: a sign or log-scale mistake in the dB->power
        // conversion that makes "lower SNR" produce LESS noise instead of
        // more. Measures real deviation from the clean signal at two very
        // different SNR targets and asserts the direction, not an exact
        // value (the exact noise realization is seed-dependent by design).
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let clean = wspr_modulate(&symbols, 1500.0, 12000);

        let mean_abs_deviation = |noisy: &[i16]| -> f64 {
            clean
                .iter()
                .zip(noisy.iter())
                .map(|(&c, &n)| (c as f64 - n as f64).abs())
                .sum::<f64>()
                / clean.len() as f64
        };

        let quiet_noise = add_awgn(&clean, 40.0, 7); // high SNR -- barely any noise
        let loud_noise = add_awgn(&clean, -30.0, 7); // low SNR -- dominated by noise

        assert!(
            mean_abs_deviation(&loud_noise) > mean_abs_deviation(&quiet_noise) * 10.0,
            "a -30dB target must add dramatically more measured noise than a +40dB target"
        );
    }

    #[test]
    fn wav_fixture_with_noise_round_trips_and_returns_none_for_out_of_scope_messages() {
        let sample_rate = 12000u32;
        let wav_bytes =
            wspr_encode_wav_bytes_with_noise("K6BP", "CM87", 30, 1500.0, sample_rate, -20.0, 99)
                .expect("K6BP/CM87/30 is a valid Type 1 message");
        // Same 44-byte header shape as the clean variant -- only the PCM
        // payload differs (noisy, not byte-identical to the clean
        // encoder's own output), so the existing header-shape assertions
        // already cover format correctness; just confirm it parses and
        // has the right sample count.
        assert_eq!(&wav_bytes[0..4], b"RIFF");
        let declared_data_size =
            u32::from_le_bytes([wav_bytes[40], wav_bytes[41], wav_bytes[42], wav_bytes[43]])
                as usize;
        assert_eq!(declared_data_size, wav_bytes.len() - 44);

        assert!(wspr_encode_wav_bytes_with_noise(
            "PJ4/K1ABC",
            "EN50",
            33,
            1500.0,
            sample_rate,
            -20.0,
            99
        )
        .is_none());
    }

    #[test]
    fn modulate_produces_the_documented_audio_length_and_stays_in_range() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let sample_rate = 12000u32;
        let audio = wspr_modulate(&symbols, 1500.0, sample_rate);
        let expected_samples =
            WSPR_NUM_SYMBOLS * (sample_rate as f64 / WSPR_SYMBOL_RATE_HZ).round() as usize;
        assert_eq!(audio.len(), expected_samples);
        // ~110.6s at 12kHz is the documented WSPR transmission length.
        let duration_s = audio.len() as f64 / sample_rate as f64;
        assert!(
            (duration_s - 110.6).abs() < 0.5,
            "duration {} s is not close to the documented ~110.6s",
            duration_s
        );
    }
}

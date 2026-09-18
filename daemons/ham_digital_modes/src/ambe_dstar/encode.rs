//! Assembles a transmittable 72-bit D-STAR AMBE frame from `decode::RawParameters` -- the exact
//! inverse of `decode::extract_raw_parameters` (scattering `b0..b8` back into the 49-bit `d[]`
//! layout) followed by `decode::parse_frame`'s own real FEC/whitening steps run forward (Golay-
//! encode `C0`'s data, whiten and Golay-encode `C1`'s data, concatenate with `C2`/`C3` raw).

use super::decode::RawParameters;
use crate::ambe::fec::golay_encode;

/// Scatters `b0..b8` back into the 49-bit `d[]` layout `decode::extract_raw_parameters` reads from
/// -- each assignment here is the direct algebraic inverse of that function's own extraction, over
/// the identical bit ranges (see `mod.rs`'s own doc-comment table).
// [@ANCHOR: pack_raw_parameters]
pub fn pack_raw_parameters(raw: &RawParameters) -> u64 {
    let mut d: u64 = 0;
    let mut set = |msb_index: usize, width: usize, value: u32| {
        let shift = 49 - msb_index - width;
        let mask = ((1u64 << width) - 1) << shift;
        d = (d & !mask) | (((value as u64) << shift) & mask);
    };

    set(0, 6, raw.b0 >> 1);
    set(48, 1, raw.b0 & 1);
    set(38, 4, raw.b1);
    set(6, 4, raw.b2 >> 2);
    set(42, 2, raw.b2 & 0b11);
    set(10, 2, raw.b3 >> 7);
    set(12, 5, (raw.b3 >> 2) & 0b1_1111);
    set(44, 2, raw.b3 & 0b11);
    set(17, 5, raw.b4 >> 2);
    set(46, 2, raw.b4 & 0b11);
    set(22, 2, raw.b5 >> 2);
    set(25, 2, raw.b5 & 0b11);
    set(27, 4, raw.b6);
    set(31, 4, raw.b7);
    set(35, 3, raw.b8 >> 1);
    // d[24] (C2's own first bit) is never read by any known decoder, per mod.rs's own doc comment
    // -- left at 0 here since it carries no real information to set.

    d
}

/// Builds the full transmittable 72-bit frame (packed MSB-first into the low 72 bits of the return
/// value, matching `decode::parse_frame`'s own input convention) from the 49-bit `d[]` layout:
/// Golay-encodes `C0`'s 12 data bits (with a `0` spare bit prepended), whitens and Golay-encodes
/// `C1`'s 12 data bits using `C0`'s own data as the whitening seed, and carries `C2`/`C3` raw.
pub fn build_frame(d: u64) -> u128 {
    let c0_data = ((d >> 37) & 0xFFF) as u16;
    let c1_data = ((d >> 25) & 0xFFF) as u16;
    let c2 = ((d >> 14) & 0x7FF) as u32;
    let c3 = (d & 0x3FFF) as u32;

    let c0_codeword = golay_encode(c0_data); // 23 bits, spare bit (0) is implicit as the 24th
    let c1_codeword = golay_encode(c1_data);
    let c1_whitened = super::whiten_c1(c1_codeword, c0_data);

    ((c0_codeword as u128) << 48)
        | ((c1_whitened as u128) << 25)
        | ((c2 as u128) << 14)
        | (c3 as u128)
}

/// Packs a raw byte buffer (9 bytes, MSB-first, matching the real over-the-wire/serial convention)
/// from a 72-bit frame value.
pub fn frame_to_bytes(frame: u128) -> [u8; 9] {
    let mut out = [0u8; 9];
    // A 72-bit value spread across 9 bytes MSB-first: byte 0 covers bits 71..64 (shift 64), byte 8
    // covers bits 7..0 (shift 0) -- `i` never exceeds 8 here, so `64 - i*8` never underflows.
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = 64 - i * 8;
        *slot = ((frame >> shift) & 0xFF) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe_dstar::decode::{extract_raw_parameters, parse_frame};

    /// The real, end-to-end round trip this whole module exists for: pack a set of raw parameters
    /// into a transmittable frame, parse that frame back, and confirm both the FEC layer reports
    /// zero corrected errors (a real, meaningful signal for D-STAR's non-perfect Golay(23,12,7) code
    /// -- unlike the P25 investigation's own perfect-code caveat) and the extracted parameters
    /// exactly match the originals.
    #[test]
    fn pack_and_parse_round_trips_a_full_set_of_raw_parameters() {
        let original = RawParameters {
            b0: 0b101_0110,    // 7 bits
            b1: 0b1010,        // 4 bits
            b2: 0b10_1101,     // 6 bits
            b3: 0b1_0110_1101, // 9 bits
            b4: 0b101_1010,    // 7 bits
            b5: 0b1101,        // 4 bits
            b6: 0b0110,        // 4 bits
            b7: 0b1001,        // 4 bits
            b8: 0b1010,        // 4 bits, low bit already 0 per this field's own real convention
        };

        let d = pack_raw_parameters(&original);
        let frame = build_frame(d);
        let parsed = parse_frame(frame);

        assert_eq!(
            parsed.epsilon_c0, 0,
            "a cleanly built C0 must decode with zero errors"
        );
        assert_eq!(
            parsed.epsilon_c1, 0,
            "a cleanly built, correctly whitened C1 must decode with zero errors"
        );

        let recovered = extract_raw_parameters(parsed.d);
        assert_eq!(recovered.b0, original.b0, "b0");
        assert_eq!(recovered.b1, original.b1, "b1");
        assert_eq!(recovered.b2, original.b2, "b2");
        assert_eq!(recovered.b3, original.b3, "b3");
        assert_eq!(recovered.b4, original.b4, "b4");
        assert_eq!(recovered.b5, original.b5, "b5");
        assert_eq!(recovered.b6, original.b6, "b6");
        assert_eq!(recovered.b7, original.b7, "b7");
        assert_eq!(recovered.b8, original.b8, "b8");
    }

    /// A real structural check on `frame_to_bytes`: the 9 bytes, read back MSB-first, must
    /// reconstruct exactly the original 72-bit frame value -- catches an off-by-one in the shift
    /// arithmetic that a single hand-picked example alone might not exercise at every byte boundary.
    #[test]
    fn frame_to_bytes_round_trips_via_msb_first_reconstruction() {
        let frame: u128 = 0x1234_5678_9ABC_DEF0_12u128 & ((1u128 << 72) - 1);
        let bytes = frame_to_bytes(frame);
        let mut reconstructed: u128 = 0;
        for &b in &bytes {
            reconstructed = (reconstructed << 8) | b as u128;
        }
        assert_eq!(reconstructed, frame);
    }
}

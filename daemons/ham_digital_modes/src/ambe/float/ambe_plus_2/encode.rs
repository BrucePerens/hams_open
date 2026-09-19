//! Assembles a transmittable 72-bit AMBE+2 half-rate frame from `decode::RawParameters` -- the
//! exact inverse of `decode::extract_raw_parameters` (scattering `b0..b8` back into the 49-bit
//! `d[]` layout) followed by `super::build_frame` (reused directly from `ambe_dstar::encode`,
//! since the FEC/whitening frame layer is bit-for-bit identical -- see `mod.rs`'s own doc comment).

use super::decode::RawParameters;

/// Scatters `b0..b8` back into the 49-bit `d[]` layout `decode::extract_raw_parameters` reads
/// from -- each assignment here is the direct algebraic inverse of that function's own
/// extraction, over the identical bit ranges (see `mod.rs`'s own doc-comment table).
// [@ANCHOR: pack_raw_parameters]
pub fn pack_raw_parameters(raw: &RawParameters) -> u64 {
    let mut d: u64 = 0;
    let mut set = |start: usize, width: usize, value: u32| {
        let shift = 49 - start - width;
        let mask = ((1u64 << width) - 1) << shift;
        d = (d & !mask) | (((value as u64) << shift) & mask);
    };

    set(0, 4, raw.b0 >> 3);
    set(37, 3, raw.b0 & 0b111);
    set(4, 4, raw.b1 >> 1);
    set(35, 1, raw.b1 & 1);
    set(8, 4, raw.b2 >> 1);
    set(36, 1, raw.b2 & 1);
    set(12, 8, raw.b3 >> 1);
    set(40, 1, raw.b3 & 1);
    set(20, 4, raw.b4 >> 3);
    set(41, 3, raw.b4 & 0b111);
    set(24, 4, raw.b5 >> 1);
    set(44, 1, raw.b5 & 1);
    set(28, 3, raw.b6 >> 1);
    set(45, 1, raw.b6 & 1);
    set(31, 3, raw.b7 >> 1);
    set(46, 1, raw.b7 & 1);
    set(34, 1, raw.b8 >> 2);
    set(47, 2, raw.b8 & 0b11);

    d
}

/// Builds the full transmittable 72-bit logical frame from a set of raw parameter indices:
/// scatters them into `d[]` ([`pack_raw_parameters`]) and hands that straight to
/// `ambe_dstar::encode::build_frame` (re-exported as `super::build_frame`), since that function's
/// own Golay-encode/whiten/concatenate logic is generation-agnostic -- it only depends on the
/// `d[]` layout (`c0_data = d[0..12)`, `c1_data = d[12..24)`, `c2 = d[24..35)`, `c3 = d[35..49)`),
/// which is identical here.
pub fn build_frame(raw: &RawParameters) -> u128 {
    super::build_frame(pack_raw_parameters(raw))
}

/// Packs a tone frame's 49-bit `d[]`, the exact inverse of [`super::decode::decode_tone_idx`]: `b0` is 120 (a detected
/// or DTMF/single tone) or, with `call_progress`, 122; `tone_idx` (DVSI `TONE_IDX`, Table 104's 33-61 column: DTMF
/// `0x80 | nibble`, single tones `round(f/31.25)`, call progress `0xA0/0xA1/0xA2`) is serialized as its low nibble
/// four times (`d[16..20)`, `d[24..28)`, `d[32..36)`, `d[40..44)`) and its high nibble twice (`d[20..24)`,
/// `d[28..32)`); `amplitude` is the 12-bit level field `d[4..16)`. `d[36..40)`'s low three bits carry `b0`'s marker.
// [@ANCHOR: pack_tone_parameters]
pub fn pack_tone_parameters(tone_idx: u8, call_progress: bool, amplitude: u16) -> u64 {
    let mut d: u64 = 0;
    let mut set = |msb_index: usize, width: usize, value: u32| {
        let shift = 49 - msb_index - width;
        let mask = ((1u64 << width) - 1) << shift;
        d = (d & !mask) | (((value as u64) << shift) & mask);
    };
    set(0, 4, 0b1111);
    set(37, 3, if call_progress { 0b010 } else { 0b000 });
    set(4, 12, amplitude as u32 & 0xFFF);
    let (low, high) = ((tone_idx & 0xF) as u32, (tone_idx >> 4) as u32);
    for msb in [16, 24, 32, 40] {
        set(msb, 4, low);
    }
    for msb in [20, 28] {
        set(msb, 4, high);
    }
    d
}

/// The transmittable 72-bit logical frame for a tone (see [`pack_tone_parameters`]).
pub fn build_tone_frame(tone_idx: u8, call_progress: bool, amplitude: u16) -> u128 {
    super::build_frame_from_d(pack_tone_parameters(tone_idx, call_progress, amplitude))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe::float::ambe_plus_2::decode::extract_raw_parameters;
    use crate::ambe::float::ambe_plus_2::parse_frame;

    /// The full real, end-to-end round trip this whole module exists for: pack a set of raw
    /// parameters into a transmittable frame, parse it back through the shared frame layer, and
    /// confirm both zero corrected FEC errors and an exact `RawParameters` match.
    #[test]
    fn pack_and_build_frame_round_trips_a_full_set_of_raw_parameters() {
        let original = RawParameters {
            b0: 0b010_1101, // 7 bits, < 120
            b1: 0b0_1011,   // 5 bits
            b2: 0b1_0110,   // 5 bits
            b3: 0b0_1011_0110, // 9 bits
            b4: 0b011_0101, // 7 bits
            b5: 0b1_0011,   // 5 bits
            b6: 0b0101,     // 4 bits
            b7: 0b1010,     // 4 bits
            b8: 0b011,      // 3 bits
        };
        assert!(original.b0 < 120);

        let frame = build_frame(&original);
        let parsed = parse_frame(frame);
        assert_eq!(parsed.epsilon_c0, 0, "cleanly built C0 must decode with zero errors");
        assert_eq!(parsed.epsilon_c1, 0, "cleanly built, whitened C1 must decode with zero errors");

        let recovered = extract_raw_parameters(parsed.d);
        assert_eq!(recovered, original);
    }

    /// Every real (non-special) `b0` value round-trips through pack/extract with no other field
    /// touched -- a full-range sweep guarding the `b0` scatter specifically (its own two-part
    /// split, `d[0..4)` + `d[37..40)`, is the most structurally distinctive of the nine).
    #[test]
    fn every_real_b0_value_round_trips() {
        for b0 in 0u32..120 {
            let raw = RawParameters {
                b0,
                b1: 7,
                b2: 7,
                b3: 7,
                b4: 7,
                b5: 7,
                b6: 7,
                b7: 7,
                b8: 7,
            };
            let d = pack_raw_parameters(&raw);
            let recovered = extract_raw_parameters(d);
            assert_eq!(recovered.b0, b0, "b0={b0} did not round trip");
        }
    }

    #[test]
    fn built_tone_frames_decode_back_to_their_tone_idx_and_kind() {
        use crate::ambe::float::ambe_plus_2::decode::{classify_b0, decode_tone_idx, FrameKind};
        for tone_idx in [0x05u8, 0x20, 0x7A, 0x80, 0x87, 0x8F, 0xA0, 0xA1, 0xA2] {
            for call_progress in [false, true] {
                let frame = build_tone_frame(tone_idx, call_progress, 0x555);
                let parsed = parse_frame(frame);
                assert_eq!(parsed.epsilon_c0 + parsed.epsilon_c1, 0);
                assert_eq!(decode_tone_idx(parsed.d), Some(tone_idx), "tone_idx {tone_idx:#x}");
                let raw = extract_raw_parameters(parsed.d);
                let expected = if call_progress { FrameKind::CallProgress } else { FrameKind::DetectedTone };
                assert_eq!(classify_b0(raw.b0), expected);
            }
        }
    }
}

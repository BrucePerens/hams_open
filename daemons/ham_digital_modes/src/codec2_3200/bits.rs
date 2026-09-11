// SPDX-License-Identifier: LGPL-3.0-or-later
//! Bitstream field packing: MSB-first bit order within each byte,
//! multi-bit fields (anything wider than 1 bit) Gray-coded before
//! packing. Bit order and Gray coding are real interop requirements --
//! this is the wire format any compliant decoder expects -- unlike the
//! quantizer *design* questions `quantise.rs`'s own doc comment covers.
//! `binary_to_gray`/`gray_to_binary` are the standard textbook Gray-code
//! formulas (XOR-with-shifted-self and its XOR-fold inverse), not
//! creative expression; the packing loop itself is reimplemented from
//! scratch, not translated, but verified bit-exact against the
//! reference's own real packed output (see this module's own tests).

use super::LPC_ORD;

fn binary_to_gray(x: u32) -> u32 {
    x ^ (x >> 1)
}

// [@ANCHOR: gray_to_binary]
fn gray_to_binary(g: u32) -> u32 {
    let mut g = g;
    g ^= g >> 16;
    g ^= g >> 8;
    g ^= g >> 4;
    g ^= g >> 2;
    g ^= g >> 1;
    g
}

/// Packs fields MSB-first into a byte buffer, Gray-coding any field
/// wider than 1 bit before packing.
pub struct BitWriter<'a> {
    bits: &'a mut [u8],
    bit_index: usize,
}

impl<'a> BitWriter<'a> {
    /// `bits` must already be zeroed -- `write` only ever ORs bits in.
    pub fn new(bits: &'a mut [u8]) -> Self {
        BitWriter { bits, bit_index: 0 }
    }

    // [@ANCHOR: BitWriter::write]
    pub fn write(&mut self, field: u32, width: u32) {
        // `field` is bounded to `width` bits by construction (an N-bit
        // Gray code of an N-bit value is still N bits), so
        // `field >> (remaining - slice)` -- extracting the top `slice`
        // bits of whatever's left -- is itself always bounded to
        // `slice` bits, with no need to mask off already-sent bits: a
        // right-shift can't introduce bits beyond that bound. `field`
        // itself is never modified; only `remaining` (which bits, from
        // the top, are still unsent) changes.
        //
        // That bound is a real caller precondition, not just a comment:
        // this struct only ever ORs bits in ("`bits` must already be
        // zeroed", see `new`'s own doc comment), so a caller passing a
        // `field` that doesn't actually fit in `width` bits doesn't just
        // lose its own high bits -- the extra high bit(s), landing at
        // whatever position `width` would have put them, can silently
        // set bits belonging to an *already-written, unrelated* field
        // sharing the same byte (verified: `write(32, 5)` immediately
        // after `write(5, 3)` into the same byte flips a bit inside the
        // first field's own 3-bit region, not just within the second
        // field's own 5 bits -- see this module's own regression test).
        // Cheap to catch in debug builds since every real caller today
        // (this crate's own clamping quantizers) already satisfies it.
        debug_assert!(
            width >= 32 || field < (1u32 << width),
            "BitWriter::write: field={field} does not fit in {width} bits -- would silently \
             corrupt already-packed bits elsewhere in this byte (BitWriter only ORs bits in)"
        );
        let field = if width > 1 {
            binary_to_gray(field)
        } else {
            field
        };
        let mut remaining = width;
        while remaining != 0 {
            let bits_left = 8 - (self.bit_index as u32 & 7);
            let slice = remaining.min(bits_left);
            let word_index = self.bit_index >> 3;
            let shifted = (field >> (remaining - slice)) as u8;
            self.bits[word_index] |= shifted << (bits_left - slice);
            self.bit_index += slice as usize;
            remaining -= slice;
        }
    }
}

/// Reads fields MSB-first from a byte buffer, un-Gray-coding any field
/// wider than 1 bit after unpacking.
pub struct BitReader<'a> {
    bits: &'a [u8],
    bit_index: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(bits: &'a [u8]) -> Self {
        BitReader { bits, bit_index: 0 }
    }

    // [@ANCHOR: BitReader::read]
    pub fn read(&mut self, width: u32) -> u32 {
        let mut field = 0u32;
        let mut remaining = width;
        while remaining != 0 {
            let bits_left = 8 - (self.bit_index as u32 & 7);
            let slice = remaining.min(bits_left);
            let mask = if slice == 8 {
                0xFFu32
            } else {
                (1u32 << slice) - 1
            };
            let byte = self.bits[self.bit_index >> 3] as u32;
            field |= ((byte >> (bits_left - slice)) & mask) << (remaining - slice);
            self.bit_index += slice as usize;
            remaining -= slice;
        }
        if width > 1 {
            gray_to_binary(field)
        } else {
            field
        }
    }
}

/// One 3200bps frame's worth of decoded field values, before/after
/// quantizer dequantization.
pub struct FrameFields {
    pub voiced0: bool,
    pub voiced1: bool,
    pub wo_index: u32,
    pub e_index: u32,
    pub lsp_indexes: [u32; LPC_ORD],
}

/// Packs one frame's fields into `BYTES_PER_FRAME` bytes, in the real
/// format's own field order: voiced0(1), voiced1(1), Wo(`WO_BITS`),
/// energy(`E_BITS`), then `LPC_ORD` LSP delta indices (5 bits each).
// [@ANCHOR: pack_frame]
pub fn pack_frame(fields: &FrameFields, wo_bits: u32, e_bits: u32) -> [u8; super::BYTES_PER_FRAME] {
    let mut bytes = [0u8; super::BYTES_PER_FRAME];
    let mut w = BitWriter::new(&mut bytes);
    w.write(fields.voiced0 as u32, 1);
    w.write(fields.voiced1 as u32, 1);
    w.write(fields.wo_index, wo_bits);
    w.write(fields.e_index, e_bits);
    for &idx in &fields.lsp_indexes {
        w.write(idx, 5);
    }
    bytes
}

/// Inverse of `pack_frame`.
// [@ANCHOR: unpack_frame]
pub fn unpack_frame(
    bytes: &[u8; super::BYTES_PER_FRAME],
    wo_bits: u32,
    e_bits: u32,
) -> FrameFields {
    let mut r = BitReader::new(bytes);
    let voiced0 = r.read(1) != 0;
    let voiced1 = r.read(1) != 0;
    let wo_index = r.read(wo_bits);
    let e_index = r.read(e_bits);
    let mut lsp_indexes = [0u32; LPC_ORD];
    for idx in lsp_indexes.iter_mut() {
        *idx = r.read(5);
    }
    FrameFields {
        voiced0,
        voiced1,
        wo_index,
        e_index,
        lsp_indexes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! fixture {
        ($name:literal) => {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_3200/",
                $name
            )
        };
    }

    #[test]
    // Tests [@ANCHOR: gray_to_binary]
    fn gray_code_round_trips_over_every_value_up_to_10_bits() {
        for x in 0..1024u32 {
            assert_eq!(gray_to_binary(binary_to_gray(x)), x, "x={x}");
        }
    }

    /// The exhaustive test above only covers 10 bits -- every real field
    /// this module packs today is at most 7 bits wide (`WO_BITS`), so
    /// that's already more than real callers ever exercise, but
    /// `gray_to_binary`'s own signature (`u32 -> u32`) claims correctness
    /// over the full 32-bit domain. Spot-check boundary and bit-pattern
    /// values well outside the exhaustive range to substantiate that
    /// wider claim rather than relying on "it's the standard formula"
    /// alone.
    #[test]
    // Tests [@ANCHOR: gray_to_binary]
    fn gray_code_round_trips_at_full_32_bit_boundary_and_pattern_values() {
        for &x in &[
            0u32,
            1,
            u32::MAX,
            u32::MAX - 1,
            0x8000_0000,
            0x7FFF_FFFF,
            0xAAAA_AAAA,
            0x5555_5555,
            0x1234_5678,
            0xDEAD_BEEF,
        ] {
            assert_eq!(gray_to_binary(binary_to_gray(x)), x, "x={x:#010x}");
        }
    }

    #[test]
    fn adjacent_gray_codes_differ_by_exactly_one_bit() {
        for x in 0..1023u32 {
            let diff = binary_to_gray(x) ^ binary_to_gray(x + 1);
            assert_eq!(
                diff.count_ones(),
                1,
                "x={x} -> x+1={} differ by {} bits",
                x + 1,
                diff.count_ones()
            );
        }
    }

    /// Regression test for a real bug found and fixed this pass: an
    /// out-of-range `field` (one that doesn't actually fit in the
    /// declared `width`) didn't just lose its own high bits -- because
    /// `BitWriter` only ever ORs bits in, the extra high bit(s) could
    /// land on top of an *already-written, unrelated* field sharing the
    /// same byte. Demonstrated directly here (pre-fix behavior,
    /// reconstructed by hand since the fix now rejects this input via
    /// `debug_assert!` before it can corrupt anything): `write(5, 3)`
    /// followed by `write(32, 5)` into the same byte -- `32` needs 6 bits
    /// (`0b100000`), one more than its declared 5-bit width, so its
    /// gray-coded form (`binary_to_gray(32) == 48 == 0b110000`) has a bit
    /// set at position 5, which falls inside the FIRST field's own
    /// 3-bit region (bits 7..5 of the byte), not the second field's.
    ///
    /// `#[cfg(debug_assertions)]`: the guard itself is a `debug_assert!`,
    /// compiled out entirely in a release build -- a real bug found running
    /// `hams_shared/tools/run_rust_coverage.py` (which defaults to
    /// `cargo llvm-cov --release`): under `--release`, `write(32, 5)` no
    /// longer panics at all, so `#[should_panic]` failed with "test did not
    /// panic". Gating the test to debug builds (the only mode where the
    /// assertion can ever fire) matches the guard's own documented scope --
    /// a cheap developer-time sanity check for a precondition every current
    /// real caller already satisfies, not a release-mode runtime guarantee.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "does not fit")]
    fn bit_writer_rejects_a_field_that_does_not_fit_its_declared_width() {
        let mut bytes = [0u8; 1];
        let mut w = BitWriter::new(&mut bytes);
        w.write(5, 3); // fine: 5 fits in 3 bits
        w.write(32, 5); // NOT fine: 32 needs 6 bits, declared width is 5
    }

    /// Same check for a 1-bit field (the `width > 1` gray-coding branch
    /// is skipped entirely for width==1, so this exercises the guard on
    /// its own separate code path). See the sibling test above for why this
    /// is gated to debug builds only.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "does not fit")]
    fn bit_writer_rejects_a_1_bit_field_that_is_not_0_or_1() {
        let mut bytes = [0u8; 1];
        let mut w = BitWriter::new(&mut bytes);
        w.write(2, 1);
    }

    #[test]
    // Tests [@ANCHOR: BitWriter::write]
    // Tests [@ANCHOR: BitReader::read]
    fn bit_writer_reader_round_trip_arbitrary_field_widths() {
        let mut bytes = [0u8; 8];
        let fields: [(u32, u32); 12] = [
            (1, 1),
            (0, 1),
            (73, 7),
            (19, 5),
            (5, 5),
            (31, 5),
            (0, 5),
            (17, 5),
            (9, 5),
            (22, 5),
            (13, 5),
            (11, 5),
        ];
        {
            let mut w = BitWriter::new(&mut bytes);
            for &(v, width) in &fields {
                w.write(v, width);
            }
        }
        let mut r = BitReader::new(&bytes);
        for &(v, width) in &fields {
            assert_eq!(r.read(width), v, "width={width}");
        }
    }

    /// Bit-exact test against the real reference's own real packed
    /// output (`codec2_bits_dump.txt`: voiced0 voiced1 Wo_index e_index
    /// lspd_indexes[0..9], then the real `bits[0..7]` those fields
    /// packed into) -- this is the one part of the encoder pipeline that
    /// genuinely can be checked byte-for-byte, since bit order and Gray
    /// coding are real interop requirements, not encoder-internal design
    /// choices. Catches a wrong bit order, a missed Gray-coding, or a
    /// wrong field order silently producing a plausible-looking but
    /// undecodable bitstream.
    #[test]
    // Tests [@ANCHOR: pack_frame]
    // Tests [@ANCHOR: unpack_frame]
    fn pack_frame_matches_the_real_reference_bits_on_real_captured_field_values() {
        let path = fixture!("codec2_bits_dump.txt");
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut n_checked = 0;
        for line in text.lines() {
            let v: Vec<i64> = line
                .split_whitespace()
                .map(|s| s.parse().unwrap())
                .collect();
            assert_eq!(
                v.len(),
                4 + LPC_ORD + super::super::BYTES_PER_FRAME,
                "line has {} fields: {line}",
                v.len()
            );
            let fields = FrameFields {
                voiced0: v[0] != 0,
                voiced1: v[1] != 0,
                wo_index: v[2] as u32,
                e_index: v[3] as u32,
                lsp_indexes: std::array::from_fn(|i| v[4 + i] as u32),
            };
            let expected: [u8; super::super::BYTES_PER_FRAME] =
                std::array::from_fn(|i| v[4 + LPC_ORD + i] as u8);
            let got = pack_frame(&fields, super::super::WO_BITS, super::super::E_BITS);
            assert_eq!(
                got, expected,
                "real captured frame's fields: voiced=({},{}) wo_idx={} e_idx={} lsp={:?}",
                fields.voiced0, fields.voiced1, fields.wo_index, fields.e_index, fields.lsp_indexes
            );

            let back = unpack_frame(&got, super::super::WO_BITS, super::super::E_BITS);
            assert_eq!(back.voiced0, fields.voiced0);
            assert_eq!(back.voiced1, fields.voiced1);
            assert_eq!(back.wo_index, fields.wo_index);
            assert_eq!(back.e_index, fields.e_index);
            assert_eq!(back.lsp_indexes, fields.lsp_indexes);
            n_checked += 1;
        }
        assert!(
            n_checked > 150,
            "expected the real captured fixture corpus, got {n_checked} rows"
        );
    }
}

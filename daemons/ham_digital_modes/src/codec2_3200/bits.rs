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

    pub fn write(&mut self, field: u32, width: u32) {
        // `field` is bounded to `width` bits by construction (an N-bit
        // Gray code of an N-bit value is still N bits), so
        // `field >> (remaining - slice)` -- extracting the top `slice`
        // bits of whatever's left -- is itself always bounded to
        // `slice` bits, with no need to mask off already-sent bits: a
        // right-shift can't introduce bits beyond that bound. `field`
        // itself is never modified; only `remaining` (which bits, from
        // the top, are still unsent) changes.
        let field = if width > 1 { binary_to_gray(field) } else { field };
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

    pub fn read(&mut self, width: u32) -> u32 {
        let mut field = 0u32;
        let mut remaining = width;
        while remaining != 0 {
            let bits_left = 8 - (self.bit_index as u32 & 7);
            let slice = remaining.min(bits_left);
            let mask = if slice == 8 { 0xFFu32 } else { (1u32 << slice) - 1 };
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
pub fn unpack_frame(bytes: &[u8; super::BYTES_PER_FRAME], wo_bits: u32, e_bits: u32) -> FrameFields {
    let mut r = BitReader::new(bytes);
    let voiced0 = r.read(1) != 0;
    let voiced1 = r.read(1) != 0;
    let wo_index = r.read(wo_bits);
    let e_index = r.read(e_bits);
    let mut lsp_indexes = [0u32; LPC_ORD];
    for idx in lsp_indexes.iter_mut() {
        *idx = r.read(5);
    }
    FrameFields { voiced0, voiced1, wo_index, e_index, lsp_indexes }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! fixture {
        ($name:literal) => {
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codec2_3200/", $name)
        };
    }

    #[test]
    fn gray_code_round_trips_over_every_value_up_to_10_bits() {
        for x in 0..1024u32 {
            assert_eq!(gray_to_binary(binary_to_gray(x)), x, "x={x}");
        }
    }

    #[test]
    fn adjacent_gray_codes_differ_by_exactly_one_bit() {
        for x in 0..1023u32 {
            let diff = binary_to_gray(x) ^ binary_to_gray(x + 1);
            assert_eq!(diff.count_ones(), 1, "x={x} -> x+1={} differ by {} bits", x + 1, diff.count_ones());
        }
    }

    #[test]
    fn bit_writer_reader_round_trip_arbitrary_field_widths() {
        let mut bytes = [0u8; 8];
        let fields: [(u32, u32); 12] = [(1, 1), (0, 1), (73, 7), (19, 5), (5, 5), (31, 5), (0, 5), (17, 5), (9, 5), (22, 5), (13, 5), (11, 5)];
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
    fn pack_frame_matches_the_real_reference_bits_on_real_captured_field_values() {
        let path = fixture!("codec2_bits_dump.txt");
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut n_checked = 0;
        for line in text.lines() {
            let v: Vec<i64> = line.split_whitespace().map(|s| s.parse().unwrap()).collect();
            assert_eq!(v.len(), 4 + LPC_ORD + super::super::BYTES_PER_FRAME, "line has {} fields: {line}", v.len());
            let fields = FrameFields {
                voiced0: v[0] != 0,
                voiced1: v[1] != 0,
                wo_index: v[2] as u32,
                e_index: v[3] as u32,
                lsp_indexes: std::array::from_fn(|i| v[4 + i] as u32),
            };
            let expected: [u8; super::super::BYTES_PER_FRAME] = std::array::from_fn(|i| v[4 + LPC_ORD + i] as u8);
            let got = pack_frame(&fields, super::super::WO_BITS, super::super::E_BITS);
            assert_eq!(got, expected, "real captured frame's fields: voiced=({},{}) wo_idx={} e_idx={} lsp={:?}", fields.voiced0, fields.voiced1, fields.wo_index, fields.e_index, fields.lsp_indexes);

            let back = unpack_frame(&got, super::super::WO_BITS, super::super::E_BITS);
            assert_eq!(back.voiced0, fields.voiced0);
            assert_eq!(back.voiced1, fields.voiced1);
            assert_eq!(back.wo_index, fields.wo_index);
            assert_eq!(back.e_index, fields.e_index);
            assert_eq!(back.lsp_indexes, fields.lsp_indexes);
            n_checked += 1;
        }
        assert!(n_checked > 150, "expected the real captured fixture corpus, got {n_checked} rows");
    }
}

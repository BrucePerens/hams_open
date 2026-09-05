// SPDX-License-Identifier: LGPL-3.0-or-later
//! 1600bps's own 64-bit frame layout -- genuinely different field
//! shape from `codec2_3200::bits::FrameFields` (four sub-frames' worth
//! of voicing bits, two independent `Wo`/energy pairs at 20ms cadence,
//! one LSP scalar-quantizer field at 40ms cadence with a
//! per-dimension bit width instead of one fixed width), but the same
//! real wire-format requirements (MSB-first, Gray-coded per field) --
//! so this reuses `codec2_3200::bits::{BitWriter, BitReader}` directly
//! rather than reimplementing bit-level packing.
//!
//! Field order, matching the real reference's own `codec2_encode_1600`/
//! `codec2_decode_1600`: voiced0(1), voiced1(1), Wo_a(`WO_BITS`),
//! e_a(`E_BITS`), voiced2(1), voiced3(1), Wo_b(`WO_BITS`), e_b(`E_BITS`),
//! then ten LSP scalar indices at `lsp_bits(i)` bits each (36 bits
//! total) -- 14 + 14 + 36 = 64 bits, matching the M17 spec's own
//! "64 bits encoded speech" per 40ms.

use crate::codec2_3200::bits::{BitReader, BitWriter};
use crate::codec2_3200::{E_BITS, LPC_ORD, WO_BITS};

use super::lsp_quantiser::lsp_bits;

pub struct FrameFields1600 {
    pub voiced0: bool,
    pub voiced1: bool,
    pub wo_index_a: u32,
    pub e_index_a: u32,
    pub voiced2: bool,
    pub voiced3: bool,
    pub wo_index_b: u32,
    pub e_index_b: u32,
    pub lsp_indexes: [u32; LPC_ORD],
}

pub fn pack_frame_1600(fields: &FrameFields1600) -> [u8; super::BYTES_PER_FRAME] {
    let mut bytes = [0u8; super::BYTES_PER_FRAME];
    let mut w = BitWriter::new(&mut bytes);
    w.write(fields.voiced0 as u32, 1);
    w.write(fields.voiced1 as u32, 1);
    w.write(fields.wo_index_a, WO_BITS);
    w.write(fields.e_index_a, E_BITS);
    w.write(fields.voiced2 as u32, 1);
    w.write(fields.voiced3 as u32, 1);
    w.write(fields.wo_index_b, WO_BITS);
    w.write(fields.e_index_b, E_BITS);
    for (i, &idx) in fields.lsp_indexes.iter().enumerate() {
        w.write(idx, lsp_bits(i));
    }
    bytes
}

pub fn unpack_frame_1600(bytes: &[u8; super::BYTES_PER_FRAME]) -> FrameFields1600 {
    let mut r = BitReader::new(bytes);
    let voiced0 = r.read(1) != 0;
    let voiced1 = r.read(1) != 0;
    let wo_index_a = r.read(WO_BITS);
    let e_index_a = r.read(E_BITS);
    let voiced2 = r.read(1) != 0;
    let voiced3 = r.read(1) != 0;
    let wo_index_b = r.read(WO_BITS);
    let e_index_b = r.read(E_BITS);
    let mut lsp_indexes = [0u32; LPC_ORD];
    for (i, idx) in lsp_indexes.iter_mut().enumerate() {
        *idx = r.read(lsp_bits(i));
    }
    FrameFields1600 {
        voiced0,
        voiced1,
        wo_index_a,
        e_index_a,
        voiced2,
        voiced3,
        wo_index_b,
        e_index_b,
        lsp_indexes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_round_trips_arbitrary_field_values() {
        let fields = FrameFields1600 {
            voiced0: true,
            voiced1: false,
            wo_index_a: 73,
            e_index_a: 19,
            voiced2: false,
            voiced3: true,
            wo_index_b: 42,
            e_index_b: 5,
            lsp_indexes: [10, 3, 15, 0, 8, 12, 7, 5, 1, 2],
        };
        let bytes = pack_frame_1600(&fields);
        let back = unpack_frame_1600(&bytes);
        assert_eq!(back.voiced0, fields.voiced0);
        assert_eq!(back.voiced1, fields.voiced1);
        assert_eq!(back.wo_index_a, fields.wo_index_a);
        assert_eq!(back.e_index_a, fields.e_index_a);
        assert_eq!(back.voiced2, fields.voiced2);
        assert_eq!(back.voiced3, fields.voiced3);
        assert_eq!(back.wo_index_b, fields.wo_index_b);
        assert_eq!(back.e_index_b, fields.e_index_b);
        assert_eq!(back.lsp_indexes, fields.lsp_indexes);
    }

    #[test]
    fn frame_uses_exactly_64_bits() {
        let total = 1 + 1 + WO_BITS + E_BITS + 1 + 1 + WO_BITS + E_BITS
            + (0..LPC_ORD).map(lsp_bits).sum::<u32>();
        assert_eq!(total, 64, "1600bps frame must be exactly 64 bits");
    }
}

// SPDX-License-Identifier: LGPL-3.0-or-later

//! D-STAR protocol-level framing (as distinct from the shared AMBE-2000 vocoder in
//! [`crate::ambe`], which both D-STAR and Project 25 use per
//! `AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md`'s own "The decision" section).
//!
//! Only the header checksum is implemented here so far -- the 39-byte routing header
//! (flags/RPT2/RPT1/UR/MY call) pack/unpack and the slow-data block/interleaving state
//! machine are real, separately-scoped follow-on work (see the plan doc's own "Real next
//! steps"), not attempted in this pass.
//!
//! # Sources
//!
//! - Header layout: "D-Star radio packet structure for the Digital Voice (DV) mode" by Dick
//!   Rucker, KM4ML (<https://www.qsl.net/kb9mwr/projects/dv/dstar/DV_packet_structure.pdf>,
//!   itself sourced from JARL's own protocol document).
//! - Slow data layout: "The Format of D-Star Slow Data" v0.2 by Jonathan Naylor, G4KLX
//!   (<https://www.qsl.net/kb9mwr/projects/dv/dstar/Slow%20Data.pdf>).
//! - The checksum's own exact bit-level parameterization (ambiguous in both documents above,
//!   which just say "CRC-CCITT" -- a name shared by several real, mutually incompatible
//!   variants) is confirmed against a real, executable, authoritative reference:
//!   `CDStarRX::checksum()` in G4KLX's own MMDVM firmware
//!   (<https://github.com/g4klx/MMDVM/blob/master/DStarRX.cpp>), the widely-deployed
//!   open-source D-STAR/DMR/YSF/P25/NXDN modem firmware -- not guessed.

/// Computes D-STAR's own header/slow-data checksum over `data`.
///
/// This is the standard CRC-16/X-25 parameterization (poly 0x1021 reflected as 0x8408, init
/// 0xFFFF, reflected input/output, final XOR 0xFFFF) -- confirmed two ways, not assumed from
/// the name "CRC-CCITT" alone (which is ambiguous between several incompatible real variants):
/// this exact bit-by-bit algorithm is mathematically identical to `CDStarRX::checksum()`'s own
/// table-driven implementation in MMDVM's real, deployed firmware (`DStarRX.cpp`) -- verified
/// directly by regenerating that function's own 256-entry `CCITT_TABLE` from this same
/// polynomial and confirming an exact match against the table's real published values, not by
/// re-deriving the polynomial from the name alone. Independently, this function's own result
/// for the ASCII bytes `"123456789"` matches the standard CRC-16/X-25 catalogue check value
/// (`0x906e`) -- see `dstar_checksum_matches_the_standard_crc16_x25_catalogue_check_value`.
///
/// For a real D-STAR header, the transmitted 2-byte FCS is this value in little-endian byte
/// order (low byte first, matching the source algorithm's own `crc8[0]`/`crc8[1]` union
/// layout on the little-endian target it runs on) appended after the checksummed bytes.
// [@ANCHOR: dstar_checksum]
pub fn dstar_checksum(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x8408;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Appends `dstar_checksum(data)`'s own little-endian FCS bytes onto `data`, matching how a
/// real D-STAR header/slow-data block carries its checksum on the wire.
// [@ANCHOR: dstar_checksum_append]
pub fn dstar_checksum_append(data: &[u8]) -> Vec<u8> {
    let crc = dstar_checksum(data);
    let mut out = data.to_vec();
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

/// Verifies that the last 2 bytes of `data_with_fcs` are the correct little-endian FCS for
/// everything before them. Returns `false` for an input shorter than 2 bytes (nothing to
/// checksum against) rather than panicking.
// [@ANCHOR: dstar_checksum_verify]
pub fn dstar_checksum_verify(data_with_fcs: &[u8]) -> bool {
    if data_with_fcs.len() < 2 {
        return false;
    }
    let (body, fcs) = data_with_fcs.split_at(data_with_fcs.len() - 2);
    dstar_checksum(body).to_le_bytes() == *fcs
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: dstar_checksum]
    #[test]
    fn dstar_checksum_matches_the_standard_crc16_x25_catalogue_check_value() {
        // "123456789" is the standard CRC RevEng catalogue check string; CRC-16/X-25's own
        // published check value for it is 0x906e -- an independent confirmation of this
        // function's exact parameterization beyond just matching MMDVM's own table, since a
        // bug shared between "regenerate the table from the polynomial" and "the real
        // algorithm" wouldn't be caught by the table-match check alone.
        assert_eq!(dstar_checksum(b"123456789"), 0x906e);
    }

    // Tests [@ANCHOR: dstar_checksum]
    #[test]
    fn dstar_checksum_of_empty_data_is_the_bare_inverted_init_value() {
        // No bytes processed -- the whole algorithm degenerates to just complementing the
        // initial 0xFFFF register, i.e. 0x0000. A real, if degenerate, sanity boundary.
        assert_eq!(dstar_checksum(b""), 0x0000);
    }

    // Tests [@ANCHOR: dstar_checksum_append], [@ANCHOR: dstar_checksum_verify]
    #[test]
    fn append_then_verify_round_trips_for_arbitrary_data() {
        for data in [&b""[..], b"K6BP", b"D-STAR routing header goes here!!!"] {
            let with_fcs = dstar_checksum_append(data);
            assert_eq!(with_fcs.len(), data.len() + 2);
            assert!(
                dstar_checksum_verify(&with_fcs),
                "append-then-verify must round-trip for {data:?}"
            );
        }
    }

    // Tests [@ANCHOR: dstar_checksum_verify]
    #[test]
    fn verify_rejects_a_single_corrupted_byte_anywhere_in_the_data() {
        let with_fcs = dstar_checksum_append(b"K6BP DE W1AW");
        for i in 0..with_fcs.len() {
            let mut corrupted = with_fcs.clone();
            corrupted[i] ^= 0xFF;
            assert!(
                !dstar_checksum_verify(&corrupted),
                "flipping byte {i} must be detected"
            );
        }
    }

    // Tests [@ANCHOR: dstar_checksum_verify]
    #[test]
    fn verify_returns_false_rather_than_panicking_on_data_shorter_than_the_fcs_itself() {
        assert!(!dstar_checksum_verify(b""));
        assert!(!dstar_checksum_verify(b"a"));
    }
}

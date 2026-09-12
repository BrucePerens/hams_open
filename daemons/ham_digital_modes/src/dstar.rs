// SPDX-License-Identifier: LGPL-3.0-or-later

//! D-STAR protocol-level framing (as distinct from the shared AMBE-2000 vocoder in
//! [`crate::ambe`], which both D-STAR and Project 25 use per
//! `AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md`'s own "The decision" section).
//!
//! The header checksum and the 39-byte routing header's own pack/unpack are implemented here.
//! The slow-data block/interleaving state machine is real, separately-scoped follow-on work
//! (see the plan doc's own "Real next steps"), NOT attempted in this pass -- checked directly
//! (not assumed) whether MMDVM's own real firmware, already the authoritative second reference
//! that resolved the checksum's own ambiguity below, has an equivalent slow-data type/length
//! parser to cross-check against: it does not (a repeater only needs to relay slow-data bits
//! transparently, never interpret their type, so MMDVM has no reason to parse them at all).
//! G4KLX's own Slow Data document is explicit that real ambiguities remain in what it
//! documents ("There are still a number of unanswered issues about the slow data... further
//! tests and investigations are needed") -- implementing the block type/length nibble split
//! speculatively, with no second source to confirm it against, would be exactly the kind of
//! unverified guess this whole module exists to avoid; the header/checksum pieces above were
//! implementable with confidence specifically because both were independently cross-checked,
//! and slow data currently is not. The routing header's 3 flag bytes are similarly carried as
//! opaque raw bytes, not decoded bit-by-bit: KM4ML's own DV packet structure doc says only
//! "same as DD mode" for their meaning, without stating what that is.
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

/// The 41 bytes a D-STAR routing header actually occupies on the wire: 3 flag octets + RPT2(8) +
/// RPT1(8) + UR(8) + MY call(8) + MY call suffix(4) = 39 bytes, followed by the 2-byte FCS.
/// Matches G4KLX's own Slow Data doc, which independently states "forty-one bytes altogether"
/// for a full header replay.
pub const DSTAR_HEADER_LEN: usize = 41;
const DSTAR_HEADER_BODY_LEN: usize = 39;

/// One D-STAR routing header (KM4ML's DV packet structure doc's own field order): 3 opaque
/// flag bytes, then four space-padded ASCII callsign-shaped fields. All four callsign fields
/// share the same real-world constraint (an amateur radio callsign, `RPT1`/`RPT2` a repeater's
/// own suffixed callsign) -- represented as owned `String`s here (already trimmed of the padding
/// used only on the wire), not fixed-size byte arrays, since nothing in this module needs to
/// avoid the allocation and a `String` is far easier for a caller to work with correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DStarHeader {
    pub flags: [u8; 3],
    /// Destination repeater callsign, ≤8 ASCII characters.
    pub rpt2: String,
    /// Departure repeater callsign, ≤8 ASCII characters.
    pub rpt1: String,
    /// Companion/target station's own callsign, ≤8 ASCII characters.
    pub ur_call: String,
    /// Own station's callsign, ≤8 ASCII characters.
    pub my_call: String,
    /// Own station's callsign suffix (module/SSID-shaped), ≤4 ASCII characters.
    pub my_call_suffix: String,
}

/// Why [`DStarHeader::pack`] refused to build a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DStarHeaderError {
    /// A field held a real character (or byte) outside 7-bit ASCII printable range -- D-STAR
    /// callsign fields are ASCII space-padded text, not arbitrary bytes.
    NonAscii,
    /// A field's own real content (excluding the space padding) was longer than the wire format
    /// has room for.
    FieldTooLong,
}

impl DStarHeader {
    fn pack_field(value: &str, width: usize, out: &mut Vec<u8>) -> Result<(), DStarHeaderError> {
        if !value.is_ascii() {
            return Err(DStarHeaderError::NonAscii);
        }
        if value.len() > width {
            return Err(DStarHeaderError::FieldTooLong);
        }
        out.extend_from_slice(value.as_bytes());
        out.extend(std::iter::repeat_n(b' ', width - value.len()));
        Ok(())
    }

    /// Packs this header into its 39-byte wire body (flags + the four space-padded callsign
    /// fields), WITHOUT the trailing 2-byte FCS -- pass the result to
    /// [`dstar_checksum_append`] to get the full 41-byte on-air header. Fails closed
    /// ([`DStarHeaderError`]) rather than silently truncating or corrupting an overlong or
    /// non-ASCII field into a wrong-but-well-formed-looking header.
    // [@ANCHOR: DStarHeader::pack]
    pub fn pack(&self) -> Result<[u8; DSTAR_HEADER_BODY_LEN], DStarHeaderError> {
        let mut out = Vec::with_capacity(DSTAR_HEADER_BODY_LEN);
        out.extend_from_slice(&self.flags);
        Self::pack_field(&self.rpt2, 8, &mut out)?;
        Self::pack_field(&self.rpt1, 8, &mut out)?;
        Self::pack_field(&self.ur_call, 8, &mut out)?;
        Self::pack_field(&self.my_call, 8, &mut out)?;
        Self::pack_field(&self.my_call_suffix, 4, &mut out)?;
        Ok(out.try_into().expect("exactly DSTAR_HEADER_BODY_LEN bytes by construction"))
    }

    /// Packs this header AND appends its own real FCS, producing the full 41 bytes a real
    /// D-STAR header occupies on the wire.
    // [@ANCHOR: DStarHeader::pack_with_fcs]
    pub fn pack_with_fcs(&self) -> Result<[u8; DSTAR_HEADER_LEN], DStarHeaderError> {
        let body = self.pack()?;
        let with_fcs = dstar_checksum_append(&body);
        Ok(with_fcs.try_into().expect("body len + 2 == DSTAR_HEADER_LEN by construction"))
    }

    fn unpack_field(bytes: &[u8]) -> String {
        // Real wire padding is ASCII space (0x20); trim it from the end only -- a callsign
        // can't legitimately start with a space, but trimming both ends would silently accept
        // a malformed leading-space field as if it were a shorter, valid one.
        String::from_utf8_lossy(bytes).trim_end_matches(' ').to_string()
    }

    /// Parses a 39-byte header body (no FCS) back into its fields. Never fails: any byte
    /// pattern is a well-formed sequence of characters once run through
    /// [`String::from_utf8_lossy`] (a real corrupted/non-ASCII byte becomes `U+FFFD`, visibly
    /// wrong rather than silently misinterpreted) -- checking the FCS via
    /// [`dstar_checksum_verify`] before calling this is the caller's own job, matching how a
    /// real receiver checks the checksum before trusting the header's contents at all.
    // [@ANCHOR: DStarHeader::unpack]
    pub fn unpack(body: &[u8; DSTAR_HEADER_BODY_LEN]) -> Self {
        Self {
            flags: [body[0], body[1], body[2]],
            rpt2: Self::unpack_field(&body[3..11]),
            rpt1: Self::unpack_field(&body[11..19]),
            ur_call: Self::unpack_field(&body[19..27]),
            my_call: Self::unpack_field(&body[27..35]),
            my_call_suffix: Self::unpack_field(&body[35..39]),
        }
    }
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

    fn sample_header() -> DStarHeader {
        DStarHeader {
            flags: [0x00, 0x00, 0x00],
            rpt2: "RPT2ABC".to_string(),
            rpt1: "RPT1XYZ".to_string(),
            ur_call: "CQCQCQ".to_string(),
            my_call: "K6BP".to_string(),
            my_call_suffix: "P".to_string(),
        }
    }

    // Tests [@ANCHOR: DStarHeader::pack], [@ANCHOR: DStarHeader::unpack]
    #[test]
    fn pack_then_unpack_round_trips_exactly() {
        let header = sample_header();
        let body = header.pack().unwrap();
        assert_eq!(body.len(), DSTAR_HEADER_BODY_LEN);
        assert_eq!(DStarHeader::unpack(&body), header);
    }

    // Tests [@ANCHOR: DStarHeader::pack]
    #[test]
    fn pack_places_every_field_at_its_documented_byte_offset() {
        // Direct, independent check against KM4ML's own stated field order and widths (3 flags,
        // then RPT2(8)/RPT1(8)/UR(8)/MY call(8)/MY call suffix(4)) -- not just "round-trips
        // through this module's own unpack," which couldn't catch a bug shared between pack and
        // unpack's own offsets (e.g. both silently agreeing on a byte-swapped field order).
        let header = DStarHeader {
            flags: [0xAA, 0xBB, 0xCC],
            rpt2: "AAAAAAAA".to_string(),
            rpt1: "BBBBBBBB".to_string(),
            ur_call: "CCCCCCCC".to_string(),
            my_call: "DDDDDDDD".to_string(),
            my_call_suffix: "EEEE".to_string(),
        };
        let body = header.pack().unwrap();
        assert_eq!(&body[0..3], &[0xAA, 0xBB, 0xCC]);
        assert_eq!(&body[3..11], b"AAAAAAAA");
        assert_eq!(&body[11..19], b"BBBBBBBB");
        assert_eq!(&body[19..27], b"CCCCCCCC");
        assert_eq!(&body[27..35], b"DDDDDDDD");
        assert_eq!(&body[35..39], b"EEEE");
    }

    // Tests [@ANCHOR: DStarHeader::pack]
    #[test]
    fn a_short_callsign_is_padded_with_real_ascii_spaces_not_nulls() {
        let header = sample_header(); // my_call "K6BP" is 4 of the field's own 8 bytes
        let body = header.pack().unwrap();
        assert_eq!(&body[27..35], b"K6BP    ");
    }

    // Tests [@ANCHOR: DStarHeader::pack]
    #[test]
    fn an_overlong_field_is_refused_not_silently_truncated() {
        let mut header = sample_header();
        header.my_call = "TOOLONGCALL".to_string(); // 11 chars into an 8-byte field
        assert_eq!(header.pack(), Err(DStarHeaderError::FieldTooLong));
    }

    // Tests [@ANCHOR: DStarHeader::pack]
    #[test]
    fn a_non_ascii_field_is_refused_not_silently_mangled() {
        let mut header = sample_header();
        header.rpt1 = "K6BP\u{00e9}".to_string(); // a real non-ASCII character (é)
        assert_eq!(header.pack(), Err(DStarHeaderError::NonAscii));
    }

    // Tests [@ANCHOR: DStarHeader::unpack]
    #[test]
    fn unpack_trims_trailing_padding_but_not_a_real_interior_space() {
        let mut body = [b' '; DSTAR_HEADER_BODY_LEN];
        body[3..10].copy_from_slice(b"K6BP W1"); // 7 real chars incl. one interior space, body[10] stays the real pad space
        let header = DStarHeader::unpack(&body);
        assert_eq!(header.rpt2, "K6BP W1");
    }

    // Tests [@ANCHOR: DStarHeader::pack_with_fcs]
    #[test]
    fn pack_with_fcs_produces_a_header_that_verifies_and_unpacks_back_to_the_original() {
        let header = sample_header();
        let full = header.pack_with_fcs().unwrap();
        assert_eq!(full.len(), DSTAR_HEADER_LEN);
        assert!(dstar_checksum_verify(&full));
        let body: [u8; DSTAR_HEADER_BODY_LEN] = full[..DSTAR_HEADER_BODY_LEN].try_into().unwrap();
        assert_eq!(DStarHeader::unpack(&body), header);
    }

    // Tests [@ANCHOR: DStarHeader::pack_with_fcs]
    #[test]
    fn a_single_corrupted_byte_in_a_transmitted_header_fails_the_fcs_check() {
        let full = sample_header().pack_with_fcs().unwrap();
        for i in 0..full.len() {
            let mut corrupted = full;
            corrupted[i] ^= 0xFF;
            assert!(
                !dstar_checksum_verify(&corrupted),
                "flipping header byte {i} must be caught by the FCS check"
            );
        }
    }
}

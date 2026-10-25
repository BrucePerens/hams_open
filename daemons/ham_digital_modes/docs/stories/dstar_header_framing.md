# Story: D-STAR routing header and checksum (ham_digital_modes)

A D-STAR voice transmission begins with a 41-byte routing header: three flag bytes, the destination repeater
(RPT2), departure repeater (RPT1), the companion callsign (UR) and the sender's callsign (MY), each 8 ASCII
characters padded with spaces, then a 4-character MY suffix, then a 2-byte checksum. This module builds and
checks that header. It does not handle the voice frames or slow data.

## Checksum

`dstar_checksum` is CRC-16/X-25: reflected polynomial 0x8408, initial value 0xFFFF, final inversion. For the
ASCII bytes `123456789` it gives 0x906E, the catalogue check value, and it matches G4KLX's MMDVM firmware.
*(Reference: `src/dstar.rs` -> `[@ANCHOR: dstar_checksum]`)*

`dstar_checksum_append` returns a copy of the data with the checksum added low byte first, as sent on air.
*(Reference: `src/dstar.rs` -> `[@ANCHOR: dstar_checksum_append]`)*

`dstar_checksum_verify` takes data plus its two trailing checksum bytes and says whether they match. Input
shorter than two bytes is simply false, not a panic.
*(Reference: `src/dstar.rs` -> `[@ANCHOR: dstar_checksum_verify]`)*

## Header

`DStarHeader::pack` builds the 39-byte body. Each callsign field is padded with spaces to its width. A
non-ASCII field gives `NonAscii` and an overlong one gives `FieldTooLong`; nothing is truncated or altered.
*(Reference: `src/dstar.rs` -> `[@ANCHOR: DStarHeader::pack]`)*

`DStarHeader::pack_with_fcs` does the same and appends the checksum, giving the complete 41 bytes.
*(Reference: `src/dstar.rs` -> `[@ANCHOR: DStarHeader::pack_with_fcs]`)*

`DStarHeader::unpack` reads a 39-byte body back into fields, removing trailing space padding only (a leading
space is kept as a sign of damage). It never fails: bytes that are not valid text become U+FFFD. Verify the
checksum with `dstar_checksum_verify` on the full 41 bytes before trusting the result.
*(Reference: `src/dstar.rs` -> `[@ANCHOR: DStarHeader::unpack]`)*

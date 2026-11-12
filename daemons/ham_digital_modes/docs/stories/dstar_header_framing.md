# Story: D-STAR routing header and checksum (ham_digital_modes)

A D-STAR voice transmission begins with a 41-byte routing header: three flag bytes, the destination repeater
(RPT2), departure repeater (RPT1), the companion callsign (UR) and the sender's callsign (MY), each 8 ASCII
characters padded with spaces, then a 4-character MY suffix, then a 2-byte checksum. This module builds and
checks that header. It does not handle the voice frames or slow data.

## Checksum

### `dstar_checksum` [@ANCHOR: dstar_checksum]

`dstar_checksum` is CRC-16/X-25: reflected polynomial 0x8408, initial value 0xFFFF, final inversion. For the
ASCII bytes `123456789` it gives 0x906E, the catalogue check value, and it matches G4KLX's MMDVM firmware.
| Argument | Type | Meaning |
|---|---|---|
| `data` | `&[u8]`, borrowed | Bytes to checksum (for a header, the 39-byte body). Any length including empty. |

**Returns:** `u16`, the CRC. **Errors:** none. **Special concerns:** pure, no state; bitwise loop, cost linear in the length. **Signature:** `pub fn dstar_checksum(data: &[u8]) -> u16`. **See also:** tests tagged `dstar_checksum` (the `123456789` check value and a known header).
*(Reference: `src/dstar.rs`)*

### `dstar_checksum_append` [@ANCHOR: dstar_checksum_append]

`dstar_checksum_append` returns a copy of the data with the checksum added low byte first, as sent on air.
| Argument | Type | Meaning |
|---|---|---|
| `data` | `&[u8]`, borrowed | Payload without a checksum. |

**Returns:** `Vec<u8>`, a new vector of `data.len() + 2` bytes; the CRC low byte comes first, then the high byte. **Errors:** none. **Special concerns:** pure; the input is not modified. **Signature:** `pub fn dstar_checksum_append(data: &[u8]) -> Vec<u8>`. **See also:** `DStarHeader::pack_with_fcs`, which calls it, and its tests.
*(Reference: `src/dstar.rs`)*

### `dstar_checksum_verify` [@ANCHOR: dstar_checksum_verify]

`dstar_checksum_verify` takes data plus its two trailing checksum bytes and says whether they match. Input
shorter than two bytes is simply false, not a panic.
| Argument | Type | Meaning |
|---|---|---|
| `data_with_fcs` | `&[u8]`, borrowed | A payload followed by its two checksum bytes, low byte first. |

**Returns:** `bool`: `true` when the CRC of everything before the last two bytes equals those bytes. **Errors:** none; input of fewer than two bytes returns `false` without panicking. **Special concerns:** pure; this is a transmission-error check, not authentication. **Signature:** `pub fn dstar_checksum_verify(data_with_fcs: &[u8]) -> bool`. **See also:** `dstar_checksum_append` (the inverse) and tests tagged `dstar_checksum_verify`.
*(Reference: `src/dstar.rs`)*

## Header

### `DStarHeader::pack` [@ANCHOR: DStarHeader::pack]

`DStarHeader::pack` builds the 39-byte body. Each callsign field is padded with spaces to its width. A
non-ASCII field gives `NonAscii` and an overlong one gives `FieldTooLong`; nothing is truncated or altered.
| Argument | Type | Meaning |
|---|---|---|
| `&self` | `DStarHeader`, borrowed | Fields `flags` (3 bytes), `rpt2`, `rpt1`, `ur_call`, `my_call` (each at most 8 ASCII characters) and `my_call_suffix` (at most 4). |

**Returns:** `Result<[u8; 39], DStarHeaderError>`: the body in wire order (flags, RPT2, RPT1, UR, MY, suffix). **Errors:** `NonAscii` when any field holds a non-ASCII character; `FieldTooLong` when a field exceeds its width. **Special concerns:** pure; it builds bytes only and never sends them. The per-field work is a private helper that pads with spaces. **Signature:** `pub fn pack(&self) -> Result<[u8; 39], DStarHeaderError>`. **See also:** tests tagged `DStarHeader::pack`, `DStarHeader::unpack`.
*(Reference: `src/dstar.rs`)*

### `DStarHeader::pack_with_fcs` [@ANCHOR: DStarHeader::pack_with_fcs]

`DStarHeader::pack_with_fcs` does the same and appends the checksum, giving the complete 41 bytes.
**Call interface:** `pub fn pack_with_fcs(&self) -> Result<[u8; 41], DStarHeaderError>`; `self` is borrowed and not modified. **Returns:** the 39-byte body followed by the 2-byte checksum, low byte first, ready to put on the wire. **Errors:** exactly those of `DStarHeader::pack`, which it calls first; no checksum is computed when packing fails. **Special concerns:** pure and synchronous, no locks, and it does not transmit. **See also:** `dstar_checksum_append`, `dstar_checksum_verify` (checks the result) and tests tagged `DStarHeader::pack_with_fcs`.
*(Reference: `src/dstar.rs`)*

### `DStarHeader::unpack` [@ANCHOR: DStarHeader::unpack]

`DStarHeader::unpack` reads a 39-byte body back into fields, removing trailing space padding only (a leading
space is kept as a sign of damage). It never fails: bytes that are not valid text become U+FFFD. Verify the
checksum with `dstar_checksum_verify` on the full 41 bytes before trusting the result.
| Argument | Type | Meaning |
|---|---|---|
| `body` | `&[u8; 39]`, borrowed | The header body without its checksum; the fixed array length is the only size check. |

**Returns:** `DStarHeader` with the three flag bytes copied and each callsign field trimmed of trailing spaces. **Errors:** none; invalid UTF-8 becomes U+FFFD. **Special concerns:** pure; the result is untrusted until the caller has verified the checksum over the full 41 bytes, and callsigns from the air should not be used as paths or commands without checking them. **Signature:** `pub fn unpack(body: &[u8; 39]) -> DStarHeader`. **See also:** tests tagged `DStarHeader::unpack`.
*(Reference: `src/dstar.rs`)*

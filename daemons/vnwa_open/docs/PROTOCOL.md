# DG8SAQ VNWA 3E: USB protocol as observed

Source: usbmon captures (2026-10-25) of the vendor program (VNWA 36.8.2) running under Wine through the
vendor's own Linux bridge, against the owner's unit. Nothing in this crate comes from the vendor's
code or firmware, and no firmware was loaded or flashed. The unit is USB `20a0:4118` ("DG8SAQ-VNWA 3.0",
bcdDevice 1.00), plus two `08bb:29c0` PCM2900C audio codecs that carry the measured signal.

## Transport

Only vendor control transfers on endpoint 0 (`bmRequestType` 0x40 out, 0xC0 in). There is no bulk or
interrupt traffic. The measurement itself arrives as audio from the two codecs (48 kHz nominal; the
vendor program calibrates the two sample rates).

## Hardware

Two Analog Devices AD9859 DDS chips (one stimulus, one local oscillator), clocked from one ~36 MHz
crystal through the chips' PLL multipliers. In the stock setting (the user declined the vendor's
overclock offer) the core clock is 360 MHz.

## Confirmed on the wire

| Item | Evidence |
|---|---|
| Generator frequency: request 0x4c, 8 bytes = `00 00 00 00` + tuning word (u32 little endian), word = round(f x 2^32 / 360 MHz) | 1 MHz `61 0b b6 00`, 2 MHz `c1 16 6c 01`, 10 MHz `c7 71 1c 07`, 20 MHz `8e e3 38 0e` |
| RF level: request 0x58, 14-bit level in `wValue` (0x3fff maximum) | user set 3fff; the program's level search stepped 0x3e8a down to 0x0fb5 and back |
| Measurement set-up block: nine writes 0x40, 0x41, 0x42, 0x43, 0x44, 0x47, 0x4b, 0x49, 0x64 | identical order every time; only the 0x49 payload changes between points |

## Decoded against the AD9859 data sheet (Rev. B, Table 5)

The set-up block is a load of AD9859 registers, sent little endian:

* Request 0x40, 7 bytes = CFR1 (4 bytes) then CFR2 (3 bytes). Seen: CFR1 `0x01080002`, CFR2 `0x00007d`.
  CFR2<7:3> = 15, so the REFCLK multiplier is 15; CFR2<2> = 1 (VCO range high); CFR2<1:0> = 1 (charge pump).
  With the 360 MHz core clock the tuning words imply, the reference is 24 MHz. CFR1<8> (LSB first) is clear.
  The bit that differs between "generator off" and "on" (CFR1 `0x01000002` against `0x01080002`) is CFR1<19>,
  which the data sheet lists as not used, so it is probably a board-level control bit carried in that word by the
  microcontroller (hypothesis).
* The first four bytes of requests 0x41, 0x42, 0x43, 0x44 and 0x4b are frequency tuning words (FTW0) at 360 MHz:
  0x41 145.92 MHz, 0x42 3.4133 MHz, 0x43 5.12 MHz, 0x44 85.3333 MHz, 0x4b 48.8889 MHz. The remaining bytes
  (0x41 `d8 15 02`, 0x47 `01 40`, 0x4b `65 00 03`) are not decoded. Which chip each request addresses is not known.
* Request 0x49: the low 32 bits of the 6-byte tail are an FTW (e.g. `0x22c3f35c`, 48.8889 MHz, one more than the
  word in 0x4b); the upper 16 bits are not decoded.

## Single-point sweep capture (v04_points)

* The 6-byte tail of request 0x49 is a **signed 48-bit** little-endian word: offset = word x 360 MHz / 2^48.
  Three tails seen decode to exactly +5/9 MHz (`5c f3 c3 22 65 00`), -4/9 MHz (`50 3d 63 17 af ff`) and
  -1/2 MHz (`fa a4 4f fa a4 ff`), so the 0x49 word is a frequency offset, not an absolute frequency. (Earlier
  readings as an unsigned or 32-bit word only looked arbitrary.)
* The first byte of the 0x49 payload counted down 5, 5, 5, 5, 4, 3, 3, 3 during one sweep: a step or phase
  counter. Its meaning is not decoded.
* New read requests during the sweep: 0x3a (2 bytes, `0a 09`, the 0x090a that the generator's 0x4c request
  carries in its `wValue`), 0x72 (1 byte, `01`) and 0x35 with `wValue` 1 (4 bytes, `c7 71 1c 07`: the generator's
  current tuning word, 10 MHz, read back).
* **Sweep frequency found (25 MHz run).** Bytes 4 to 8 of the 0x49 payload are a 40-bit little-endian word
  w = round(f / 9 x 2^40 / 360 MHz). Seen: `e7 87 45 ca 00` for the 10 MHz sweep (10/9 MHz worth),
  `c1 d3 ad f9 01` for 25 MHz (25/9), and `31 27 3a 14 00` / `61 4e 74 28 00` for 1 MHz and 2 MHz
  (1/9 and 2/9). The factor nine is unexplained. The generator's read-back (request 0x35, wValue 1) returned the
  tuning words of 10 MHz (`c7 71 1c 07`) and then 25 MHz (`71 1c c7 11`), confirming request 0x4c's word.

## The offset tail (found with single-point sweeps, 1 to 100 MHz)

The signed 48-bit tail of request 0x49 is `(previous - new) / divisor` MHz, with `previous` the frequency before the
sweep started (intermediate words do not change it), `new` the frequency of the word in the same payload, and
`divisor` = 3 x (first byte - 1): 6 when the first byte is 3, 9 when it is 4. Checked on 14 values, e.g. 1 to 4 MHz
-0.5, 1 to 5 MHz -0.6667, 5 to 50 MHz -7.5, 50 to 100 MHz -8.3333, 50 to 1 MHz +8.1667, 50 to 10 MHz +6.6667,
10 to 100 MHz -15, 25 to 10 MHz +2.5 (byte 3) and +1.6667 (byte 4). The encoder in `protocol.rs` reproduces the
captured bytes exactly, including rounding. What the first byte and the divisor mean is not known (possibly the
number of stages the chip takes to move from the old frequency to the new one).

## Seen but not decoded

* Request 0x49 payload (15 bytes): `[b0][00 0a 09 ..][6-byte tail]`. `b0` varies (0x21, 0x05, 0x06, 0x04,
  0x2b, 0x19, 0xca). The tail, read as a little-endian number, is proportional to a frequency (ratios
  1:2:15, 1:10:15:30 seen), but it does not map one-to-one onto the sweep points: in a 3-point 10 to 20 MHz
  sweep the last four writes were identical.
* The first byte pair of the 0x40 payload: `02 00 08 01 7d 00 00` with the generator on and `02 00 00 01 ..`
  off (bit 0x08 of the third byte).
* Reads at start-up: 0x00 (5 bytes), 0xee (64 bytes, a rising table), 0x3f, 0xf7, 0xff, 0x97, 0xf2, 0x58,
  0x86, 0x46 (8 bytes), 0x4f, 0xf3, 0x71, 0x60, 0x0e (polled), 0x85 (2 bytes, jitters), 0x91, 0x66, 0x62,
  0x61, 0x98, 0x9a, 0x04, 0x0d, 0x02.
* Request 0x15 read one byte at a time from 42 fixed addresses (0x00 to 0x1b1). The bytes look like an
  identity string; the vendor program uses them to recognise the hardware. An open driver does not need
  them unless the unit is found to refuse to measure without them (untested).

## Not tried

No transfer has been sent by this crate. Measurements (the audio path, the LO word, the sweep) are not
decoded. The unit once dropped off the bus and came back as a new device number (a loose connector).

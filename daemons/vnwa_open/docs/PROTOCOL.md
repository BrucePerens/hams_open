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

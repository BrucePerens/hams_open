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

## Clock structure (from the vendor program's Instrument Settings page, owner's unit)

The unit's two DDS chips are an **AD9859** and an **AD9951** (the program lists both). Their register maps are
identical (CFR1 0x00, CFR2 0x01, ASF 0x02, ARR 0x03, FTW0 0x04, POW0 0x05; REFCLK multiplier CFR2<7:3>, 4x to 20x),
and neither has a frequency-sweep engine, so a sweep is arithmetic done by the unit's microcontroller. Both chips are
fed 12 MHz (TCXO) x 3 (premultiplier, constant) = 36 MHz, then multiplied by nRF = 10 (RF clock 360 MHz) and
nLO = 9 (LO clock 324 MHz).

**Correction.** An earlier version of these notes read request 0x40's 7-byte payload as CFR1 and CFR2 and derived a
x15 multiplier and a 24 MHz reference. That does not fit the settings above (x10 and x9 on 36 MHz) and was a
misreading; the meaning of request 0x40 and of 0x41, 0x42, 0x43, 0x44, 0x47 and 0x4b is not known.

## Request 0x49 payload (15 bytes)

`[step][00][nRF][nLO][sweep word, 5 bytes][offset, 6 bytes]`, with nRF = 0x0a and nLO = 0x09 in every capture
(read back as `0a 09` by request 0x3a). The sweep word is `round(f * 2^40 / (36 MHz * nRF * nLO))`, little endian, and
each chip's tuning word is one integer multiply of it: `FTW_RF = floor(w * nLO / 256)`, `FTW_LO = floor(w * nRF / 256)`.
The RF value equals the tuning word the unit reports when the generator setting is read back (request 0x35) at all of
1, 5, 10, 25, 50 and 100 MHz. The factor nine seen earlier is nLO.

## Audio path (loopback, TX connected to RX, 10 MHz single sweep, both codecs recorded at 48 kHz stereo)

Each codec gives a clean tone in both channels during the sweep: **12.000 kHz** (about 1100 to 1200 rms) at 10 MHz.
An earlier run in the same recording showed 18.000 kHz. So the IF is a few kHz, fs/4 at 48 kHz for the 12 kHz tone.
How the commands produce the IF is not decoded: the LO tuning word computed from the sweep word lands on the RF
frequency to within 0.2 Hz, so the 12 kHz difference comes from something else (an offset sent elsewhere, or the
alias scheme the Help file describes).

## The IF, and the set-up constants (one-variable test: n = 2)

The program's "n" setting gives IF = 48 kHz / (4 n): n = 1 is 12 kHz, n = 2 is 6 kHz. Changing n from 1 to 2 changed
exactly two set-up requests, each to half its value, and the codec audio moved from a 12.000 kHz to a 6.000 kHz tone:

* **0x42**, bytes 0 to 3 = `round(IF * 256 * 2^32 / 324 MHz)`: 12 kHz gives 3.072 MHz worth (`dd 60 6d 02`), 6 kHz gives
  1.536 MHz worth (`6e b0 36 01`). The fifth byte (0) is unchanged.
* **0x41** (7 bytes): the 32-bit word (131.328 MHz worth at n = 1, 65.664 MHz at n = 2) and the following 16-bit field
  (`0x15d8` at n = 1, `0x0aec` at n = 2) both halve; the last byte (`02`) stays. Meaning not known.
* **n = 3 (IF 4 kHz)** confirmed both ways: the codec audio showed a 4.000 kHz tone, 0x42 became `4a 20 cf 00 00`
  (1.024 MHz worth), and 0x41 became `4c a4 96 22 | 48 07 | 02`: the 32-bit word is `round(IF * 10944 * 2^32 / 324 MHz)`
  and the 16-bit field is `IF * 0.466` (12 kHz: 5592, 6 kHz: 2796, 4 kHz: 1864), a fit exact at all three settings.
* Unchanged by n: 0x40, 0x43, 0x44, 0x47, 0x4b and the 0x49 words. 0x43 = 4.608 MHz worth = 18 kHz x 256, and an
  18.000 kHz tone appears in the codec audio just before every sweep (a calibration/level burst); 0x44 = 76.8 MHz
  worth = 300 kHz x 256 (not matched to anything yet). 0x4b = 44.0 MHz worth (not matched).

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

* The first byte of request 0x49's payload (the "step" counter, 3, 4 or 5 in the single-point sweeps).
* Request 0x40: `02 00 08 01 7d 00 00` with the generator on and `02 00 00 01 7d 00 00` off (bit 0x08 of the third
  byte); not CFR1/CFR2 (see the correction above).
* Reads at start-up: 0x00 (5 bytes), 0xee (64 bytes, a rising table), 0x3f, 0xf7, 0xff, 0x97, 0xf2, 0x58,
  0x86, 0x46 (8 bytes), 0x4f, 0xf3, 0x71, 0x60, 0x0e (polled), 0x85 (2 bytes, jitters), 0x91, 0x66, 0x62,
  0x61, 0x98, 0x9a, 0x04, 0x0d, 0x02.
* Request 0x15 read one byte at a time from 42 fixed addresses (0x00 to 0x1b1). The bytes look like an
  identity string; the vendor program uses them to recognise the hardware. An open driver does not need
  them unless the unit is found to refuse to measure without them (untested).

## Not tried

No transfer has been sent by this crate. Measurements (the audio path, the LO word, the sweep) are not
decoded. The unit once dropped off the bus and came back as a new device number (a loose connector).

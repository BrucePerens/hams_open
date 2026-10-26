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

## Time per point (request 0x40)

The program sends one block when a new time per point takes effect (at the next `setgrid` or sweep). Its first byte, the third
and the sixth follow the time per point exactly (84 distinct settings from 0.2 to 100 ms, one steady-state block each, IF 4 kHz,
through the vendor program's remote control `timeperpoint`):

* **b0 = ceil(1.5 x milliseconds)**, at least 1 (0.7 ms: 2, 1.4 ms: 3, 2.1 ms: 4, 15 ms: 23, 100 ms: 150). An earlier rounding rule fitted
  only the few settings seen at the time.
* **b5 = ceil(16 / b0)** (16, 8, 6, 4, 4, 3, 3, 2 ... 2 for b0 = 1 ... 15, then 1) and **b2** the same except 3 at b0 = 5; both are 1 from
  b0 = 16 up. Why the program has two such counts is not known. An earlier block with bytes (2, 1) at 8 ms was captured while the
  setting was still being stepped with the arrow keys and conflicts with the steady-state value (2, 2); it is not used.
* The block these settings produce carries a placeholder 0x49 payload (step counter 31, the 1 MHz word); the step counter in real
  sweep blocks is still understood only for 3 and 4 (the offset divisor `3 x (counter - 1)`).
  In the GUI-driven runs the step counter byte of request 0x49 was 2 at 30 and 60 ms and 3 to 5 before; its meaning is still unknown.

The pre- and post-sample counts did not change anything on the wire (they are PC-side processing). So 0x40 is a timing block, not
CFR1/CFR2 (see the correction above). `setup::setup_block` accepts any time per point from 0.2 to 100 ms.

## The set-up block as a whole (173 captured blocks)

Every block is the same nine writes in the same order: 0x40, 0x41, 0x42, 0x43, 0x44, 0x47, 0x4b, 0x49, 0x64.
`wValue`: 0x40 and 0x41 carry a "mode" (0 mostly, also 1, 2, 0x0302, 0x0303 and 8; meaning unknown); 0x42 is 7,
0x43, 0x44 and 0x47 are 5, 0x4b is 6, 0x49 and 0x64 are 0. `wIndex`: the RF level (14 bits) on the first seven
requests, 0 on 0x49 and 0x64. In 120 blocks the level is 0 on all nine and in 50 it is on the first seven; in a few
blocks it appeared on only the first one or the first four, which looks like a race inside the vendor program (a
variable changing while the block is being sent), so the builder does not reproduce it. The bytes that are not
derived from the settings (0x47 `00 00 00 00 01 40`, 0x4b `5b f3 c3 22 65 00 03`, 0x40 bytes 1, 3, 4 and 6, the
last byte of 0x41) are the same in all blocks. `setup::setup_block` builds the block from a frequency, the previous
frequency, the IF, the time per point, the step counter, the mode and the level, and reproduces nine captured blocks
byte for byte (frequencies 1 to 100 MHz, IF 12, 6 and 4 kHz, 1.33, 8 and 60 ms).

Limits of the builder: the bytes 2 and 5 of 0x40 are known only for 1.33, 8, 30 and 60 ms, and the offset divisor
`3 x (counter - 1)` only for counters 3 and 4 (other counters, seen in the first blocks after start-up, give offsets
that do not follow it), so it refuses those combinations.

## Audio to a complex number (`audio.rs`)

From the vendor's Help file: amplitude and phase come from at least one whole IF period of samples, after skipping the
presamples that follow a frequency change and before the postsamples that precede the next; the right channel is the
reference. `audio::measure_point` does that with the exact tone-at-IF sum over whole periods. Real loopback samples
(10 MHz, TX to RX, both codecs) give left/right = 1.12 at 179 degrees (12 kHz), 1.22 at -178 degrees (6 kHz) and 1.24 at
-177 degrees (4 kHz): a constant inversion and a slowly rising gain ratio, so the IF-dependent part is the unit's analogue
path and the calibration is meant to remove it. The recording also shows the signal arriving in bursts (a single sweep
gives the tone for part of a second only), so a reader must gate on the reference amplitude.

## Reference check against the vendor program's own S21 (2026-10-26)

Method. The vendor program was started with its documented `-tcpip 55555` remote control (listening only inside the private
network namespace) and driven with `setgrid`, `timeperpoint 60` and `sweep s21`; the S21 it returned on its data port is the
reference (TX connected to RX, no calibration, IF 4 kHz). The codecs can be opened by only one program at a time and the program
paces its sweep by its audio capture, so my own recording could not run alongside it. Instead the same sweep was run with no
audio device selected in the vendor program (the hardware still performs the first point) while both codecs were recorded.

What it showed:
* The loopback path is flat in frequency: the vendor's single-point S21 is -0.780 + 0.052j (|S21| 0.782, 176.2 degrees) at 1, 3,
  5, 10, 15, 20, 25 and 30 MHz, repeatable to 0.3 percent.
* Over a sweep, both the vendor's values and my recording drift smoothly with **time** (|S21| 0.783 to 0.803 in the first 0.84 s,
  phase by about 2 degrees): a settling transient of the unit. An earlier comparison that spread my 0.88 s recording over the
  vendor's 30 points (1.8 s) looked like a frequency trend that did not agree; that was the wrong time axis, not a measurement
  difference. The recorded run only produced the tone for the first 0.88 s (the program stops without sync).
* With the vendor's point k placed at t = k x 60 ms after the tone starts, my S21 matches the vendor's to 1.3 to 1.5 percent rms
  (best-fit scale 0.995, phase offset 0.8 degrees) over the first 15 points.
* At single frequencies (1, 10, 20, 30 MHz, two runs each) my conjugated left/right ratio from codec 2 is 0.7881 at 176.17
  degrees, the vendor's 0.781 to 0.783 at 176.1 to 176.4 degrees: 0.7 to 0.9 percent apart.
* The vendor's phase is the complex conjugate of `measure_point`'s (`audio::vendor_convention`).
* Both codecs match once the reference channel is right: codec 2 carries the reference on its right channel (the default) and
  codec 1 on its **left** (`audio::Reference::Left`; the vendor program has a "Reference = Right/Left Channel" setting).
  Without that option codec 1 looked like a different measurement (ratio 1.279, the inverse).

IF dependence (a non-flat check). The unit's own path has an amplitude and phase response that depends on the IF, and the
vendor program reports it: S21 on the TX-to-RX loopback is 0.874 at 172.3 degrees at IF 12 kHz, 0.799 at 174.7 at 6 kHz and
0.782 at 176.2 at 4 kHz, the same from 1 to 300 MHz (S11 returns the same numbers there). Single-point sweeps at 5 and 25 MHz with
IF 12 kHz and 6 kHz (`VNWA_SamplesPerPeriod` 4 and 8 in `VNWA.ini`), recorded on both codecs, match the vendor values to 0.1 to 0.3
percent (codec 1, reference left) and 0.5 to 1.0 percent (codec 2, reference right), with phase within 0.2 degrees.

Not covered: the loopback is flat in RF frequency (to 300 MHz), so a device under test with a frequency-dependent response was not
measured (the IF dependence above is the only non-flat response that was available without one), and the unit's time drift is
not explained. `audio::tests` keeps the recorded chunks and the vendor
values as regression tests (sweep laid out in time: scale 0.98 to 1.02, phase offset under 2 degrees, residual under 3 percent;
four single frequencies: under 2 percent; IF 12 kHz and 6 kHz with both codecs: under 1 and 2 percent, phase within 1 degree).

Wine audio notes for this setup. The vendor's Linux guide is right: in a fresh prefix Wine 10 under PipeWire shows the codecs only
with the ALSA driver (`HKCU\Software\Wine\Drivers` `Audio` = `alsa`); with the Pulse driver the two codecs have the same name and a
sweep returned no data. `autosetupaudio` failed (error 6) even then, so the capture device names were written into `VNWA.ini`
(`SoundDevice=In: USB AUDIO  CODEC - USB Audi`, `AuxSoundDevice=In: 2-USB AUDIO  CODEC - USB Au`). Without a selected device the
program shows "No Sync Found" and the remote control returns zero for every S value.

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

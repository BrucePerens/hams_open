# SDRplay RSP USB protocol, from observed wire traffic

Status: the RSP2pro section is complete as far as captures without a signal source allow (2026-10-25); it was verified by a small stand-alone program (1.12) and by the open driver in this directory, `rsp_open` (1.13). The RSP1A section (2) is complete as far as captures without a signal source allow (2026-10-25) and was checked by the same kind of stand-alone program (2.10); `rsp_open` drives the RSP1A from it (2.13). The RSP1 section is in progress.

Goal: describe, in our own words and only from the USB traffic between SDRplay's software and receivers the author owns, how the RSPs are controlled, so that an open driver can be written. This document is the specification `rsp_open` was written from.

Confidence tags: **[obs]** seen directly on the wire and reproducible; **[fit]** a field interpretation that fits every captured value (often confirmed by matching the GPL kernel msi2500/msi001 formulas); **[guess]** plausible but not tested; **[meas]** our own measurement on the running receiver with `rsp_open`, which sends only requests listed here as observed; **[hw]** a fit confirmed by the receiver's behaviour under `rsp_open`. Items that came from the `rsp_open` work say so ("rsp_open:").

## 0. Clean-room boundary, provenance and how the captures were made

What was done:
- The Linux kernel's usbmon facility (binary interface, memory-mapped ring) was read by our own capture program. It kept only events whose device address equalled the RSP's address as read from sysfs at that moment (re-read every 0.25 s; address 0 always discarded), so no other device's traffic was ever written. Firmware-sized control OUT payloads (> 64 bytes) were recorded as a length only.
- Controls were driven either through the SDRconnect application's websocket interface (one capture) or through our own small client of the **public, documented SDRplay API v3.15**, built against the API's published headers and linked to its library: ordinary use of the API with SDRplay hardware. The API service ran as an ordinary user process.
- References used to interpret the traffic, for facts only (no code copied): Linux `drivers/media/usb/msi2500/msi2500.c` and `drivers/media/tuners/msi001.c` (GPL) and libmirisdr-4 (GPL).

What was not done: no disassembly, decompilation, strings-mining, debugger or tracing (ltrace/strace/LD_PRELOAD) of the API library, the API service, SDRconnect or any other SDRplay binary; no SDRplay code or documentation text is reproduced here; no firmware image was stored or examined (the captures hold only its length). Nothing was connected to any antenna port at any time.

Licence: SDRplay's software is used under SDRplay's own licence terms. This document describes only the bus traffic of the author's own receiver; much of it overlaps what the public GPL msi2500/msi001 drivers already document.

The API package used was SDRplay's public Linux download, API version 3.15.2 (`SDRplay_RSP_API-Linux-3.15.2.run`, 511429 bytes, SHA-256 `3a97ca764263bbe76fb0f2220e6408942357e8864c19e1408a6d6987af382fe3`). Its archive was unpacked without running any of its code (not even the self-extracting stub or its install script).

## 1. RSP2pro (USB 1df7:3010, API hwVer 2)

### 1.1 Descriptors (from `lsusb -v -d 1df7:3010`, read-only)

- USB 2.0 high speed, one configuration, bus powered, MaxPower 400 mA, class/subclass/protocol 0xff/0xff/0xff, EP0 max packet 64.
- Cold (as powered up): bcdDevice **2.00**, no string descriptors. After the firmware load: bcdDevice **2.07**, iSerial 1 = 10-digit serial number. [obs]
- Interface 0, alternate settings [obs]:
  - alt 0: no endpoints (idle).
  - alt 1: EP 0x81 IN isochronous, 3 x 1024 bytes/microframe (high-rate formats, ADC clock >= 3 MHz).
  - alt 2: EP 0x81 IN isochronous, 1 x 1024 bytes/microframe (used for ADC clock 2.0/2.048 MHz).
  - alt 3: EP 0x81 IN bulk, 512. **Selecting the API's bulk mode hung the device** (section 1.10); do not use.

### 1.2 Software path used for each capture

| Capture | Front end | What |
|---|---|---|
| service start | API service alone | service start: no USB traffic at all |
| firmware load | SDRconnect + API service | **firmware load**, re-enumeration, first open |
| SDRconnect without API service | SDRconnect, no API service | SDRconnect reads the serial string itself, then aborts (SIGABRT, exit 134) |
| SDRconnect baseline | SDRconnect + API service | SDRconnect's default open (100 MHz WFM, 2 MS/s, low-IF 1.62 MHz, AGC on), property probe, 30 s idle |
| per-control captures | our API client + API service | one control at a time (below) |
| USB-port recovery | none | recovery from the bulk-mode hang |

- With the RSP2pro already warm (firmware loaded), SDRconnect_headless without `sdrplay_apiService` read the serial string and then aborted right after start (reproduced 3 times, with and without the API library on `LD_LIBRARY_PATH`). With the service running it worked, so all RSP2pro device-control bytes below were generated by the API service.
- **Correction (2026-10-25 12:45 PDT): SDRconnect appears to load RSP firmware by itself.** On the RSP1A, a cold device (bcdDevice 2.00, plugged in 12:37:02) re-enumerated warm (2.07, serial present) at 12:37:56, the same second SDRconnect_headless started, with no API service running [inferred from timing and the kernel log, not captured on the wire]. The warm-device RSP2pro test above could not show this. So SDRconnect probably contains its own USB loader and uses the API service only for streaming/control (or failed for another reason on the warm RSP2pro); the abort is unexplained.
- SDRconnect's websocket exposes only centre/VFO frequency, sample rate, LNA state (0..8), antenna (`Port A`, `Port B`), AGC on/off and demodulator/filter properties; no bias-T, notch, Hi-Z, external reference, IF mode, IF gain, decimation, DC/IQ or ppm. Those controls were captured with our own API client.
- Init order differs by front end only in the parameters chosen (SDRconnect asks for 6 MHz ADC + 1.62 MHz IF + BW 1.536 MHz; our client used zero-IF 2 MHz); the order of writes is the same API sequence (1.5).

### 1.3 Firmware load and re-enumeration (firmware-load capture)

When the API first opened the cold device (bcdDevice 2.00) [obs]:
1. Vendor OUT 0x44, wValue 0x0000, wIndex 0, wLength 4096 (data not recorded).
2. Vendor OUT 0x44, wValue 0x1000, wIndex 0, wLength 1987. wValue is the load offset. **Total firmware upload 6083 bytes in two transfers.** (libmirisdr names 0x44 `CMD_DOWNLOAD`.)
3. Vendor OUT 0x41 register write: MSi2500 register 0x08 = 0x000080.
4. Vendor OUT 0x40 (libmirisdr `CMD_RESET`), wValue 0x0001: the device leaves the bus.
5. ~0.7 s later it re-enumerates at a new address with bcdDevice 2.07 and a serial string.
6. Host reads string 0 and string 1 (serial), then vendor IN 0x46 (EEPROM read), wValue 0x0040, wLength 16 -> 16 x 0xff on this unit.

The firmware stays resident until power is removed; disabling and re-enabling the root-hub port (through the port's sysfs `disable` switch) re-enumerated it still at 2.07, so that port's VBUS is not switched. Every later open (API or SDRconnect) starts with steps 6 only. **An open driver needs its own firmware for a cold RSP2pro, or must require that SDRplay's software ran once since power-up.** What the boot-ROM (2.00) personality can do without the download is outside the scope of this document.

### 1.4 Vendor requests seen

All vendor requests use EP0, bmRequestType 0x40 (OUT) or 0xc0 (IN). [obs] for fields; names from libmirisdr where they exist.

| bRequest | Dir | wValue | wIndex | wLength | Meaning |
|---|---|---|---|---|---|
| 0x40 | OUT | 0x0001 | 0 | 0 | reset/boot downloaded firmware (only after 0x44) |
| 0x41 | OUT | `(value & 0xff) << 8 \| reg` | `value >> 8` | 0 | write 24-bit `value` to MSi2500 register `reg` [fit, same as kernel/libmirisdr] |
| 0x42 | IN | 0x0000 | 0x0010 | 4 | read 4 bytes (only during tuner VCO calibration and at close, see 1.6.6) |
| 0x43 | OUT | 0 | 0 | 0 | start streaming |
| 0x44 | OUT | offset | 0 | <= 4096 | firmware download block |
| 0x45 | OUT | 0 | 0 | 0 | stop streaming |
| 0x46 | IN | 0x0040 | 0 | 16 | read EEPROM at 0x40 (blank, 0xff) |
| 0x48 | IN | 0 | 0 | 1 | read one byte, twice at every open: always 0x04 in the API's opens; **0x09 in every rsp_open open** (rsp_open: [obs]). Meaning unknown; see 1.13.6 |
| 0x49 | OUT | `addr << 8 \| byte` | 0x0040 | 0 | write one byte to a board register `addr` (0x00, 0x01, 0x12, 0x13 seen). libmirisdr calls 0x49 "WGPIO"; on the RSP2 these are register writes to the board's MCP23S18 SPI I/O expander, wIndex 0x40 being its SPI write control byte (1.8.1) [fit] |

Standard requests: SET_INTERFACE (alt 1/2 at start, alt 0 at close), GET_DESCRIPTOR string 0/1.

### 1.5 Open / start / stop sequence (API, zero-IF, 2 MHz, 100 MHz, IF gain reduction 40 dB, LNA state 0; basic-open capture)

`REG xx = v` is request 0x41; `TUNER rN = v` is request 0x41 to register 0x09 (SPI to the MSi001, see 1.6); `B aa = bb` is request 0x49.

```
REG 08 = 00ee80                (open)
SET_INTERFACE alt 2            (alt 1 for >= 3 MHz ADC clock)
READ 0x48 -> 04 ; READ 0x48 -> 04
REG 05 = 00000c
TUNER r14 = 00000e
REG 00 = 000200
REG 02 = 004801
TUNER r5 = 28bb85              (PLL threshold 3000)
REG 07 = 000014                (stream format, 1.7)
TUNER r6 = 200016
TUNER r0 = 043420              (mode VHF, zero IF, BW 200 kHz)
B 12 = 5d ; B 13 = a5 ; B 00 = 00 ; B 01 = 00     (board band/antenna byte, front-end byte, two latch/commit writes)
TUNER r3 = 00fff3
TUNER r2 = 213e72              (PLL N/frac -> 100.000 MHz)
TUNER r13 = 00000d ; TUNER r14 = 00000e
TUNER r1 = 00c281              (gain: IF GR 40)
B 13 = b9                      (front-end byte for LNA state 0)
REG 04 = 000000 ; REG 03 = 01181f   (ADC clock synthesiser: 2.000 MHz)
START (0x43) ; STOP (0x45) after ~60 ms ; START again after ~300 ms   (always this triple at init)
... stream ...
STOP (0x45)
B 13 = b9 ; TUNER r12 = 00001c ; TUNER r0 = 000000 (tuner standby)
READ 0x42 wIndex 0x10 -> 4 bytes
TUNER r13 = 00020d ; TUNER r14 = 7102ee  (byte 1 of the 0x42 reply appears in r14 bits 7:4.. e.g. reply 102e.. -> 7102ee, 102a.. -> 7102ae)
TUNER r0 = 063000 ; TUNER r14 = 00800e
REG 03 = 091300 ; REG 05 = 000004   (ADC clock / IF sleep)
SET_INTERFACE alt 0
```
Every retune repeats `B 12, B 13 = a5, B 00 = 00, B 01 = 00, ...` around the tuner writes when the band or gain table changes, and ends with `B 13 = <front-end byte>` after the gain write. [obs]

### 1.6 MSi001 tuner registers (via MSi2500 register 0x09)

The 24-bit value written to register 0x09 is one MSi001 SPI word: bits 3:0 = tuner register address, bits 23:4 = payload, exactly as in the kernel msi001 driver [fit, every captured word decodes consistently].

1.6.1 **r0 (mode/IF/BW)**: bits 11:4 = band mode, 13:12 = IF mode, 16:14 = bandwidth, 19:17 = first-LO plan (AM mode).
- Band mode by RF [obs]: 1 kHz - <60 MHz `0xe1` (AM, upconverting, Antenna A/B path); Hi-Z port selects `0x61` instead (1.8); 60 - <120 MHz `0x42` (VHF); 120 - <250 MHz `0x44` (B3); **250 - <420 MHz `0xe1` again (AM mode)**; 420 - <1000 MHz `0x48` (B4/5); 1000 - 2000 MHz `0x50` (L). Edges measured to <= 2.5 MHz (exact list 1.9).
- IF mode [obs]: zero IF = 3, 450 kHz = 2, 1.620 MHz = 1, 2.048 MHz = 0.
- Bandwidth [obs]: 200 kHz 0, 300 kHz 1, 600 kHz 2, 1.536 MHz 3, 5 MHz 4, 6 MHz 5, 7 MHz 6, 8 MHz 7.
- Bits 19:17 [obs]: 2 normally; in AM mode the LO-plan setting changes them: LO 120 MHz = 2, 144 MHz = 1, 168 MHz = 0 (toggle capture). "Auto" picks 120 MHz below 60 MHz, and in 250-420 MHz switches 120 -> 144 at ~375 MHz and -> 168 at ~395 MHz. So in AM mode the first IF is 120/144/168 MHz (5, 6 or 7 x 24 MHz).
- r0 = 000000 is standby; 063000 is written at close.

1.6.2 **r2 (PLL integer/fraction)**: bits 15:4 = fraction, 23:16 = integer N. **r5** = 28bb85: bits 15:4 = fraction modulus 3000 (constant in every capture). Frequency [fit, checked e.g. 100 MHz: N 33, frac 999]:
`f_LO = 96 MHz x (N + frac/3000) / div`, div = 32 (VHF), 16 (B3), 4 (B4/5), 2 (L), 16 in AM mode. In AM mode the first IF (IF1 = 120, 144 or 168 MHz, 1.6.1) enters with opposite signs in the two AM bands [fit]: below 60 MHz the path upconverts, `f_LO = f_RF + IF1` (1 kHz -> 120 MHz, 10 MHz -> 130 MHz, 30 MHz with LO 144 -> 174 MHz); in 250-420 MHz it downconverts, `f_LO = f_RF - IF1` (250 MHz -> 130 MHz, 300 MHz -> 180 MHz, 400 MHz with LO 168 -> 232 MHz). Resolution is coarse (2 kHz at HF in AM mode); the API evidently finishes the tuning digitally (no USB traffic for small offsets inside one PLL step was looked for; open question).

1.6.3 **r1 (gain)**: bits 9:4 = IF gain reduction in dB (0..59, written directly; gR 0 needs min-GR "extended", 1.11); bits 11:10 = AM-path (mixer buffer) gain reduction step, used by LNA states on HF/Hi-Z; bit 12 = mixer gain reduction; bit 13 = MSi001 LNA gain reduction; bits 16:14 normally 3 (seen once as 4 at 30 MHz, unexplained). [fit]

1.6.4 **r3**: bits 15:4 are **12 more PLL fraction bits** below the r2 fraction (rsp_open: [fit] on all 66 captured (frequency, r2, r3) triples, [hw] tuning within one 122 Hz FFT bin in every band). With `q = ceil(f_LO x div x 3000 x 4096 / (96 MHz x (1 + ppm/1e6))) - 1` (the largest integer strictly below the exact quotient): N = q / (3000 x 4096), frac = (q / 4096) mod 3000, r3 payload = q mod 4096. This explains every captured value: an exact multiple gives frac-1 and r3 0xfff (100 MHz -> 213e72 / 00fff3, i.e. the API's "truncation" of 1.6.2 is this rule, not a rounding choice); half and quarter steps above 420 MHz give 0x7ff/0x3ff/0xbff; ppm +10 at 100 MHz gives r2 unchanged and r3 0x000 (000003). The ppm correction is applied to the tuner LO (divide by 1 + ppm), not only to the ADC clock; on hardware ppm +10 moved the internal 96 MHz harmonic by +0.98 kHz when tuned 95.8 MHz, as a 10 ppm LO shift predicts (rsp_open: [meas]).

1.6.5 **r6** = 200016 (constant). **r12** = 00001c (written before VCO calibration and at close). **r13/r14**: 00000d/00000e at every tune; other values during calibration (1.6.6).

1.6.6 **VCO / first-LO calibration**: when the AM-mode first-LO plan changes (LO 144/168 MHz, i.e. 375-420 MHz, or explicitly), and at close, the API writes r12 = 1c, r0 = 0, reads 4 bytes with request 0x42 (wIndex 0x10), then writes r13 = 0x0002Xd and r14 = a value containing bytes from that reply, then the new r0. The reply varies run to run (e.g. `102e440e`, `110e320e`). Read-back semantics unknown [obs, meaning open]; fitted and tested in 5.6.

### 1.7 MSi2500 registers and stream format

Register writes seen: 0x00 = 000200, 0x02 = 004801, 0x05 = 00000c (open) / 000004 (close), 0x08 = 00ee80 (open; 000080 just before the firmware reset), 0x03/0x04 (ADC clock), 0x07 (stream format). These match the kernel driver's init values except reg 0x08 (kernel 0x00f380 / 0x006080) [obs].

**ADC clock (reg 0x03, 0x04)** [fit, every value captured decodes exactly, including ppm]: reg 0x03 value bits 11:8 = N, bit 7 = fraction bit 20, bits 4:2 = output divider code d (divider 2(d+1)), bits 1:0 = 3, bits 15:14 = clock-range code (0 below 6 MHz, 1 at 6, 2 at 7-8, 3 at 8.192-10 MHz), bit 12 set on the write made at open and on writes caused by a ppm change, clear on writes caused by a sample-rate change [obs; meaning unknown], bits 23:16 = 0x01; reg 0x04 value bits 19:0 = fraction bits 19:0. `f_ADC = 48 MHz x (N + frac/2^21) / (2(d+1) x 12)`. Examples: 2 MHz 01181f/000000, 2.048 MHz 01081f/0624dd, 6 MHz 01490b, 8 MHz 018807, 10 MHz 01ca07; ppm +10 at 2 MHz gives 01179f/0fff58 (1.99998 MHz).

**Format register 0x07 and the stream** (sample-rate and IF/bandwidth captures) [obs]:

| ADC clock | reg 0x07 | alt | samples per 1024-byte block | sample coding |
|---|---|---|---|---|
| 2.0 MHz (zero IF) | 000014 | 2 | 252 IQ | 16-bit LE words, I then Q (14-bit range seen) |
| 2.048 MHz | 000005 | 2 | 336 IQ | 12-bit packed |
| 3 - 6 MHz | 000094 | 1 | 252 IQ | 16-bit |
| 7 - 8 MHz | 000085 | 1 | 336 IQ | 12-bit packed |
| 8.192 - 9 MHz | 0000a5 | 1 | 384 IQ | kernel "384" format (10-bit + per-group shift bits) |
| 10 MHz | 000c94 | 1 | 504 IQ | 8-bit signed |
| any low-IF mode (450 kHz/2 MHz, 1.62 MHz/6 MHz, 2.048 MHz/8.192 MHz) | 0x4xx (e.g. 000414, 000494) | 2 or 1 | 252 IQ | 16-bit, **decimated by 2 in the device** |

- bit 7 accompanies alt 1 (3 x 1024 per microframe); bits 11:10 = 3 is the 8-bit format; **bit 10 alone halves the output rate**: the block counter still advances by 504 per 252-sample block, i.e. it counts ADC clocks [fit]. With bit 10 the API receives e.g. 3 MS/s from a 6 MHz clock and does the IF downconversion and further decimation on the host.
- Each 1024-byte block (three per microframe in alt 1, one in alt 2; the API's iso URBs have 64 packets; at 2 MHz zero IF every microframe carries one block, at SDRconnect's 6 MHz low-IF setting every other microframe carries three) starts with a 16-byte header: bytes 0-3 LE sample counter (advances by 252, 336, 384 or 504 per block as above, or by 504 in decimate-by-2 mode), bytes 4-7 a status word (`28 10 00 00`, `28 70 00 00`, `3b 70 00 00` seen; byte 4/5 vary with gain state and run, meaning unknown), bytes 8-15 a pattern that cycles through four fixed values. Then 1008 bytes of samples. [obs]
- Throughput observed: 2 MHz zero IF 8.19 MB/s (alt 2); SDRconnect's 6 MHz low-IF 12.19 MB/s (alt 1, half the microframes); usbmon reported no dropped events in any capture.
- Host-side only (no USB traffic at all when changed): API decimation factor 2..32, DC offset enable, IQ imbalance enable, DC calibration mode. AGC runs on the host: with AGC on, the API keeps rewriting TUNER r1 (and B 13) as the level changes (SDRconnect baseline capture shows r1 stepping 00c3b1 -> 00c1b1).

### 1.8 Board control bytes (request 0x49, wIndex 0x40)

**Byte 0x12** (band filter, antenna, reference) [obs, bit roles fit]:
- bits 1:0 antenna: `01` = Antenna A, `10` = Antenna B (0x5d <-> 0x5e at 100 MHz).
- bits 5:3 RF band filter, set purely by frequency (table 1.9).
- bit 6: external reference clock output, **0 = enabled** (0x5d -> 0x1d when the API's reference-output option is turned on).
- bit 2 = 1, bit 7 = 0 in every capture.
- Hi-Z (AM port 1) does not change byte 0x12; it changes the MSi001 band mode 0xe1 -> 0x61 (r0), i.e. Hi-Z is the MSi001's other AM input.

**Byte 0x13** (front end) [obs]:
- bit 0: bias-T, **0 = on** (0xb9 -> 0xb8). Toggled twice (Antenna A selected and Antenna B selected) **with nothing connected to any port**, then left off.
- bits 7:6 RF notch (the API's FM/MW broadcast notch for RSP2): `10` off, `01` on (0xb9 -> 0x79), same at 10 MHz, 100 MHz and on Hi-Z.
- bits 5:2 front-end gain step selected by the LNA state (below); 0xa5 is written transiently during every retune.
- Bytes 0x00 and 0x01 are always written 0x00 after 0x12/0x13: these are the expander's direction registers IODIRA/IODIRB (all pins outputs), see 1.8.1 (this replaces the earlier "latch/commit" guess).

**LNA state -> (byte 0x13 bits 5:2, MSi001 r1 bits 13:10)**, IF GR 40, Antenna A [obs, gain and toggle captures]:

| Band | states | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|---|---|
| 1-60 MHz and 250-420 MHz (AM mode) | 9 | b9 / - | b5 / - | a5 / - | 95 / - | b9 / AMbuf3 | b5 / AMbuf3 | a5 / AMbuf3 | 95 / AMbuf3 | 95 / AMbuf3+mixer |
| 60-250 MHz VHF/B3 (100 MHz measured) | 9 | b9 / - | b5 / - | a5 / - | 95 / - | b9 / LNA | b5 / LNA | a5 / LNA | 95 / LNA | 95 / LNA+mixer |
| 420-1000 MHz (450 MHz) | 6 | b9 / - | b9 / LNA | b5 / - | b5 / LNA | a5 / LNA | a5 / LNA+mixer | - | - | - |
| 1000-2000 MHz (1.5 GHz) | 6 | b9 / - | b9 / LNA | 95 / - | a5 / - | b5 / LNA | b5 / LNA+mixer | - | - | - |
| Hi-Z port (10 MHz) | 5 | b9 / - | b9 / AMbuf1 | b9 / AMbuf2 | b9 / AMbuf3 | b9 / AMbuf3+mixer | (5 rejected) | | | |

(`-` = r1 bits 13:10 all 0; "LNA" = r1 bit 13; "mixer" = bit 12; "AMbufN" = bits 11:10 = N.) Byte 0x13 bits 5:2 therefore encode the board's attenuator/LNA bypass steps: `1110` (0xb9), `1101` (0xb5), `1001` (0xa5), `0101` (0x95); their dB values are not measurable here without a signal source. States above the band's count are refused by the API (OutOfRange).

#### 1.8.1 Board control expander (MCP23S18)

Sources: **[photo]** part marking read from a photograph of the author's RSP2pro board; **[ds]** Microchip datasheet DS22103A (MCP23018/MCP23S18, public, read for facts only); **[obs]** our captures.
- The board-control bytes go to a **Microchip MCP23S18-E/SO**, a 16-bit SPI I/O expander, on the left side of the board (marking `MCP23S18-E/SO`, date code 1641) [photo]. Its outputs are open-drain [ds].
- In the expander's default register map (IOCON.BANK = 0, DS22103A Table 1-1), address 0x00 is IODIRA, 0x01 IODIRB, 0x0A/0x0B IOCON, **0x12 GPIOA, 0x13 GPIOB**, 0x14/0x15 OLATA/OLATB [ds]. So "byte 0x12" is port A (GPA7:0) and "byte 0x13" is port B (GPB7:0); antenna = GPA1:0, band filter = GPA5:3, ext-ref output = GPA6; bias-T = GPB0, front-end step = GPB5:2, notch = GPB7:6 [fit].
- Its SPI control byte is 0100 000 followed by the R/W bit, i.e. **0x40 for a write** (DS22103A Figure 1-7) [ds]. That is exactly request 0x49's constant wIndex 0x0040, so request 0x49 is most likely "write register (wValue high byte) = data (wValue low byte) over SPI with control byte wIndex", forwarded by the MSi2500's firmware [fit].
- At power-on reset IODIRA/IODIRB are 0xFF (all pins inputs) [ds], and a write to a GPIO register goes to its output latch (OLAT) [ds]. Every capture shows the same pattern: GPIOA, GPIOB (the transient 0xa5), then **IODIRA = 0x00, IODIRB = 0x00** (all 16 pins outputs), then the final GPIOB value. So the writes to 0x00/0x01 described in 1.8 as a "latch/commit" guess are the direction registers: the API loads the output values first and then turns the pins into outputs, and repeats this on every full tune (redundant after the first, harmless) [obs + ds]. Single-control changes (antenna, notch, bias-T, ext-ref) write only GPIOA or GPIOB [obs].
- Across every RSP2pro capture (open, sweeps, toggles, rate changes, the bulk-mode attempt and the verification run; 566 request-0x49 writes) and every RSP1A capture, request 0x49 addressed only 0x00, 0x01, 0x12 and 0x13, always with wIndex 0x0040. **No write to IOCON (0x0A/0x0B), OLAT (0x14/0x15) or any other expander register was seen**, so the board relies on the power-on IOCON default (BANK = 0) [obs]. The firmware-load capture has no request-0x49 traffic at all; the first expander writes come with the first open after the load [obs]. Whether the board bytes would work without the IODIR writes was not tested: after power-up the pins would stay inputs, and rsp_open always writes IODIR as the API does [ds, inference].
- In every RSP2pro capture GPA2 is always written 1 and GPA7 and GPB1 always 0; their function is unknown (open-drain 0 = pin pulled low) [obs, guess].
- The same four addresses and wIndex on the RSP1A suggest the RSP1A uses the same expander arrangement, with a different bit map (section 2) [fit]. The expander part on the RSP1A board has not been identified from a photograph.
- A 25640-family SPI EEPROM (8-pin SOIC, marking partly hidden) sits next to the expander [photo]. Whether it is what request 0x46 reads is not known.

### 1.9 Frequency -> band switching (frequency-sweep and band-edge captures; steps 0.1-2.5 MHz)

| RF range (MHz) | byte 0x12 (Ant A) | bits 5:3 | MSi001 mode | Notes |
|---|---|---|---|---|
| 0.001 - <12 | 0x45 | 000 | 0xe1 AM | edge between 11.9 and 12.0 |
| 12 - <30 | 0x65 | 100 | 0xe1 AM | edge 29.5-30.0 |
| 30 - <60 | 0x55 | 010 | 0xe1 AM | edge 59.75-60.0 |
| 60 - <120 | 0x5d | 011 | 0x42 VHF | edge 119-120 |
| 120 - <250 | 0x7d | 111 | 0x44 B3 | edge 247.5-250 |
| 250 - <300 | 0x75 | 110 | 0xe1 AM | edge 297.5-300 |
| 300 - <380 | 0x4d | 001 | 0xe1 AM | first LO 144 MHz from ~375 |
| 380 - <420 | 0x6d | 101 | 0xe1 AM | first LO 168 MHz from ~395; edge 417.5-420 |
| 420 - <1000 | 0x5d | 011 | 0x48 B4/5 | |
| 1000 - 2000 | 0x7d | 111 | 0x50 L | edge 997.5-1000 |

Exact edges probably sit at the round numbers (12, 30, 60, 120, 250, 300, 380, 420, 1000 MHz) but were bracketed only to the step size. The API also rewrites bytes 0x12/0x13 at 0.5 and 2 MHz without a value change (gain-table boundaries [guess]).

### 1.10 Bulk mode hang

With the API's transfer mode set to bulk, init selected alt 3, wrote the usual registers, and the START request (0x43) completed with status -2; Init returned "Fail", the **API service then crashed** (libusb mutex assertion) and the **RSP stopped answering even GET_DESCRIPTOR** (kernel: descriptor read error -110 through several resets). Recovered by writing 1 then 0 to the RSP's root-hub port sysfs `disable` switch (RSP's port only); it re-enumerated with the firmware still resident (2.07) and worked normally. An open driver should use isochronous alt 1/2.

### 1.11 Control-to-bytes summary (RSP2pro)

| Control | Bytes that change |
|---|---|
| Centre frequency | TUNER r2 (and r3 bits); band change also r0 mode, B 12 bits 5:3, B 13 rewrite; AM-mode LO plan change adds calibration (1.6.6) |
| ADC clock / API sample rate | REG 03/04; REG 07 and alt setting when the format class changes; STOP/SET_INTERFACE/START around each change |
| IF mode | TUNER r0 bits 13:12; REG 07 bit 10 (decimate by 2) |
| IF bandwidth | TUNER r0 bits 16:14 |
| IF gain reduction | TUNER r1 bits 9:4 |
| LNA state | B 13 bits 5:2 and TUNER r1 bits 13:10 (table 1.8) |
| min GR "extended" | nothing by itself; allows r1 IF GR below 20 |
| AGC | host-side loop writing r1/B 13 |
| DC / IQ correction, DC cal mode, decimation | nothing (host DSP) |
| Antenna A/B | B 12 bits 1:0 |
| Hi-Z (AM port 1) | TUNER r0 mode 0xe1 -> 0x61 (below 60 MHz only) |
| RF (broadcast) notch | B 13 bits 7:6 |
| Bias-T | B 13 bit 0 (0 = on) |
| Ext reference output | B 12 bit 6 (0 = on) |
| ppm | REG 03/04 (ADC clock) and TUNER r3 |
| LO mode (120/144/168) | TUNER r0 bits 19:17, r2, calibration sequence |

### 1.13 Additions from the rsp_open driver work (2026-10-25 afternoon; placed before 1.12 so 1.12 stays next to its results)

Provenance: every item is either a fit of the captures above (decoded with our own scripts) or our own measurement with `rsp_open` on the running receiver (bias-T off, nothing connected, only requests from 1.4 sent). The full measurement log is `measure_2026-10-25.txt` in this directory.

1.13.1 **ADC clock divider and range code** [fit, sample-rate and IF/bandwidth captures]: `f = 4 MHz x (N + frac/2^21) / D`, D = 2(d+1). Observed D: 16 at 2.0/2.048 MHz, 12 at 3, 10 at 4, 8 at 5, 6 at 6 and 7, 4 at 8-10 MHz; every value fits "largest even D <= 16 with f x D in 32..40 MHz, else in 32..42 MHz". The fraction is rounded down (unlike the tuner PLL). Range code (reg 0x03 bits 15:14): 0 below 6 MHz, 1 at 6, 2 at 7-8, 3 at 8.192-10 MHz (edges between the observed rates are a [guess]). ppm divides the ratio by (1 + ppm/1e6) [fit, toggle capture].

1.13.2 **Alt setting and format choice** [fit, sample-rate and IF/bandwidth captures; hw]: the API picks the widest format whose block rate fits the endpoint (alt 2: one 1024-byte block per microframe = 8000 blocks/s; alt 1: 24000 blocks/s): alt 2 16-bit up to 2.016 MS/s, alt 2 12-bit up to 2.688, alt 1 16-bit to 6.048, 12-bit to 8.064, 384-format to 9.216, 8-bit above. Every observed (rate -> alt, reg 0x07) pair follows. Low-IF modes (decimate by 2): reg 0x07 = 0x414 on alt 2, 0x494 on alt 1, counter step 504 per 252-pair block (rsp_open: [hw], 0 gaps at 1, 3 and 4.096 MS/s output).

1.13.3 **Sample-rate change sequence** [obs, sample-rate capture]: STOP; ~300 ms; REG 03 = 011300; SET_INTERFACE; REG 07; REG 04; REG 03 (bit 12 clear); START; then TUNER r1 and B 13 rewritten. When the format and alt do not change (8.192 -> 9 MHz) only REG 04, REG 03. Retune sequences [obs, frequency-sweep and toggle captures]: inside one band only TUNER r3, r2; on a band, Hi-Z/A-B-port or gain-table change the full sequence TUNER r0, B 12, B 13 = a5, B 00, B 01, r3, r2, r13 = 0d, r14 = 0e, r1, B 13. Gain change: r1, B 13. Antenna A/B, ext-ref: B 12 only; notch, bias-T: B 13 only; bandwidth: r0 only. rsp_open reproduces these orders and streams through all of them with 0 counter gaps (80 operations while streaming, plus live 2 -> 6 -> 2 MHz rate changes) [hw].

1.13.4 **Stream formats confirmed on hardware** [hw]: 16-bit words with 14-bit signed samples (2-6 MHz), 12-bit packed (I in the low 12 bits of each 3-byte group; 2.048, 7-8 MHz), 384 format (6 groups x [16 runs of 10 bytes = 8 ten-bit values, then a 32-bit word with a 2-bit scale per run], 24 trailing bytes; 8.192-9 MHz) and 8-bit (10 MHz) all decode to the internal 24 MHz harmonic at the expected offset. Layouts of the two packed formats confirmed against libmirisdr-4's converters (GPL, facts only). The 250-375 MHz AM down-conversion is not spectrum-inverted (264 MHz harmonic at -200 kHz when tuned 264.2 MHz) [hw]. A single counter gap is sometimes seen within the first blocks after START [obs].

1.13.5 **Corrections to earlier sections**: (a) 1.6.2: the API's apparent truncation (frac 999 at 100 MHz) is the strictly-below rule of 1.6.4 together with r3 = 0xfff, not plain truncation. (b) 1.6.4: r3 is the fraction extension, no longer a guess. (c) 1.12: the verification program's 0.8 kHz spur offset came from rounding the fraction and leaving r3 constant; with the 1.6.4 rule the harmonics land at +199.951 kHz for +200 kHz (122 Hz bins) [hw]. (d) 1.4: 0x48 is not always 0x04 (1.13.6). The LNA table (1.8), band table (1.9) and control-to-bytes table (1.11) were reproduced exactly by rsp_open's unit tests and worked unchanged on hardware [hw].

1.13.6 **Request 0x48 value** [obs]: 0x04 in the API's opens, 0x09 in rsp_open's. The sequence before the read is identical (REG 08 = 00ee80, SET_INTERFACE). The difference between the two is the previous close: the API's close runs the VCO calibration (1.6.6) and ends with r0 = 063000 and r14 = 00800e; the verification program's and rsp_open's close do not. Hypothesis [guess]: 0x48 reports device or tuner state left by the previous session. Planned test (deferred, needs the RSP2pro): read 0x48 with rsp_open immediately after an API session has closed the device; 0x04 there would support the hypothesis.

1.13.7 **Measured front end** (rsp_open: [meas]; signals: the internal 24 MHz harmonics and the noise floor; nothing connected; uncertainties 1 sd of 3 captures, usually <= 0.2 dB):
- The board front-end steps (B 13 bits 5:2) and the notch change the noise floor (VHF: b5 -6.4, a5 -7.7, 95 -8.4 dB; notch -8.7 dB at 96 MHz, -20.0 dB at 1 MHz) but move the 24 MHz harmonic by < 1.1 dB, so the leak enters after them; these floor drops are lower bounds of the step and notch attenuation.
- MSi001 mixer gain-reduction bit (r1 bit 12): 18.3-19.3 dB in every band. LNA GR bit (r1 bit 13): 23.4 dB in VHF; 1-5 dB in B3/B4-5/L (lower bounds, leak partly after the LNA). AM buffer step 3 (bits 11:10 = 3): 8.3-9.5 dB (HF), 13.6 dB (264 MHz). Total spur range state 0 -> max: VHF 42.3 dB, HF 24 MHz 29.1, 264 MHz 32.7, 480 MHz 25.2, 1200 MHz 20.9.
- IF gain reduction (r1 bits 9:4): 1 dB per code, +-0.2 dB per 5 dB step over 20-59.
- Floor across band edges (300 kHz below / above, dBFS/Hz): 12 MHz -108.7/-108.0, 30 -110.9/-111.5, 60 -113.2/-109.4, 120 -106.4/-103.6, 250 -107.3/-112.9, 300 -116.2/-117.6, 1000 -113.8/-119.8 (+-0.3). Steps of 3-6 dB only where the MSi001 mode changes; filter shapes not measurable without a signal source.
- ADC: with maximum gain reduction the total noise is rate-independent (-65.4 dBFS at 2 MS/s, -65.3 at 6), giving ENOB about 10.6 bits (2 MS/s, 16-bit format), 10.4 (8 MS/s, 12-bit), 10.3 (9 MS/s, 384), 8.6 (10 MS/s, 8-bit), +-0.1 bit.
- DC offset mostly on Q, (+120..+170, -340..-450) in 16-bit left-aligned units at GR 30-59, larger at low GR; GR 0 clips on noise alone (1.7 % of samples) with nothing connected.
- Sample clock +1.65 +- 0.25 ppm against the NTP-disciplined host clock over 120 s (host error not included). The harmonics cannot measure the reference error (everything derives from the same 24 MHz).

### 1.12 Verification program

A small stand-alone C/libusb-1.0 program of our own (not published; neither pyusb nor rusb handled isochronous transfers well) implements only what this document describes: the open sequence of 1.5 with our own VHF PLL calculation (1.6.2), zero IF, 2 MHz ADC clock, alt 2, BW 1.536 MHz, IF GR 30, LNA state 0, Antenna A, bias-T/notch/ext-ref off; it parses the 1024-byte blocks itself (header, counter check, 252 int16 I/Q pairs). It refuses to run unless bcdDevice is 2.07 (it never sends 0x44/0x40). **No SDRplay software was running** (the API service was stopped first).

Result (2026-10-25), 262144 samples per tuning, **0 counter gaps**:

| Tuned | 24 MHz harmonic | expected offset | strongest peak found | level over floor |
|---|---|---|---|---|
| 95.8 MHz | 96 MHz | +200 kHz | +199.2 kHz | 33.7 dB |
| 95.9 MHz | 96 MHz | +100 kHz | +99.1 kHz | 34.5 dB |
| 96.3 MHz | 96 MHz | -300 kHz | -300.8 kHz | 33.5 dB |
| 72.3 MHz | 72 MHz | -300 kHz | -300.8 kHz | 39.9 dB |
| 119.8 MHz | 120 MHz | +200 kHz | +199.2 kHz | 38.1 dB |
| 100.0 MHz | none in band | - | only weak (+15 dB) clutter | - |

So the receiver's own 24 MHz reference harmonics land exactly where our tuning words put them, move the right way when retuned, and I/Q order is I then Q. (A weaker image at -3x the offset, e.g. -597 kHz for +199 kHz, is probably a mixer harmonic response [guess].) Our PLL rounds the fraction (100 MHz -> frac 1000) where the API truncates (999). The device was returned to standby and alt 0 afterwards.

## 2. RSP1A (USB 1df7:3000, API hwVer 255)

Status: complete as far as captures without a signal source allow (2026-10-25). Front end: the API service and our own API client only (section 0); SDRconnect was not used for this round. Same confidence tags as section 1. The receiver was first opened under capture from its cold state, so the round includes one firmware load (2.3; length only, as in 1.3).

### 2.1 Descriptors (read cold, `lsusb -v`)
Identical in layout to the RSP2pro [obs]: USB 2.0 high speed, class 0xff, MaxPower 400 mA, bcdDevice 2.00 cold / 2.07 warm, no strings cold, a 10-character serial string warm; interface 0 alt 0 (no endpoints), alt 1 isochronous EP 0x81 3 x 1024, alt 2 isochronous 1 x 1024, alt 3 bulk 512 (bulk not tried on the RSP1A, after the RSP2pro hang of 1.10).

### 2.2 API service
The API service had only a Unix datagram socket (the system log) and no TCP/UDP sockets while it drove the RSP1A [obs].

### 2.3 Firmware load [obs]
Vendor 0x44 wValue 0x0000 wIndex 0 wLength 4096, then 0x44 wValue 0x1000 wLength 1987: 6083 bytes in two transfers, the same block sizes as the RSP2pro (content not recorded or compared). Then REG 08 = 000080 and request 0x40 wValue 1; the device leaves the bus and re-enumerates about 0.75 s later at a new address with bcdDevice 2.07; the host reads strings 0 and 1 and the EEPROM block of request 0x46 at 0x40 (16 x 0xff).

### 2.4 Open/start/stop, MSi2500, tuner, stream
Same sequence as 1.5 with only these differences [obs]: request **0x48 returns 0x0d** (RSP2pro 0x04; possibly a board or hardware ID [guess]); the board bytes in the full tune are `B 12 = e4, B 13 = df, B 00 = 00, B 01 = 00`, and after the gain write **both** `B 12 = e4` and `B 13 = dd` are written (the RSP2pro writes only B 13 there). MSi2500 registers, the ADC clock formula (reg 0x03/0x04, including ppm), the format register 0x07 table, the alternate-setting choice, the block header and counter, low-IF decimate-by-2 and the host-side DC/IQ/decimation/AGC are **all identical to 1.7** (every value in the sample-rate and IF/bandwidth captures matches the RSP2pro's value for value). The tuner (MSi001-compatible) words r0/r1/r2/r3/r5/r6/r12-r14, band modes, IF/bandwidth fields, the AM-mode first-LO plans (120/144/168 MHz) and the calibration read 0x42 are also identical to 1.6. The close sequence is the RSP2pro's (STOP, B 13 = dd, r12 = 1c, r0 = 0, 0x42 read, r13/r14, r0 = 063000, r14 = 00800e, REG 03 = 091300, REG 05 = 000004, alt 0).

A second init inside the same API session (the IF/bandwidth capture re-initialises after each IF-mode change) omits the two 0x48 reads and REG 05/00/02 and starts at REG 08, SET_INTERFACE, TUNER r14, r5, REG 07, r6, r0 [obs]; `rsp_open` always sends the first-open form.

### 2.5 Board byte 0x12 (band filter + front-end attenuator) [obs, roles fit]
- bits 4:0: RF band filter, set by frequency (2.6).
- bits 7:5: front-end attenuator step selected by the LNA state: `111`, `110`, `101`, `011` (byte values 0xe_, 0xc_, 0xa_, 0x6_).
- No antenna select (one SMA port) and no reference-output control are offered by the API for this model.

### 2.6 Band switching (frequency-sweep and band-edge captures; edges bracketed to the step shown)
| RF range (MHz) | B 12 bits 4:0 (B 12 at LNA 0) | MSi001 mode | edge resolution |
|---|---|---|---|
| 0.001 - <2 | 00000 (e0) | 0xe1 AM | 1.95-2.0 |
| 2 - <12 | 00010 (e2) | 0xe1 AM | 11.9-12.0 |
| 12 - <30 | 00001 (e1) | 0xe1 AM | 29.75-30.0 |
| 30 - <60 | 00011 (e3) | 0xe1 AM | 59.75-60.0 |
| 60 - <120 | 00100 (e4) | 0x42 VHF | 119.75-120.0 |
| 120 - <250 | 10100 (f4) | 0x44 B3 | 249.5-250.0 |
| 250 - <300 | 10000 (f0) | 0xe1 AM (down-converting) | 299.5-300.0 |
| 300 - <380 | 01000 (e8) | 0xe1 AM; first LO 144 MHz from ~375 | 377.5-380.0 |
| 380 - <420 | 11000 (f8) | 0xe1 AM; first LO 168 MHz from ~395 | 417.5-420.0 |
| 420 - <1000 | 00100 (e4) | 0x48 B4/5 | |
| 1000 - 2000 | 10100 (f4) | 0x50 L | 999-1000 |

The tuner PLL words (r2/r3) at every captured frequency are those of the 1.6.4 rule, as on the RSP2pro. As on the RSP2pro, the API also rewrites the full tune sequence without a value change at some frequencies (0.5 and 50 MHz seen; gain-table boundaries [guess]).

### 2.7 Board byte 0x13 [obs]
Base value 0xdd (all off, LNA states 0-3). Active-low switches:
- bit 0: **DAB notch**, 0 = on (dd -> dc).
- bit 2: **FM/MW broadcast notch**, 0 = on (dd -> d9). One bit for both; the API sets it the same at 1, 10, 100, 200 and 450 MHz (no band gating in the bytes).
- bit 4: **bias-T**, 0 = on (dd -> cd). Toggled once on and off **with nothing connected**, then left off.
- bit 1: front-end step used by LNA states >= 4 (>= 3 in L band): 1 = set (dd -> df).
- bits 7, 6, 3 are 1 and bit 5 is 0 in every capture (function unknown).

Write order [obs]: FM/MW notch and bias-T write B 13 only. The DAB notch writes B 13 and then repeats the gain write (TUNER r1, B 12, B 13). Combined example: notch on, then LNA state 5 at 100 MHz gives `B 12 = c4, B 13 = db`; back to LNA 0, `B 12 = e4, B 13 = d9`.

The B 13 value written between B 12 and the direction registers in the full tune is 0xdf in every captured open and the B 13 value already in force (0xdd) in every captured retune and bandwidth change [obs]; all of those were made with LNA state 0 and both notches off, so "the value in force" is a fit for other states [fit].

### 2.8 LNA state tables (IF GR 40; entries are B 12 bits 7:5 code / B 13 bit 1 / MSi001 r1 bits 13:10) [obs, gain capture]
"-" = r1 bits 13:10 zero; mix = bit 12; LNA = bit 13; AM3 = bits 11:10 = 3. Codes: e = 111, c = 110, a = 101, 6 = 011.

| Band (MHz measured) | states | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| <60 AM (1, 10, 30, 55) | 7 | e/0/- | c/0/- | a/0/- | 6/0/- | 6/0/mix | 6/0/AM3 | 6/0/AM3+mix | x | x | x |
| 60-250 (100, 200) | 10 | e/0/- | c/0/- | a/0/- | 6/0/- | e/1/- | c/1/- | a/1/- | 6/1/- | 6/1/mix | 6/1/LNA |
| 250-420 AM (300) | 10 | e/0/- | c/0/- | a/0/- | 6/0/- | e/1/- | c/1/- | a/1/- | 6/1/- | 6/1/mix | 6/1/AM3 |
| 420-1000 (450) | 10 | e/0/- | e/0/LNA | c/0/LNA | a/0/LNA | e/1/- | e/1/LNA | c/1/LNA | a/1/LNA | 6/1/LNA | 6/1/LNA+mix |
| 1000-2000 (1500) | 9 | e/0/- | c/0/- | a/0/- | e/1/- | c/1/- | a/1/- | 6/1/- | 6/1/LNA | 6/1/LNA+mix | x |

(x = refused by the API, OutOfRange.) At 60-420 MHz states 8 and 9 use the tuner's mixer and LNA bits singly, not cumulatively. Every gain change (IF gain reduction or LNA state, including the host AGC's) writes TUNER r1, B 12, B 13 in that order [obs]. IF gain reduction (r1 bits 9:4, 20-59 dB; 0-19 with extended min-GR) and the host AGC behave as on the RSP2pro.

### 2.9 Control-to-bytes summary (RSP1A)
| Control | Bytes that change |
|---|---|
| Centre frequency | TUNER r2/r3; band change: r0 mode, B 12 bits 4:0, B 13 rewrite; AM-mode LO plan change: calibration (1.6.6) |
| Sample rate / IF mode / ppm / LO mode | as RSP2pro (1.11); the sample-rate sequence ends with the gain write r1, B 12, B 13 |
| IF bandwidth | TUNER r0, B 12, B 13 (value in force), B 00, B 01, r3, r2, r1, B 12, B 13 (no r13/r14) |
| LNA state | B 12 bits 7:5, B 13 bit 1, TUNER r1 bits 13:10 (2.8) |
| IF gain reduction | TUNER r1 bits 9:4 (then B 12, B 13 rewritten) |
| FM/MW notch | B 13 bit 2 (0 = on) |
| DAB notch | B 13 bit 0 (0 = on), then r1, B 12, B 13 |
| Bias-T | B 13 bit 4 (0 = on) |
| DC/IQ correction, decimation, AGC loop | nothing on the wire (host) / host-driven gain writes |

### 2.10 Standalone check
The stand-alone program of 1.12 with only the product ID and the two board bytes changed (B 12 = e4, B 13 = df then dd); it refuses unless bcdDevice is 2.07 (it never talks to a cold device and never sends 0x44/0x40). No SDRplay software was running. Result: 262144 samples per tuning, **0 counter gaps**; the 24 MHz harmonics exactly where predicted: 95.8 MHz -> +199.2 kHz (27.9 dB over the floor), 95.9 -> +99.1 kHz (28.7 dB), 96.3 -> -300.8 kHz (27.9 dB), 72.3 -> -300.8 kHz (35.2 dB), 119.8 -> +199.2 kHz (33.1 dB); 100.0 MHz shows no spur. (As in 1.12, that program rounded the PLL fraction; the 1.6.4 rule removes the 0.8 kHz offset.)

### 2.11 Differences from the RSP2pro
| Item | RSP2pro | RSP1A |
|---|---|---|
| USB ID / API hwVer | 1df7:3010 / 2 | 1df7:3000 / 255 |
| Descriptors, alt settings | as 1.1 | identical |
| Firmware load | 4096 + 1987 = 6083 bytes, 0x44 then 0x40 | identical block sizes and sequence |
| Request 0x48 reply (API opens) | 0x04 | 0x0d |
| Other vendor requests (0x40-0x46, 0x49 with wIndex 0x40) | as 1.4 | identical set; no new requests |
| MSi2500 registers, ADC clock, stream formats, header, decimate-by-2 | 1.7 | identical, value for value |
| Tuner registers, band modes, LO plans, calibration | 1.6 | identical |
| B 12 role | filter bits 5:3, antenna bits 1:0, ref-out bit 6 | filter bits 4:0, front-end attenuator bits 7:5 |
| B 13 role | front-end step bits 5:2, notch bits 7:6, bias-T bit 0 | DAB notch bit 0, FM/MW notch bit 2, bias-T bit 4, front-end step bit 1 |
| Gain write | r1, B 13 | r1, B 12, B 13 |
| B 13 in the middle of a full tune | 0xa5 | 0xdf at open, the value in force later |
| Band-filter edges (MHz) | 12, 30, 60, 120, 250, 300, 380, 420, 1000 | **2**, 12, 30, 60, 120, 250, 300, 380, 420, 1000 (extra filter below 2 MHz) |
| LNA states (HF / VHF / 250-420 / 420-1000 / L) | 9 / 9 / 9 / 6 / 6 (Hi-Z 5) | 7 / 10 / 10 / 10 / 9 |
| Antenna ports | A, B, Hi-Z | one port; no antenna control |
| Notches | one RF (broadcast) notch | FM/MW notch and a separate DAB notch |
| Reference output | yes (B 12 bit 6) | not offered |
| Bias-T | B 13 bit 0, active low | B 13 bit 4, active low |

### 2.12 Open questions (RSP1A)
Same as section 4 items 2-8 (they are common to both), plus: dB values of the B 12 bits 7:5 attenuator steps and of B 13 bit 1; whether the FM and MW notches are physically one filter or two switched together; the B 13 value mid-tune with LNA state >= 4 or a notch on (2.7); the function of B 13 bits 3, 5, 6, 7; exact edges (bracketed above); bulk mode untested; the request 0x48 value under `rsp_open`.

### 2.13 rsp_open on the RSP1A (2026-10-25)
`rsp_open` drives the RSP1A with the same chipset code as the RSP2pro and the RSP1A board layer of 2.5-2.9. Its unit tests reproduce the captured RSP1A byte sequences exactly: the open sequence, B 12 / r0 / r2 at 23 frequencies including the 2 MHz edge, every LNA state of 2.8 in eight bands (r1, B 12, B 13), the notch, DAB notch and bias-T toggles and the notch + LNA combination, ppm, the sample-rate and bandwidth sequences and the close. Like the RSP2pro path it sends only the requests of 1.4 that are marked observed, never switches the bias-T on in tests, and drives only a receiver at bcdDevice 2.07. Hardware tests are written; their results will be added here when they have been run on the receiver.

### 2.14 Low-IF tuning and bandwidth changes (both models) [obs]
Re-reading the IF/bandwidth captures of both models while writing the RSP1A path:
- In the low-IF modes the API offsets the tuner LO above RF: tuned 100 MHz, r2 is 0x215a92 (100.45 MHz) with the 450 kHz IF, 0x21a132 (101.58 MHz) with the 1.62 MHz IF and 0x2202f2 (102.048 MHz) with the 2.048 MHz IF, on the RSP2pro and on the RSP1A alike. So the low-IF LO is not the zero-IF LO, as the `rsp_open` work had assumed: it moves up by the IF (by 1.58 rather than 1.62 MHz in the 1.62 MHz mode, unexplained). This probably explains the unexpected line positions `rsp_open` saw in the low-IF outputs [guess]. `rsp_open` does not apply the offset yet (deferred; see the README).
- A bandwidth change is not "r0 only" (correcting 1.13.3): on both models the API writes r0, B 12, B 13 (RSP2pro 0xa5, RSP1A the value in force), B 00, B 01, r3, r2, r1 and then the gain bytes, without r13/r14. `rsp_open` uses this sequence on the RSP1A and keeps the hardware-verified r0-only write on the RSP2pro.

## 3. RSP1 (USB 1df7:2500)

Status (2026-10-25, boot-ROM mode only). Sources: our own measurements on a cold RSP1 (bcdDevice 2.00, no firmware, no SDRplay software running); no vendor traffic of a working RSP1 exists (the vendor API loaded firmware into it, 6008 bytes in two blocks of 4096 + 1912, then could not open it).
- Descriptors as the other models (bcdDevice 2.00 cold; the vendor load makes it 2.06). The Linux `msi2500` kernel driver binds to it at 2.00 (`/dev/swradio0`), i.e. the stock driver already uses the cold device. `rsp_open` detaches that driver for the run (libusb auto-detach) and the kernel driver rebinds afterwards.
- The RSP2pro open sequence (1.5) without any request 0x48 or 0x49 streams on the cold RSP1: 24 MHz reference spurs at the expected offsets (+199.95 kHz at 95.8 MHz etc.), 0 counter gaps. No expander is present or needed, so there are no board controls: one antenna port, no notch, bias-T or reference output.
- `rsp_open` (`Rsp::open_boot_rom_model_with(Model::Rsp1, ..)`) tunes with the RSP2pro band table and tuner words. Spurs, all as expected in offset, 0 gaps (single runs, `hil_boot_rom_rsp1_reference_spurs`): 23.8 MHz 55.9 dB, 71.8 MHz 54.9, 95.9 MHz 44.1, 96.3 MHz 43.5, 119.8 MHz 49.5, 264.2 MHz 55.9, 383.8 MHz 44.9, 407.8 MHz 52.2, 480.2 MHz 53.1, 1200.2 MHz 42.6. The RSP1's own front-end filtering was not measured (no signal source); gain steps other than the tuner's IF/LNA/mixer bits do not exist.
- A firmware-loaded RSP1 (2.06) is not opened by `rsp_open`.

## 4. Open questions

0. (rsp_open, 2026-10-25) Answered or narrowed: r3 role (1.6.4); ADC divider (1.13.1); format choice (1.13.2); front-end dB values partly (1.13.7); the low-IF LO offset (2.14). New: 0x48 = 0x09 under rsp_open (1.13.6); where RF lands in the low-IF outputs once the LO offset is applied; the 1.58 MHz offset of the 1.62 MHz mode (2.14).
1. (Superseded, see section 5.) A cold device (bcdDevice 2.00) does NOT need the firmware load for the receive path; it only lacks requests 0x48/0x49 (board control). Whether the board's power-on state passes usable signals on each band, and whether the expander can be reached without the firmware, are open.
2. Meaning of request 0x48 (RSP2pro: 0x04 in the API's opens, 0x09 in rsp_open's, 1.13.6; RSP1A: 0x0d in the API's opens, 2.4) and of the 4-byte 0x42 read (wIndex 0x10) used in VCO calibration; how its bytes map into r13/r14.
3. Header bytes 4-15 of each stream block.
4. dB values of the B 13 front-end steps and of LNA states (need a signal generator; no antenna may be connected).
5. Exact band edges (bracketed to 0.1-2.5 MHz).
6. TUNER r1 bits 16:14 (3 vs 4), r3 bits, r6, r12-r14 roles.
7. Why bulk mode (alt 3) hangs the device.
8. Whether fine tuning below the PLL step is done in the API's DSP (expected; not checked).
9. API options not exercised: ADS-B mode, sample-rate re-calibration flag, synchronous (sample-number-timed) updates, AGC set-point/attack/decay parameters, min-GR limits beyond one test.

## 5. Boot-ROM personality (bcdDevice 2.00, no firmware loaded) [meas]

Added 2026-10-25 (afternoon). Method: a cold RSP2pro (1df7:3010, bcdDevice 2.00, just plugged in, never opened by any SDRplay software), driven by our own programs (libusb, and `rsp_open` in boot-ROM mode). No SDRplay software was running, no firmware was uploaded (0x44 and 0x40 were never sent), nothing was connected to an antenna port, bias-T was never switched on. Everything below is bus behaviour of the device; nothing was disassembled or read from any vendor file.

5.1 **The ROM implements enough to receive.** On the cold device [meas]:
- Request 0x41 (MSi2500 register write, hence also the MSi001 tuner through register 0x09) is accepted and takes effect.
- SET_INTERFACE alt 1/2, requests 0x43 (start) and 0x45 (stop) are accepted; isochronous data flows with the block header and counter of 1.7, 0 counter gaps.
- With the open sequence of 1.5 minus the board bytes, the receiver tunes and streams: the internal 24 MHz harmonics appear at +199.95 kHz (tuned 95.8 MHz), +99.98 kHz (95.9) and -300.05 kHz (96.3), 48.6 / 49.3 / 48.6 dB over the floor, i.e. within one 122 Hz bin of the tuning words and at the same strength as on a firmware-loaded unit (1.12: 33 to 40 dB under a different FFT size and gain). Further harmonic tests, all 0 gaps: 23.8 MHz +199.95 kHz 52.9 dB; 71.8 MHz 53.3 dB; 119.8 MHz 51.3 dB; 264.2 MHz (AM down-conversion) -199.95 kHz 52.9 dB; 480.2 MHz 42.2 dB; 1200.2 MHz 46.6 dB. Sample rates, formats and IF modes are chip-level (MSi2500/MSi001) and were not re-tested cold.
- So **the firmware upload is not needed for the receive path** (tuner, ADC clock, sample rate, IF gain, streaming).

5.2 **What the ROM does not do** [meas]: requests **0x48** (one-byte read) and **0x49** (board expander write, 1.8.1) are never answered: every attempt times out (300 ms to 1 s), whatever value MSi2500 register 0x08 holds (0x00f380, 0x006080, 0x00ee80, 0x000080, 0x000000 tried) and with the alternate setting selected. Probing the other vendor IN requests 0x41, 0x43, 0x45-0x4f also times out. Both requests therefore appear to be provided by the downloaded firmware. Consequences for a cold unit: the **board is uncontrolled**. Antenna A/B/Hi-Z selection, the RF band filters, the notch(es), the bias-T, the external-reference output and the front-end attenuator steps (LNA state's board part) cannot be set; the board stays in its power-on state (expander pins are inputs at reset [ds], so the filter/antenna selection is whatever the board's pull resistors give; not measured; no signal source was connected). The tuner-side gain (MSi001 r1: IF gain, mixer, LNA bits) still works. A timed-out request does not wedge the device; a following request is answered.

5.3 **Request 0x42 on the ROM** [obs]: it answers. wValue is ignored; wIndex is a byte offset into a small area that repeats every 256 bytes (live tuner state, see 5.6 (c), not a fixed table): the non-zero bytes are 0x00 = b0, 0x01 = 36, 0x02 = 40, 0x10 = 11, 0x11 = 30, 0x12 = 42, 0x13 = 0d, 0x14 = 31, 0x1c = 02, all others 0x00 (offsets 0xfd..0xff read the wrapped bytes). A read returns the 4 bytes from that offset on. The values are identical on repeated reads. What they are (identity, option bytes, SPI read-back) is unknown. In the firmware-loaded VCO calibration (1.6.6) the read at wIndex 0x10 returns varying bytes, so that area is live data there.

5.4 **Is the expander reachable some other way? Tried, not shown.** The expander's write frame (control byte 0x40, register, data) is 24 bits, the width of an MSi001 word. Writing such words through MSi2500 register 0x09 (the tuner SPI) instead of request 0x49 ran, but did not change the receive spectrum measurably when the "band filter" byte was varied (0x5d / 0x7d / 0x45 at 95.8 MHz: reference-spur level within 1 dB) and disturbed the tuner (spur level fell from about 48 dB to about 18 dB), because the MSi001 sees the same words. No chip-select for the expander was found. Open: whether an unknown MSi2500 register or GPIO selects the expander's chip-select; request 0x49 and 0x48 are then only convenience wrappers of the firmware.

5.5 **Driver.** `Rsp::open_boot_rom_with` opens a device at bcdDevice 2.00, sends no 0x48, 0x49, 0x44 or 0x40 requests, and refuses the board controls (`set_notch`, `set_bias_t`, `set_ext_ref_out`, `set_dab_notch`, antenna changes) with `Error::Unsupported`. The hardware test is `hil_boot_rom_reference_spurs`; the unit test `boot_rom_sends_no_board_or_0x48_requests`. The RSP1A (1df7:3000) is accepted by the same code path but has not been tested cold (not on hand at this time); neither has the RSP1.

5.6 **375-420 MHz, the first-IF plans and the calibration handshake** [meas, fit]. Method: wire traffic only. (a) Fit: the 21 distinct (0x42 reply, r13, r14) triples in the existing captures were compared bit by bit (reply = 4 bytes, W = little-endian 32-bit word). r14 payload (bits 23:4) bit n equals W bit n+8 for n = 1..5; bits 7 and 9 equal W16 (or W14: the data cannot tell them apart); bit 10 = W17; bits 12..15 = W18..W21; bit 16 = W22 (or W13, same data); bit 17 = W25 (or the inverse of W24); bit 18 = 1; bits 0, 6, 8, 11, 19 = 0. r13 payload = 0x20 | bit 1 from W0; bit 0 is not explained by the reply (it varied with context). With only 21 triples several of these assignments are ambiguous and the fit is not confirmed. (b) Hardware: with the handshake written from this fit, and without it (r13 = 0x00000d, r14 = 0x00000e as for any tune), the receiver was tuned 383.8 MHz (144 MHz plan) and 407.8 MHz (168 MHz plan) on a cold RSP2pro: the 24 MHz harmonics (384, 408 MHz) appear at +200.0 kHz, 50-57 dB over the floor, in all four combinations tried (single runs each); 407.8 MHz with the 144 MHz plan gave 20 dB and a weak line, consistent with the vendor moving to the 168 MHz plan at about 395 MHz. So receiving works in this band without the handshake; the handshake's effect (VCO selection, phase noise, sensitivity) was not measured. (c) In a fresh device the 0x42 read at wIndex 0x10 returned `1130420d` before any write and `00000000` after the r12 = 0x1c, r0 = 0 steps; after a normal tune it returned `110e260d` / `100e260d`, i.e. it is live tuner state, not a fixed ROM table (this corrects 5.3's wording "static"). (d) `rsp_open` now tunes 375-420 MHz with first-LO plan 144 MHz from 375 MHz and 168 MHz from 395 MHz (edges bracketed only: plan change seen "from ~375" and "~395" in 1.6.1) and the filter codes of 1.9 / 2.6; it does not run the handshake. Hardware test: `hil_boot_rom_reference_spurs` (383.8 and 407.8 MHz).

5.7 **Hazard: do not write unknown registers.** An exploratory scan wrote 24-bit patterns into MSi2500 registers 0x0a-0x4f (looking for a way to reach the board expander, 5.4). During it the sample stream stopped (first seen at register 0x0f; registers 0x0e immediately before it had no visible effect; which write is responsible was not isolated). Afterwards the device, even after a USB reset and a root-hub port disable/enable (which only re-enumerates: the port's power is not switched), returned a receive path with no 24 MHz spurs (about 6 dB instead of 48 dB) and lower amplitude, and later returned no samples at all until it was physically unplugged. A physical power cycle restored it (95.8 MHz spur back at 42-49 dB). The cause of the wrong receive path is not isolated; "a register keeps a bad value across a USB reset" is the hypothesis that fits. A libusb reset of the wedged device hung re-enumeration. One further episode of zero samples right after the power cycle did not reproduce (three runs in a fixed order were fine afterwards) and is unexplained. The driver therefore writes only registers documented here and never issues a USB reset.

5.8 **RSP1A cold** [meas]. A cold RSP1A (1df7:3000, bcdDevice 2.00) streams with the RSP2pro's cold open sequence (no 0x48, no 0x49): 24 MHz spurs at the expected offsets in every band tried (23.8 MHz to 1.2 GHz including both 375-420 MHz plans, 40-59 dB, 0 gaps; `hil_boot_rom_rsp1a_reference_spurs`). **However, in its first session after being plugged in, the same unit delivered only empty isochronous packets** (1600 packets, all status OK, 0 bytes) with that sequence, with the old sequence that also sent the timing-out 0x48/0x49 requests, with four values of register 0x08 (0x00ee80, 0x00f380, 0x006080, 0x00e080), at several tunings and through the Rust driver, over roughly twenty minutes. After one vendor API run (5.9: firmware load and open under capture), two unplug/replug cycles and one wedge (5.10), the cold unit streamed with every one of those same programs, including the old failing ones. **The cause of the earlier empty stream is not known.** Hypotheses, all untested: something the vendor session left in non-volatile state; a first-power-up condition of this unit that cleared; a power or port effect of the first connection. The earlier hypothesis that the sample clock is enabled through the board expander was wrong (5.9). If a cold RSP1A delivers only empty packets, a vendor firmware load followed by a physical unplug is the only known way to clear it.

5.9 **RSP1A: what the warm unit shows** [meas]. A vendor run was captured on the RSP1A (`b01_rsp1a_api`: firmware load 4096 + 1987 bytes, then the normal open; request 0x48 returned 0x1d in this open, 0x0d in an earlier one). Afterwards, on the warm unit (bcdDevice 2.07), the RSP2pro's open sequence streams normally in all of these cases, with the 24 MHz spurs at the expected offsets: (0) no board writes and no 0x49 at all, (1) after setting the expander's direction registers to all-inputs (its power-on state) with no other board write, (2) with the normal board writes. So the expander's output state does not gate the stream, which **refutes the simple form of the hypothesis in 5.8** (clock enabled through the expander); the firmware must set up something else, not visible as USB traffic, for sampling to start. (BM=1 leaves the output latches as they were; only the pin directions change, so a board with pull resistors on those nets could in principle still behave differently.)
The 0x42 read area (5.3) of the RSP1A: cold `... 10: 00 00 00 00 31 | 18: 0b | 1c: 02`, warm `... 18: 0f | 1c: ff`, and the tuner-state bytes at 0x10-0x13 are `00 00 00 00` in both (idle). On the RSP2pro cold they are `11 30 42 0d`, byte 0x18 is 0x00. Flipping the bits of documented registers one at a time (from 0) on the warm RSP1A: register 0x03 changes byte 0x02 (0x40 to 0x43 for any bit); register 0x05 with any of its low 8 bits set makes the whole area read zeros (bytes 0x00-0x02, 0x14, 0x18, 0x1c), so register 0x05 gates what that area shows; registers 0, 2, 4, 7 and 8 change nothing in it. These gave an oracle for the survey of 5.10.

5.10 **Register survey on the cold RSP1A** [meas]. After the unit streamed no samples (5.8, first session), writes of `ffffff` and then `000000` to undocumented MSi2500 registers 0x0a-0x20 (0x0f skipped) were compared against the 0x42 read area. Results: registers 0x0c and 0x0d set byte 0x1c of the area (`02` to `ff`; 0x0c = 0 clears it, 0x0d = 0 does not); registers 0x10 and 0x20 clear and set bit 5 of byte 0x14 (`31` / `11`); registers 0x0a, 0x0b, 0x0e had no effect on the area; **a write to register 0x21 made the device stop answering control requests (I/O error) until it was physically unplugged and replugged.** After the replug the unit streamed on its own (5.8), and setting registers 0x0c and 0x0d to `ffffff` before or after the open sequence made no difference to the stream or the spurs. Registers 0x22 and above were not tried. Hazard: do not write undocumented registers (5.7), 0x21 and 0x0f in particular.

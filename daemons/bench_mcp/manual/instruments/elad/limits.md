---
title: ELAD FDM receivers: limits
summary: Ranges, rates, what is not verified.
see_also: [instruments/elad/setup, instruments/elad/troubleshooting]
instrument: elad
verified: fixtures only
---

# ELAD FDM receivers: limits

* Rates: FDM-S2 192, 384, 768, 1536, 3072 and 6144 kHz; FDM-S1 up to 3072 kHz. At 3.072 and 6.144 MHz the stream is about 24.6 MB/s, near what USB 2.0 high speed carries; a slow or shared host may drop data (see `measured_rate_hz`).
* One call keeps at most 16 million samples (128 MB) and runs at most `max_capture_s`.
* **Not verified on hardware yet:** the I/Q byte format (32-bit little-endian components below 6.144 MHz, 16-bit at 6.144 MHz, I then Q), the tuning-word and front-end codes, the dBm estimate. The control path (identity, firmware, EEPROM) was read from a real FDM-S2; no sample has been received from one because its FPGA image was not loaded.
* The dBFS figures are relative to a full-scale complex tone; Welch averaging with a Hann window reads a tone between bins up to 1.4 dB low.
* The receiver has one USB host at a time: do not run another program (ELAD's, SDR software) on it while a tool runs.

---
title: SSA3000X overview
summary: What the SSA3021X driver is, which tools drive it, what each tier allows and what it deliberately leaves out.
see_also: [instruments/sa/setup, instruments/sa/limits, instruments/sa/operation]
tools: [bench_sa_status, bench_sa_sweep, bench_sa_marker, bench_sa_screenshot, bench_sa_tg_on, bench_sa_tg_off, bench_sa_tg_level, bench_sa_tg_normalize, bench_sa_tg_s21]
instrument: sa
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# SSA3000X overview

The driver was written against a Siglent SSA3021X (9 kHz to 2.1 GHz, tracking generator option installed), firmware 1.3.9.10, with the Siglent SSA3000X programming guide PG0703X E03D for the command syntax. It is for spectra, level and frequency readings, and scalar transmission (S21) measurements of filters, cables and amplifiers with the tracking generator. It uses only the base analyzer and the tracking generator (TG). **It does not use the Meas, EMI or CAT options**: on this unit they are trial options that lapse, so nothing depends on them (no channel power, ACPR, occupied bandwidth, EMI detectors or reflection mode).

| Tool | Tier | Does |
|---|---|---|
| [bench_sa_status](tools/bench_sa_status) | read_only | identity, options, generator state and level, sweep settings |
| [bench_sa_screenshot](tools/bench_sa_screenshot) | read_only | the screen as a BMP file |
| [bench_sa_sweep](tools/bench_sa_sweep) | read_config | set range, RBW, VBW, reference level, attenuation, detector, averaging; one sweep; CSV, summary and peaks |
| [bench_sa_marker](tools/bench_sa_marker) | read_config | marker 1 to 4: peak search or place, read frequency and level |
| [bench_sa_tg_on](tools/bench_sa_tg_on) | read_config | tracking generator output on at a level |
| [bench_sa_tg_off](tools/bench_sa_tg_off) | read_config | tracking generator off, read back to prove it |
| [bench_sa_tg_level](tools/bench_sa_tg_level) | read_config | tracking generator level, on or off unchanged |
| [bench_sa_tg_normalize](tools/bench_sa_tg_normalize) | read_config | store a through reference sweep |
| [bench_sa_tg_s21](tools/bench_sa_tg_s21) | read_config | S21 sweep in dB, against the through or the generator level |

The tracking generator is treated like any other setting of the analyzer: it outputs RF at the level set (0 dBm at most), the same tier applies, and there are no confirm flags. It is off unless a tool switches it on, and the S21 tools switch it off again afterwards unless told otherwise.

Not done on purpose: no raw-SCPI tool, no use of the USB connection (its replies are cut short), no change of the number of trace points (the unit fixes it at 751), no instrument-side normalization (the S21 tools normalize in software).

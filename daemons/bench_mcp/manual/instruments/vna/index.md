---
title: DG8SAQ VNWA 3E vector network analyser
summary: The VNWA 3E driven without the vendor program: sweeps, calibration, generator, frequency meter, recovery.
see_also: [concepts/errors, concepts/reserved-devices]
instrument: vna
verified: n/a (navigation)
---

# DG8SAQ VNWA 3E vector network analyser

A USB vector network analyser (DG8SAQ VNWA 3E) driven through the `vnwa_open` crate: its USB control interface and its two USB audio codecs, with no vendor software. The tools measure S21 and S11 as raw ratios (calibration is a separate, optional step), run the signal generator and the frequency meter, and recover the unit when it stops answering. Pages: overview, setup, operation (every function step by step), measurement-recipes (worked examples with expected results), limits, troubleshooting, protocol, and monitoring-and-recovery (what the unit reports about itself and what the driver does when a measurement fails). A page whose verified line says `in progress` is a placeholder.

A second driver, `nanovna`, puts a NanoVNA behind the same tools; see the NanoVNA page for what it supports and what is verified.

<!-- children -->

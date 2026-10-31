---
title: bench_vna_meter
summary: VNA frequency meter: the offset of the RX signal from a centre frequency within +/-25 Hz.
see_also: [tools/bench_vna_generator_off, instruments/vna/limits]
tools: [bench_vna_meter]
instrument: vna
tier: read_config
verified: loopback reads 0; sign and absolute accuracy not verified against a standard, 2026-10-29
---

# bench_vna_meter

Measures the frequency of the signal on the RX input within +/-25 Hz of `freq_hz`, by the phase of RX against the unit's own reference over about 1.7 s. The TX DDS runs at the centre during the measurement, so nothing else may use the unit, and a generator must be off.

## Arguments

<!-- arguments -->

* `freq_hz`: the centre frequency in hertz (the expected frequency of the signal). Required.
* `point_ms`: measurement time per point in milliseconds, 0.2 to 100, default 60.
* `instrument`: the VNA alias.

## Returns

`center_hz`, `tx_dds_hz`, `offset_hz` with `offset_codec1_hz`, `offset_codec2_hz` and `stderr_hz`, `frequency_hz` (centre plus offset), `measurement_s`, a `note` and `recovery` counts. A TX-to-RX loopback reads an offset of 0. The reference is the unit's 12 MHz TCXO; the sign convention (higher RX frequency is a positive offset) and the absolute accuracy are not verified against an external standard.

## NanoVNA

With a `nanovna` instrument this tool is not supported (it returns a typed error). See [the NanoVNA page](instruments/vna/nanovna).

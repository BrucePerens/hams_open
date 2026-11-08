---
title: bench_glasgow_spi_jedec_id
summary: Glasgow: read a SPI flash's JEDEC ID (opcode 0x9f). Full tier, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_spi_jedec_id]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_spi_jedec_id

Exchanges `9f 00 00 00` on CS, SCK, COPI and CIPO. The reply's bytes 1 to 3 are the manufacturer, memory type and capacity codes. No judgement: `all_ones` and `all_zeros` say only that the data line never moved.

## Arguments

<!-- arguments -->

## Returns

`{"reply_hex": "ffef4018", "manufacturer_id": "0xef", "memory_type": "0x40", "capacity_id": "0x18", "all_ones": false, "all_zeros": false}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan). Power the flash from its own supply or the Glasgow's port voltage only if the person says so; a chip in circuit may be held in reset by its host.

## Example

`bench_glasgow_spi_jedec_id {"cs": "A0", "sck": "A1", "copi": "A2", "cipo": "A3", "frequency_khz": 1000}`

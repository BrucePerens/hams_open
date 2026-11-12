---
title: bench_scope_decode
summary: Serial decode (I2C, SPI, UART, CAN, LIN): on, list, format and per-bus settings.
see_also: [tools/bench_scope_trigger, tools/bench_screenshot, instruments/scope]
tools: [bench_scope_decode]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_decode

Configures the scope's serial decode. `on` switches decode on or off (`DCST`); `bus` (1 or 2) with `list` (`OFF`, `D1`, `D2`) and `format` (`BIN`, `DEC`, `HEX`) set the decode list and number format (`DCPA`); `protocol` (`i2c`, `spi`, `uart`, `can`, `lin`) with `settings` configures that bus (`B<n>:DCIC`, `DCSP`, `DCUT`, `DCCN`, `DCLN`). Settings are the guide's parameters in lower case: i2c `dis`, `scl`, `sclt`, `sda`, `sdat`, `rw`; spi `dis`, `clk`, `clkt`, `edge`, `miso`, `misot`, `mosi`, `mosit`, `cstp`, `cs`, `cst`, `ncs`, `ncst`, `tim`, `bit`, `dlen`; uart `dis`, `rx`, `rxt`, `tx`, `txt`, `baud`, `dlen`, `par`, `stop`, `pol`, `bit`; can `dis`, `canh`, `canht`, `canl`, `canlt`, `src`, `baud`; lin `dis`, `src`, `srct`, `baud`. Sources are `C1` or `C2` (the digital channels need the MSO option), thresholds are numbers in volts. The guide has no query for the bus settings and no command that returns the decoded frames, so the tool returns only the decode state (`DCST?`); look at the result with `bench_screenshot`. Decode may be a licensed option; with no argument the tool only reads the state. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27.

## Returns

`sent` (the commands), `decode` (`DCST?` after them) and a `note`.

## Arguments

<!-- arguments -->

* `on`: true or false: decode on or off.
* `bus`: 1 or 2.
* `list`: `OFF`, `D1` or `D2`: the decode list.
* `format`: `BIN`, `DEC` or `HEX`.
* `protocol`: `i2c`, `spi`, `uart`, `can` or `lin` (needs `bus` and `settings`).
* `settings`: object of the bus's parameters in lower case.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

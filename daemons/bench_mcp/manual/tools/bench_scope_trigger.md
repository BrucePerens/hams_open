---
title: bench_scope_trigger
summary: Serial triggers (I2C, SPI, UART, CAN, LIN) and the pattern trigger.
see_also: [tools/bench_set, instruments/scope]
tools: [bench_scope_trigger]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_trigger

Sets the scope's serial trigger or pattern trigger. `serial` is `{bus, settings}`: the bus is `i2c`, `spi`, `uart`, `can` or `lin`; the trigger type is set to SERIAL (`TRSE SERIAL`) and each setting is written with the guide's `TR<BUS>:<PARAM>` command and read back. Settings are named as the guide's parameters in lower case with `_` for `:` (i2c: `scl`, `sda`, `con`, `addr`, `data`, `dat2`, `qual`, `rw`, `alen`, `dlen`; spi: `clk`, `clk_edge`, `clk_tim`, `mosi`, `miso`, `cstp`, `cs`, `ncs`, `trty`, `data`, `dlen`, `bit`; uart: `rx`, `tx`, `trty`, `con`, `qual`, `data`, `baud`, `dlen`, `par`, `pol`, `stop`, `bit`; can: `canh`, `con`, `id`, `idl`, `data`, `dat2`, `baud`; lin: `src`, `con`, `id`, `data`, `dat2`, `baud`). A source is `C1` or `C2` with an optional threshold (`C1,0.2V`); only the analogue channels are offered. A baud rate that is not a standard one is sent as `CUSTOM,<n>`. `pattern` is `{c1, c2, state}`: each channel `L`, `H` or `X`, and `state` `AND`, `OR`, `NAND` or `NOR` (`TRPA`). The earlier probe did not get these accepted with the tokens it tried; on the unit they may be licensed options, and the read-back shows what it accepted. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27.

## Returns

`sent`, `readback`, `trigger` (`TRSE?` after the change) and `settings` (each serial setting as the scope reports it).

## Arguments

<!-- arguments -->

* `serial`: `{bus, settings}` as described above.
* `pattern`: `{c1, c2, state}` as described above.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

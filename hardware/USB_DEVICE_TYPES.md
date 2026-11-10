# Hams USB device types (the sub-ID registry)

Every device in the hams family answers the vendor request `GET_INFO` (0x01) on endpoint zero with a record whose bytes 3 and 4 (little endian) are the **device type**, the sub-ID. This file maps each number to its driver, protocol, firmware source and licence statement, as `docs/proposals/HAMS_USB_DEVICE_PROTOCOL.md` section 3 requires. A type is added here only together with the open source (and open hardware) files for it. The USB identifier is not yet registered with pid.codes: until it is, development uses the test PID 1209:0001, and 0x7373 is not used (Bruce's decision; registering is a public pull request that needs his go-ahead).

| Type | Name | Firmware source | Host driver | Protocol | Licence |
|---|---|---|---|---|---|
| 0x0001 | Station I/O (relay: PTT, keyline, power switching) | `hardware/fx2_station_io/` | `daemons/radioctl/crates/radioctl-station/src/fx2.rs` | 0.2, `fx2_station_io/README.md` | AGPL-3.0-or-later (software); hardware licence not decided |
| 0x0002 | Bench instrument (GPIO, logic capture, I2C, SPI, UARTs, pulses) | planned, `hardware/fx2_bench/` (not written) | planned, `bench_mcp` `bench_fx2_*` tools (not written) | not defined yet | AGPL-3.0-or-later (software) |

Numbers are provisional placeholders until the pid.codes entry exists. 0x0000 is never a device. A new type takes the next free number.

## Rules between the types

* The relay's host driver accepts only 0x0001 and refuses every other type in a station role (`Info::require_station_io`, tested with a bench-type record). It also refuses a 0x0001 board that does not report a hardware transmit time-out, a heartbeat and release on USB reset.
* `bench_mcp` must refuse 0x0001 (and any type it does not know) when its FX2 driver exists, and may only use 0x0002.
* A host ignores bytes of `GET_INFO` it does not know, and refuses an unknown protocol major version.
* The station image must never gain bench-style features (arbitrary output control, raw I2C pass-through): its safety argument depends on being small and on pin functions fixed by the map.

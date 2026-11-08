---
title: Glasgow bench session
summary: What the owner connects, step by step, to verify the pin tools on real hardware with no risk to anything else.
see_also: [instruments/glasgow/limits, instruments/glasgow/troubleshooting, tools/bench_glasgow_port_set]
instrument: glasgow
verified: fixtures only
---

# Glasgow bench session

Prerequisite: `bench_glasgow_list` shows `high_speed: true` ([troubleshooting](instruments/glasgow/troubleshooting)); the udev rule is installed; `bench_glasgow_status` shows both ports at 0 V. Everything below uses only the Glasgow, jumper wires and the parts named. Nothing is connected to a radio or a target you care about. After each step run `bench_glasgow_safe_state`.

1. **Loopback, 3.3 V, no target.** Join A0 to A1 with one jumper wire. `bench_glasgow_port_set {"port":"A","volts_v":3.3,"confirm":true}`, then `bench_glasgow_gpio {"pins":"A0:1","actions":["A0=1","A1"],"confirm":true}` (expect `A1` true), `["A0=0","A1"]` (expect false). Expected: the readback agrees with the drive; `bench_glasgow_status` after the call shows port A measured about 3.3 V.
2. **Pattern, checked by the scope.** Remove the jumper. Scope probe on A0 and ground. `bench_glasgow_pattern {"pins":"A0","steps":[{"value":1,"hold_ms":5},{"value":0,"hold_ms":5}],"repeat":300,"confirm":true}`. Expected: a square wave of about 100 Hz and 3.3 V on A0 for 3 s (the period is only about 1 ms accurate), then A0 high-impedance.
2b. **Logic capture of a known signal.** Feed A0 from the scope's probe-compensation output (about 1 kHz, 3 V; or a function generator at 3.3 V) and run `bench_glasgow_logic_capture {"pins":"A0:0","duration_s":2}`. Expected: `rising_edge_frequency_hz` about 1000 and `high_fraction` about 0.5. (The capture cannot run during the pattern: one session at a time.)
3. **UART loopback.** Join A1 (TX) to A0 (RX). `bench_glasgow_uart_transact {"send_text":"hello","timeout_s":1,"confirm":true}` expect `hex` 68656c6c6f.
4. **I2C.** Connect an I2C EEPROM or sensor breakout (3.3 V parts only, its own pull-ups or two 4.7 kOhm resistors from SCL and SDA to Vio, the Glasgow's port Vio pin) with SCL on A0, SDA on A1, GND to the Glasgow ground, Vio to the breakout's VCC. `bench_glasgow_i2c_scan`.
5. **SPI flash.** An 8-pin 25-series flash on a breakout, 3.3 V: CS A0, SCK A1, COPI A2, CIPO A3, Vio to VCC and to /WP and /HOLD, GND. `bench_glasgow_spi_jedec_id`.
6. **JTAG or SWD.** A known 3.3 V target: TCK A0, TMS A1, TDI A2, TDO A3 (SWD: SWCLK A0, SWDIO A1), GND. `bench_glasgow_jtag_scan` or `bench_glasgow_swd_idcode`.

Record what happened and update the `verified` lines of the pages with the date and what was run. Do not change a tier or a limit to make a step pass.

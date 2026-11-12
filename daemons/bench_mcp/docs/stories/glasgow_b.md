# The Glasgow Applet Tools Plan, Run and Read Back Every Pin Call, and Their Output Is Parsed Strictly

As the person (or the AI agent) at the bench with a Glasgow Interface Explorer, I want to run an I2C scan or read or write, a SPI flash ID read, a UART exchange or sniff, a logic capture, plain GPIO, a timed pattern, a JTAG scan or an SWD DPIDR read through `bench_glasgow_*` tools that refuse a bad argument before anything is sent, drive a pin only with the I/O voltage this server set and read back, leave every pin an input when the call ends, and give me structured numbers instead of the raw text the `glasgow` command prints. This story covers the three files that do that work in `daemons/bench_mcp/src/glasgow/`: `ops.py` (the helper script that runs inside the applet), `ops.rs` (the applet tool table, the planner, the one execution path and the result shaping) and `parse.rs` (pure parsers for what `glasgow` prints and writes). The module's other half (config, port set and safe state, the session record, the device lock, the runner) is `mod.rs` and `runner.rs`, covered elsewhere; the tool pages for the agent are in `manual/instruments/glasgow/` and the overview is the "Glasgow Interface Explorer" section of `README.md`.

The flow of one pin tool in one paragraph: `mod.rs::call_with` checks the tool name and the instrument's tier, then hands every applet tool to `ops::call`. `call` asks `plan` for a `Plan` (every argument checked, the applet command line built, nothing sent), refuses a driving call without `confirm`, and runs `execute`, which takes the device lock, requires that this server set the port voltage and that it has not expired, reads the device back, checks the USB link speed, runs the `glasgow` child through the runner, then parks every used port. `parse.rs` turns the child's output into values and `done` adds the standard fields to the answer. Tools that carry a Python payload (I2C, UART transact, GPIO pattern, SWD) run `ops.py` through `glasgow script`; the others run the stock `glasgow run <applet>` command.

Nothing here transmits on the air. The Glasgow's pins do reach the target circuit, so for every tool that can drive or pull a pin the entries say how the tier, the `confirm` flag, the port voltage set first by `bench_glasgow_port_set`, the `max_on_s` watchdog and the parking apply; there is no band check and no radio TX gate because no radio is involved.

Conventions: a "pin" is a port letter `A` or `B` and a number 0 to 7 (`A0`); a "pin spec" is `A0`, `A0:3` (A0 to A3) or a comma list; a "port" is the group of 8 pins that share one I/O supply. Voltages are volts (`f64`), times are seconds unless a name ends in `_ms` or `_ns`. Rust tests are cited as `glasgow::tests::<name>` (file `src/glasgow/tests.rs`, all driven by the simulated device in `src/glasgow/testkit.rs`); Python tests are `OpsTests` in `glasgow/test_glasgow_tools.py` and the harness `src/glasgow/test_ops_harness.py`. None touches a real device.

## The helper script (`ops.py`)

The script is compiled into the server (`include_str!`), written to `<out_dir>/bench_glasgow_ops.py` and loaded through a one-line entry file `<out_dir>/glasgow_entry.py` by `glasgow script`, which evaluates the entry inside the applet's context. Each operation is an `async` function `_op(ns, p)` taking `ns`, the context dictionary (`args`, `device`, `self` and the applet's interface object under the name `i2c_iface`, `uart_iface`, `gpio_iface` or `swd_iface`), and `p`, the JSON payload as a dictionary. It returns a dictionary that `main` prints. The script does not read files, open sockets or start programs. It does no argument checking of its own: the Rust planner has already bounded every field, and a payload that did not come from the planner is outside the contract.

### `glasgow_ops_i2c_scan` [@ANCHOR: glasgow_ops_i2c_scan]

`async def _i2c_scan(ns, p)`: finds which 7-bit addresses answer on the I2C bus by probing each with the applet's `ping`, which is an address phase with no data bytes (an empty write). Run by the tool `bench_glasgow_i2c_scan` after the planner has built the payload; the operator reaches it only through that tool.

| Argument | Type | Meaning |
|---|---|---|
| `ns` | dict, borrowed | Applet context; only `ns["i2c_iface"]` is used. |
| `p["lo"]` | int | First address to probe, 8 to 119 (checked by the planner, default 8). |
| `p["hi"]` | int | Last address to probe, inclusive, 8 to 119, not below `lo` (default 119). |

Returns `{"addresses": [int, ...], "addresses_hex": ["0x50", ...], "range": ["0x08", "0x77"]}`, the addresses that acknowledged in ascending order (an empty list when nothing answered, which is a normal result) and the probed range as hex text. An exception from the interface (for example a bus fault) is not caught here: the script fails and the Rust side reports that the applet exited non-zero with the pins parked.

Side effects and safety: it drives SCL and SDA (open-drain; the pull-ups are the target's) for up to 112 address probes. A probe can upset a device that does not expect one; the tool text says so. It has no loop bound beyond the address range, and the whole call is bounded by the Rust runner's timeout. It runs only after `execute` has checked the port voltage and taken the device lock, and `execute` parks the pins afterwards. It cannot be retried unsafely: a scan changes no state in a well-behaved target.

See also: test `glasgow::tests::the_helper_script_runs_through_its_entry_in_every_applet_context`; `glasgow_ops_main`, `glasgow_plan`.

### `glasgow_ops_i2c_read` [@ANCHOR: glasgow_ops_i2c_read]

`async def _i2c_read(ns, p)`: reads `count` bytes from one I2C device, first writing the optional register bytes in the same transaction so the bus does a repeated start (not a stop) between the write and the read, as EEPROMs and sensors expect. Run by `bench_glasgow_i2c_read`.

| Argument | Type | Meaning |
|---|---|---|
| `p["address"]` | int | 7-bit address, 8 to 119. |
| `p["register"]` | list of int | Bytes to write first (0 to 8 bytes, each 0 to 255). An empty list means read with no preceding write. |
| `p["count"]` | int | Bytes to read, 1 to 256. |

Returns `{"acknowledged": true, "hex": "<bytes as hex>", "count": n}` on success. A device that does not acknowledge, or any bus error, is a result and not a crash: `{"acknowledged": false, "error": "<ExceptionName>: <message>"}`. The `except Exception` is deliberately broad, so a programming error in the interface would also show as a not-acknowledged result with its name in `error`.

Side effects: drives SCL and SDA for one transaction; the register write is a write to the target (a device may treat a register pointer write as a command), but the planner caps it at 8 bytes and the tool is at tier Full. It does not itself require `confirm` (only `bench_glasgow_i2c_write` does), because a read is not meant to change the target. Parking and the voltage rules are the caller's.

See also: test `glasgow::tests::i2c_read_and_write_report_acknowledgement_and_writes_need_confirm`, `glasgow::tests::the_helper_script_runs_through_its_entry_in_every_applet_context`.

### `glasgow_ops_i2c_write` [@ANCHOR: glasgow_ops_i2c_write]

`async def _i2c_write(ns, p)`: sends one write transaction to an I2C device. Run by `bench_glasgow_i2c_write`, the one I2C tool that changes the target, so `ops::call` has already required `confirm: true` from the person at the bench.

| Argument | Type | Meaning |
|---|---|---|
| `p["address"]` | int | 7-bit address, 8 to 119. |
| `p["data"]` | list of int | 1 to 256 bytes to send, the register address first when the device has one. |

Returns `{"acknowledged": true, "written": len(data)}` on success, or `{"acknowledged": false, "error": "<ExceptionName>: <message>"}` when the device did not acknowledge or the bus failed (a result, not a crash). Note that `written` is the number of bytes handed to the interface; the interface raises when a byte is not acknowledged, so a success means all were.

Side effects: writes to the target, which can change its configuration or memory. A write is not idempotent in general; the tool refuses to run without `confirm` and the agent must not retry blindly after an error, because the device may have taken part of the data before failing. Tier Full, port voltage set and read back, pins parked afterwards by `execute`.

See also: test `glasgow::tests::i2c_read_and_write_report_acknowledgement_and_writes_need_confirm`; `glasgow_ops_call` (the confirm rule).

### `glasgow_ops_uart_transact` [@ANCHOR: glasgow_ops_uart_transact]

`async def _uart_transact(ns, p)`: optionally sends bytes on the UART, then collects the reply until the first of four things happens and says which. Run by `bench_glasgow_uart_transact`. The applet is configured by the command line (baud, parity, stop bits, pins) and not by this payload.

| Argument | Type | Meaning |
|---|---|---|
| `p["send"]` | list of int | Bytes to send (flushed). Empty means listen only and drive nothing. The planner takes up to 4096 bytes from `send_hex`, then appends `send_text` bytes and the chosen line ending. |
| `p["until"]` | list of int | A terminator, up to 64 bytes. Empty means no terminator. |
| `p["timeout_s"]` | float | Overall deadline in seconds from the start of the call, 0.1 up to the config's `max_capture_s` (default 2). |
| `p["idle_ms"]` | float | Silence that ends the call once some data has been received, 1 to 10000 ms (default 200). |
| `p["max_bytes"]` | int | Most bytes to keep, 1 to 65536 (default 4096). |

Returns a dictionary: `sent` (bytes written), `received` (bytes kept, never above `max_bytes`), `hex` (the kept bytes), `ended_by`, `elapsed_s` (seconds, 4 decimals) and `baud_measured` (the applet's `get_baud()`). `ended_by` is one of `"until"` (the collected bytes end with the terminator), `"max_bytes"` (the byte limit was reached; extra bytes of the last chunk are dropped), `"idle"` (no data for `idle_ms` after the last chunk) or `"timeout"` (the deadline passed first, or nothing ever arrived). The idle limit applies only after the first chunk and only when it falls before the deadline: a device that is still sending at the deadline ends as `"timeout"`, not `"idle"`.

Concurrency and side effects: it awaits `read_all(flush=False)` bounded by `asyncio.wait_for`, so it never blocks the event loop longer than the remaining wait. Sending drives the TX pin at the port voltage, so `ops::call` requires `confirm: true` when `send` is not empty (a listen-only call needs none). Not idempotent when it sends. Other exceptions from the interface propagate and fail the script.

Example result: `{"sent": 3, "received": 6, "hex": "4f4b0d0a3e20", "ended_by": "until", "elapsed_s": 0.0312, "baud_measured": 115200}`.

See also: tests `OpsTests.test_a_device_still_sending_at_the_deadline_ends_by_timeout_not_idle`, `test_a_device_that_goes_quiet_ends_by_idle` and `test_a_silent_device_ends_by_timeout_with_nothing_received` in `glasgow/test_glasgow_tools.py`, and `glasgow::tests::uart_transact_sends_only_with_confirm_and_collects_the_reply`.

### `glasgow_ops_gpio_pattern` [@ANCHOR: glasgow_ops_gpio_pattern]

`async def _gpio_pattern(ns, p)`: plays a list of pin values with hold times on the `control-gpio` applet, timed from the host. Run by `bench_glasgow_pattern`.

| Argument | Type | Meaning |
|---|---|---|
| `p["first"]` | int | The value of the first step; its bits set the initial output level of each pin before timing starts. |
| `p["steps"]` | list of `[value, hold_ms]` | `value` is a bit mask over the pins (bit 0 is the first pin of the `pins` spec), `hold_ms` 0.5 to 5000. 1 to 256 steps. |
| `p["repeat"]` | int | Times the whole list runs, 1 to 100000. |
| `p["max_s"]` | float | A time limit for the run: the planner sets it to the computed total plus 2 s. |

Behaviour: every pin the applet owns (`gpio_iface.count`) is made an output at its `first` level, the clock starts, then for each repeat and step the whole word is set with `set_all(value)` and the script sleeps until the hold has passed. After each step it records a timestamp and raises `RuntimeError` if more than `max_s` has elapsed since the start. In a `finally` block every pin is released back to an input, so a failure or a timeout still ends with the pins undriven.

Returns `{"steps_run": n, "elapsed_s": seconds, "released": true, "note": "..."}`. `steps_run` counts executed steps over all repeats; `elapsed_s` runs from the start to the last step's end. The note states the accuracy: levels are set from the host, so timing is good to about a millisecond, not to the FPGA clock. When the time limit raises, no result line is printed and the call fails, with the pins released.

Safety: this drives up to 8 pins strongly at the port voltage, so the tier is Full, `confirm` is required, the planner refuses a pattern whose total time exceeds the config's `max_capture_s`, and `execute` parks the ports afterwards as well. A completed pattern leaves the pins as inputs; the port rail stays on until `bench_glasgow_safe_state` or `max_on_s`.

See also: tests `glasgow::tests::gpio_and_pattern_release_every_pin_even_when_the_run_fails` and `glasgow::tests::the_helper_script_runs_through_its_entry_in_every_applet_context`; `glasgow_plan` (the limits).

### `glasgow_ops_swd_idcode` [@ANCHOR: glasgow_ops_swd_idcode]

`async def _swd_idcode(ns, p)`: initializes the ARM Serial Wire Debug link with `swd_iface.initialize()` (the tool text names this the line reset and the JTAG-to-SWD switch sequences, then a read of DPIDR) and reports the debug port identification register. It reads nothing else from the target. The payload `p` is empty and unused. Run by `bench_glasgow_swd_idcode`.

Returns `{"dpidr": "0x2ba01477", "version": n, "min": bool, "designer": n, "partno": n, "revision": n}` (the hex value is an example): `dpidr` is the 32-bit register as `0x` and eight hex digits; `version`, `designer`, `partno` and `revision` are the register's numeric fields as the applet decodes them and `min` is its MIN bit. The meaning of the fields is ARM's; this code only passes them on.

Errors: a target that does not answer makes `initialize()` raise, which fails the script (the Rust side reports the applet's exit and that the pins were parked). Side effects: drives SWCLK and SWDIO. The switch sequence puts a dual JTAG/SWD target into SWD mode, which is a small change of state; the planner uses tier Full with a port voltage already set, and no `confirm` is required.

See also: tests `glasgow::tests::spi_id_jtag_and_swd_results_are_structured`, `glasgow::tests::the_helper_script_runs_through_its_entry_in_every_applet_context`.

### `glasgow_ops_main` [@ANCHOR: glasgow_ops_main]

`async def main(ns)`: the entry point that the generated entry file calls as `await _m.main(globals())`. It reads the operation name and the JSON payload from `ns["args"].script_args` (the two words after `--` on the command line), looks the operation up in the private table `_OPS` (`i2c_scan`, `i2c_read`, `i2c_write`, `uart_transact`, `gpio_pattern`, `swd_idcode`), awaits it, and prints exactly one line `BENCH_GLASGOW_RESULT <json>` with `flush=True`. `parse::result_line` on the Rust side finds that line; stdout may hold other lines, and the last such line wins.

| Argument | Type | Meaning |
|---|---|---|
| `ns` | dict, borrowed | The context `glasgow script` gives the entry: `args` (with `script_args = [op, payload_json]`) and the applet interface object the operation needs. |

Errors: an unknown operation raises `KeyError`, a malformed payload raises `json.JSONDecodeError`, and an operation that raises (see each entry) ends the script before the print; in each case no result line is printed and the Rust side reports "the helper script printed no result line (it did not finish)" or the applet's non-zero exit, with the pins parked. A device that does not acknowledge is the one failure reported as a result, by `_i2c_read` and `_i2c_write`. It has no timeout of its own; the Rust runner bounds the call.

See also: test `glasgow::tests::the_helper_script_runs_through_its_entry_in_every_applet_context`; `glasgow_parse_result_line`, `glasgow_execute` (which writes the files).

## The applet tools (`ops.rs`)

### `glasgow_applets` [@ANCHOR: glasgow_applets]

`pub fn applets() -> Value`: the body of the tool `bench_glasgow_applets`, which tells a model what the Glasgow module can run. Called by `mod.rs::call_with` at tier ReadOnly; it takes no argument, touches no device, takes no lock and never fails.

Returns a JSON object: `applets` (one entry per applet with `tool`, `applet` and the default `pins` by role, for example `{"scl": "A0", "sda": "A1"}` for `i2c-controller`), `not_provided` (the things this server will never offer: flash, factory, firmware changes, any applet outside the list, free-form command lines, and an I/O voltage set by a tool argument other than `bench_glasgow_port_set`), `ports` (`["A", "B"]`), `pins_per_port` (8) and a note that the first call of each pin configuration builds its gateware in 5 to 20 s and that `bench_glasgow_prepare` does that ahead of time. The list is a literal in the code, not generated from `SPECS`; whether it stays in step with `SPECS` is checked only by the test below. The eight applets are `i2c-controller`, `spi-controller`, `uart`, `uart-analyzer`, `analyzer`, `control-gpio`, `jtag-probe` and `swd-probe`.

See also: test `glasgow::tests::the_applet_list_names_every_tool_and_what_is_not_provided`; `glasgow_plan` (the actual defaults).

### `glasgow_pin_arg` [@ANCHOR: glasgow_pin_arg]

`fn pin(args: &Value, key: &str, default: Option<&str>) -> Result<(String, char), String>`: reads one pin argument, such as `scl` or `rx`, from the tool's arguments. Called by `plan` through its `role` helper for every applet whose pins are named by role.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool's JSON arguments. |
| `key` | `&str` | The argument name, used in error messages. |
| `default` | `Option<&str>` | The pin to use when the key is absent or null. `None` makes the argument required. |

Returns `(spec, port)`: the pin text as given (or the default) and its port letter `'A'` or `'B'`. Errors (all `String`): the value is not a string (`<key> must be a pin such as A0`); the key is absent with no default (`<key> is required (a pin such as A0)`); the text does not parse as a pin spec (the error of `parse::pins`, prefixed with the key); the spec names more or fewer than one pin (`<key> must be one pin`, so `A0:3` is refused). A pin of either port is accepted here, so roles may be on different ports; the later voltage check then applies to each. Pure: no I/O.

See also: test `glasgow::tests::applet_arguments_are_checked_before_anything_is_sent`; `glasgow_parse_pins`, `glasgow_distinct_pins`.

### `glasgow_distinct_pins` [@ANCHOR: glasgow_distinct_pins]

`fn distinct(pins: &[&String]) -> Result<(), String>`: refuses a pin that fills two roles of one applet (for instance `scl` and `sda` both `A0`). Called by `plan` for every applet with named roles (I2C, SPI, UART, JTAG, SWD) with the list of the pin texts recorded by `role`. Borrowed slice, no side effects.

Returns `Ok(())` when all pin texts differ, otherwise `Err("pin <p> is used for two roles")` naming the first repeat. It compares the text, not the meaning: `A0` and `A0:0` would not be recognised as the same pin, but `role` accepts only a single pin so that spelling is not reachable through `pin`. It is not applied to the multi-pin specs of capture, GPIO and pattern, whose duplicates `parse::pins` refuses.

See also: test `glasgow::tests::an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards`, `glasgow::tests::applet_arguments_are_checked_before_anything_is_sent`.

### `glasgow_plan` [@ANCHOR: glasgow_plan]

`fn plan(ctx: &Ctx, inst: &Instrument, name: &str, args: &Value) -> Result<Plan, String>`: turns a tool name and its arguments into a `Plan`, checking every argument first so that nothing reaches the device unless every value is legal. Called first by `ops::call` and by `prepare` (which feeds it defaults). It reads the instrument's `glasgow` config (limits) and `ctx.out_dir` (the capture file name) and does nothing else: no file is written, no process started.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Only `out_dir` is used, for capture file names. |
| `inst` | `&Instrument`, borrowed | The instrument entry: `glasgow.op_timeout_s`, `max_capture_s`, `max_capture_mb` limit the plan. |
| `name` | `&str` | One of the tool names in `SPECS`. |
| `args` | `&Value`, borrowed | The tool's JSON arguments. |

The returned `Plan` (private struct) holds: `tool` (a short label, `i2c`, `spi`, `uart`, `uart_sniff`, `logic`, `gpio`, `pattern`, `jtag`, `swd`), `applet` (the glasgow applet name), `opts` (applet options placed before the operation: pin flags, `-f`, `-b`, `-m`, `--parity`), `tail` (for a `run`, the operation words; for a capture, the output file path; for GPIO, the action words), `script` (for a payload tool: the helper operation name and its JSON payload), `ports` (the set of ports the pins are on), `needs_hs` (true when the tool streams over bulk endpoints and so needs a 480 Mb/s link; false for `gpio` and `pattern`), `timeout`, `stop_after` (the capture duration), `watch` (the capture file and its byte limit, `max_capture_mb` times 1,000,000), `hold` (how long GPIO levels are held) and `pins` (the pin texts for `distinct` and for the answer).

The ranges it enforces, by tool: I2C frequency 100 to 400 kHz (default 100), addresses 8 to 119, `to_address` not below `from_address`, read count 1 to 256, register 0 to 8 hex bytes, write data 1 to 256 bytes; SPI mode 0 to 3, frequency 10 to 8000 kHz, and the fixed exchange `9f 00 00 00`; UART baud 50 to 3,000,000, parity `none`/`even`/`odd`/`zero`/`one`, stop bits 1 to 8 (transact only), `send_hex` up to 4096 bytes plus `send_text` and an `append` of `none`, `cr`, `lf` or `crlf`, timeout 0.1 to `max_capture_s`, idle 1 to 10000 ms, `max_bytes` 1 to 65536, `until_hex` up to 64 bytes, and for sniff a duration of 1 to `max_capture_s` (default 5); logic capture 1 to 16 pins, optional comma `names` of letters, digits and underscores with one name per pin, `pull` of `none`/`up`/`down`, duration 0.1 to `max_capture_s` (default 2); GPIO 1 to 32 actions each `A0`, `A0=0`, `=1`, `=H`, `=L` or `=Z` on a listed pin, hold 0 to 30 s (and at most `max_capture_s`); pattern at most 8 pins, 1 to 256 steps, repeat 1 to 100000, `hold_ms` 0.5 to 5000, `value` within the mask of the pins, total run time within `max_capture_s`; JTAG and SWD frequency 10 to 4000 kHz. The timeout of a plan is the tool's own time plus the config's `op_timeout_s`.

Errors: every violation is an `Err(String)` that names the argument, the value and the range with its source ("the config's max_capture_s"); an unknown tool for this instrument is `<name> does not apply to <instrument>`. It does not check the tier (`call_with` does), the voltage (`volt_spec`) or `confirm` (`call`). Defaults for pins are A0 and A1 (and A2, A3 for SPI, JTAG).

Unknown: `send_text` has no length cap of its own in the code (only `send_hex` is capped at 4096 bytes), so the total `send` can exceed 4096.

See also: tests `glasgow::tests::applet_arguments_are_checked_before_anything_is_sent`, `glasgow::tests::an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards`; `glasgow_number`, `glasgow_pin_arg`, `glasgow_parse_pins`, `glasgow_parse_hex_bytes`.

### `glasgow_park` [@ANCHOR: glasgow_park]

`pub(crate) fn park(ctx: &Ctx, inst: &Instrument, port: char, release_pulls: bool) -> Result<(), String>`: puts all eight pins of one port into the "all inputs" state by running `glasgow run control-gpio --pins <port>0:7`, which loads a control-gpio gateware that drives nothing. Called by `execute` for every used port after every call (with `release_pulls` true), and by `port_set` in `mod.rs` before a rail comes up (false). The caller must hold the device lock.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner and paths. |
| `inst` | `&Instrument`, borrowed | The instrument; its serial selects the device and `glasgow.op_timeout_s` sets the time. |
| `port` | `char` | `'A'` or `'B'`. |
| `release_pulls` | `bool` | true (rail on): also append `P0=Z` to `P7=Z` for the port's pins to release every weak pull. false (rail off): do not, because pulls cannot be configured while the rail is off. |

Returns `Ok(())` when glasgow exited with code 0. Errors: the runner's error (a child that would not end, or timed out, via `run`), or `parking port <P> failed (glasgow exited <code>: <last 3 stderr lines>)` plus the "Next:" hint of `next_step`. The timeout is the config's `op_timeout_s` but never less than 60 s, since loading the gateware can take a while.

Side effects: starts one `glasgow` child, changes the gateware loaded on the FPGA and so the electrical state of the pins; it does not touch the port voltage, which stays as set. Callers treat a failure as serious: `execute` answers a parking failure with `fail_closed`, which switches every port off. Safe to repeat.

See also: test `glasgow::tests::an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards`, `glasgow::tests::a_hung_or_failed_applet_ends_with_the_pins_parked`; `glasgow_execute`, `glasgow_fail_closed`.

### `glasgow_volt_spec` [@ANCHOR: glasgow_volt_spec]

`fn volt_spec(inst: &Instrument, ports: &BTreeSet<char>) -> Result<String, String>`: builds the `-V` argument of a call, for example `A=3.30,B=1.80`, from the voltages this server recorded when `bench_glasgow_port_set` powered the ports. A tool never takes the I/O voltage from its own arguments; this is the only source. Called by `execute` (real runs and the dry run).

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `&Instrument`, borrowed | The instrument; its name keys the session record. |
| `ports` | `&BTreeSet<char>`, borrowed | The ports the plan uses. |

Returns the comma-joined `<port>=<volts with 2 decimals>` text in port order. Errors: a port with no record gives `port <p> has no voltage set by this server. Call bench_glasgow_port_set ...`; a port whose recorded deadline (`max_on_s` after it was set) has passed gives the special string `EXPIRED:<p>`, which `execute` recognises and turns into a `fail_closed`, so an expired port is switched off and the call refused.

Concurrency: it reads the process-wide session map through `with_session`, which holds the map's mutex only for the lookup. It has no side effects.

See also: test `glasgow::tests::an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards`, `glasgow::tests::a_port_past_its_max_on_s_is_switched_off_by_the_next_call`; `glasgow_sessions`, `glasgow_port_set`.

### `glasgow_execute` [@ANCHOR: glasgow_execute]

`fn execute(ctx: &Ctx, inst: &Instrument, p: &Plan, dry: bool) -> Result<Result<(Out, Vec<String>), Value>, String>`: the one path every applet tool takes, and the place where the safety rules live. Called only by `ops::call`.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner, sysfs root, output directory, lock file path. |
| `inst` | `&Instrument`, borrowed | Instrument with its config. |
| `p` | `&Plan`, borrowed | The plan from `plan`. |
| `dry` | `bool` | true: build and return the description of the call and send nothing. |

Return value: `Ok(Err(json))` is the dry-run answer (`dry_run`, `sent: []`, `would_run` with the command line, `ports_needed`, `ports_set_by_this_server`, `then`, `note`); `Ok(Ok((out, args)))` is a finished run with the child's `Out` and the argument list it ran; `Err(text)` is a refusal or failure. The dry run takes no lock, starts nothing and shows `<port>=<not set>` for a port without a voltage.

The steps of a real run, in order: (1) take the device lock (`device_lock`, up to 2 s wait; a second session is an error); (2) `volt_spec` for the plan's ports, an expired port becoming `fail_closed`; (3) read the device back with `read_ports` and for every used port require the supply within `max(0.15 V, 5 %)` of the recorded voltage, the limit register at most 0.05 V above it, and neither fault nor short; any disagreement is `fail_closed` (all ports off, proven, session cleared); (4) when the tool needs high speed, look at the link speed in sysfs: a 12 Mb/s link is refused with an error that names the cable, or, when the config has `full_speed_fallback`, the launcher `glasgow_fs.py` is written to the output directory and used, and the result is labelled; (5) for payload tools, write `bench_glasgow_ops.py` (the compiled-in `ops.py`) and `glasgow_entry.py` into the output directory, rewriting a file only when its content differs; (6) run `glasgow [--serial S] script <entry> <applet> -V <volts> <opts> -- <op> <json>` or `glasgow run <applet> -V <volts> <opts> <tail>` through `run`, with the plan's timeout, stop time and capture-file watch; (7) sleep the plan's hold (GPIO) when the run succeeded; (8) park every used port with `park`.

Failure outcomes: a failed run followed by a successful park returns the run's error plus "The pins were parked (inputs)."; any parking failure returns `fail_closed` text (the ports are switched off and read back at 0 V, or the error says to unplug the cable). The lock is held for the whole call, including the hold sleep, and released on return. Files written: the two helper files and optionally `glasgow_fs.py` in `ctx.out_dir`. Transmitter note: none; the pin tools drive the target's pins, never an antenna, and the rail stays on after the call until `bench_glasgow_safe_state` or `max_on_s`.

See also: tests `glasgow::tests::an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards`, `glasgow::tests::bulk_tools_refuse_a_full_speed_link_and_register_tools_do_not`, `glasgow::tests::the_full_speed_fallback_runs_bulk_tools_on_a_full_speed_link_and_labels_the_result`, `glasgow::tests::a_port_past_its_max_on_s_is_switched_off_by_the_next_call`; `glasgow_fail_closed`, `glasgow_device_lock`, `glasgow_read_ports`, `glasgow_run`.

### `glasgow_done` [@ANCHOR: glasgow_done]

`fn done(mut v: Value, p: &Plan, started: Instant) -> Value`: adds the standard fields to a tool's result so every answer says what state the device is in. Called by `ops::call` on the parsed result of every finished run.

| Argument | Type | Meaning |
|---|---|---|
| `v` | `Value`, owned | The parsed result. Only a JSON object is changed; any other value is returned untouched. |
| `p` | `&Plan`, borrowed | Supplies `applet` and `pins`. |
| `started` | `Instant` | When the call began; `call_s` is the time since then. |

Fields added: `applet` (the glasgow applet name), `pins` (the pin texts of the plan), `pins_state` ("parked: all inputs, pulls released"), `outputs_enabled` (always `true`; see below), `rails_note` ("the port voltage you set is still on until bench_glasgow_safe_state or max_on_s") and `call_s` (seconds, three decimals). Existing keys with those names would be overwritten. Pure.

Notes: `pins_state` is a statement that `execute` has already made true by parking. `outputs_enabled` is inserted as `true` for every tool including passive captures; its exact intended meaning is unknown to this entry, because the code gives no reason (it does not tell whether any output was driven).

See also: test `glasgow::tests::an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards`; `glasgow_ops_call`, `glasgow_execute`.

### `glasgow_ops_call` [@ANCHOR: glasgow_ops_call]

`pub fn call(ctx: &Ctx, inst: &Instrument, name: &str, args: &Value) -> Result<Value, String>`: runs any applet tool and returns its structured answer. Called by `mod.rs::call_with` for every tool that is not one of the module's own (list, status, applets, safe state, port set, prepare), after the tier check there.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner and paths. |
| `inst` | `&Instrument`, borrowed | The instrument entry; the tier was already checked. |
| `name` | `&str` | A tool name from `SPECS`. |
| `args` | `&Value`, borrowed | JSON arguments; `dry_run` and `confirm` are read here with `flag` (true, false or absent, nothing else). |

What it does: plan (every argument checked); decide whether the call drives or pulls pins or writes to the target; refuse it without `confirm` unless `dry_run`; run `execute`; collect `W:` and `E:` log lines; refuse a non-zero exit that was not a requested stop; parse the output; add the standard fields with `done`, plus `log_problems` and, on a full-speed link under the fallback, `full_speed_fallback: true` and a note about about 1 MiB/s throughput.

A call "drives" when the tool is `bench_glasgow_i2c_write` or `bench_glasgow_pattern`; `bench_glasgow_uart_transact` with a non-empty `send`; `bench_glasgow_gpio` with any action ending `=0`, `=1`, `=H` or `=L` (a read or `=Z` needs no confirm); or `bench_glasgow_logic_capture` with `pull` of `up` or `down`. The error then says to call with `confirm: true` only when the person at the bench has agreed. The scan, read, JEDEC ID, JTAG, SWD and sniff tools do not need `confirm`.

Results by tool: payload tools return the script's result dictionary (`glasgow_ops_*`); `bench_glasgow_spi_jedec_id` expects 4 bytes back, returns `reply_hex`, `manufacturer_id`, `memory_type`, `capacity_id` and `all_ones` / `all_zeros` (nothing drove the data line); JTAG returns `taps` and `tap_count`; GPIO returns `reads` (pin to bool, from `pin=value` lines), `held_s` and `released`; logic capture returns `file`, `file_bytes`, `summary` (the VCD summary), `overrun` (any `x` sample) and `stopped_by`; UART sniff returns `file`, `file_bytes`, `channels` (the UART CSV summary, at most 4096 bytes kept per channel) and `stopped_by`. Errors include the planner's, the confirm refusal, those of `execute`, "the <applet> applet exited <code>: <stderr tail> The pins were parked.", the missing result line, a SPI reply of the wrong length and an unwritten capture file.

Concurrency: blocking; one device session at a time through the lock in `execute`. A dry run is cheap and touches nothing.

See also: tests `glasgow::tests::an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards`, `glasgow::tests::i2c_read_and_write_report_acknowledgement_and_writes_need_confirm`, `glasgow::tests::uart_transact_sends_only_with_confirm_and_collects_the_reply`, `glasgow::tests::captures_write_a_file_and_return_a_summary`, `glasgow::tests::gpio_and_pattern_release_every_pin_even_when_the_run_fails`, `glasgow::tests::a_hung_or_failed_applet_ends_with_the_pins_parked`; `glasgow_plan`, `glasgow_execute`, `glasgow_done`, `glasgow_flag`.

### `glasgow_prepare` [@ANCHOR: glasgow_prepare]

`pub fn prepare(ctx: &Ctx, inst: &Instrument, args: &Value) -> Result<Value, String>`: the tool `bench_glasgow_prepare` (tier ReadConfig). Builds the FPGA gateware of one tool's applet and pin configuration into the install's cache ahead of use, so the first real call does not pay the 5 to 20 s build. It needs no voltage, no confirm and does not drive a pin.

| Argument | Type | Meaning |
|---|---|---|
| `args["tool"]` | string, required | `i2c`, `uart`, `uart_sniff`, `spi`, `logic`, `gpio`, `pattern`, `jtag` or `swd`. |
| pin arguments | as the tool | The same pin arguments the tool takes (`scl`, `sda`, `pins` and so on); `pins` defaults to `A0:3`, `gpio` gets a default `actions` of `["A0"]`, `pattern` a one-step default `steps`. |
| `args["revision"]` | string, optional | One of `A0`, `B0`, `C0`, `C1`, `C2`, `C3`, `D0`. Absent: taken from the configured serial or the single listed device (`revision` in `mod.rs`, which may run `glasgow list`). |

It maps the tool word to its tool name (the I2C one uses the scan tool), builds a `Plan` through `plan` to reuse the argument checks, drops the runtime-only options (`-f`, `-b` and `-m` with their values: the gateware does not depend on them nor on the I/O voltage), and runs `glasgow build --rev <rev> -t bitstream -f <scratch>/out.bin <applet> <pin options>` niced, with the config's `build_timeout_s`. The scratch directory `/dev/shm/bench_glasgow_build_<pid>` holds the unused output file and is removed afterwards; the build itself fills the install's cache `cache/GlasgowEmbedded/bitstreams`.

Returns `{"built": true, "applet", "revision", "build_s" (one decimal), "cache_bytes" (total size of that cache directory), "note"}`. The cache key covers the applet, its pins and the revision, so a later call with the same pins uses it. Errors: a missing or unknown `tool`, a bad `revision`, any planner error, a build that fails (the exit code and last four stderr lines with the "Next:" hint) or times out. It takes no device lock (the build uses no device).

See also: test `glasgow::tests::prepare_builds_the_gateware_without_the_device_or_a_voltage`; `glasgow_plan`, `glasgow_revision`, `manual/tools/bench_glasgow_prepare.md`.

## The parsers (`parse.rs`)

All of these are pure functions over text or a directory listing. They are tested against recorded output in `tests/glasgow_fixtures/`. The rule they share: text that cannot be read is an error or an absent value, never a guess.

### `glasgow_parse_num` [@ANCHOR: glasgow_parse_num]

`fn num(field: &str, what: &str) -> Result<f64, String>`: parses one cell of the voltage table as a finite `f64`. Called by `voltage_table` for every numeric column. `field` is borrowed, whitespace around it is ignored; `what` names the column for the error.

Returns the number. Errors: text that does not parse, or parses to NaN or an infinity (Rust would accept `NaN`, `inf` and `infinity`), gives `<what>: "<field>" is not a finite number`. This stops a misreading of the device's readback from passing a non-finite voltage into the comparisons that guard the port.

See also: test `glasgow::tests::the_voltage_table_of_a_real_device_parses_and_garbage_does_not`; `glasgow_parse_voltage_table`.

### `glasgow_parse_voltage_table` [@ANCHOR: glasgow_parse_voltage_table]

`pub fn voltage_table(text: &str) -> Result<Vec<PortRow>, String>`: parses the table that `glasgow voltage` prints (and, with fewer columns, `voltage-limit`). Called by `read_ports` in `mod.rs`, which every readback (status, port set, execute, safe) goes through. A row that does not parse is an error, never a guess: a voltage the server cannot read is one it must treat as unknown.

A row is a tab-separated line whose first field is `A`, `B`, `C` or `D`; every other line (header, log lines) is ignored. Fields: port, Vsupply, Vlimit, then optionally Isupply, Ialert, Vsense, with `(FAULT)` anywhere in the line.

| `PortRow` field | Type | Meaning |
|---|---|---|
| `port` | `String` | Port letter. |
| `vsupply_v` | `f64` | Measured supply voltage, volts. |
| `vlimit_v` | `f64` | The voltage-limit register, volts. |
| `isupply_a` | `Option<f64>` | Supply current, amperes. `None` for `n/a`, a missing or empty column, or a short. |
| `short` | `bool` | True when the Isupply column reads `(SHORT)`. |
| `ialert_a` | `Option<f64>` | Current alert threshold, amperes; `None` for `n/a`, missing or empty. |
| `vsense_v` | `f64` | Sensed voltage at the port, volts; `NaN` when the column is missing (the `voltage-limit` table). |
| `fault` | `bool` | True when the line contains `(FAULT)`. |

Errors: a row with fewer than 3 columns ("has too few columns"), a non-finite or unparsable number (from `num`), or no port row at all (the error quotes the first 200 characters of the text). Returns the rows in the order printed. The row struct derives `Clone`, `Debug` and `PartialEq` and is not serialized.

See also: test `glasgow::tests::the_voltage_table_of_a_real_device_parses_and_garbage_does_not`; `glasgow_parse_num`, `glasgow_read_ports`, `glasgow_execute` (which compares the rows to the recorded setting).

### `glasgow_parse_device_list` [@ANCHOR: glasgow_parse_device_list]

`pub fn device_list(stdout: &str) -> Vec<String>`: the serial numbers `glasgow list` prints, one per line on stdout. Log lines on the same stream start with `X: ` and are not serials, so each trimmed line is kept only if `serial_ok` accepts it. Returns the serials in order (empty when none). No error case. Called by `revision` and by the list tool in `mod.rs` to find the device when the config names no serial. Pure.

See also: test `glasgow::tests::serials_revisions_and_log_problems`; `glasgow_serial_ok`, `glasgow_revision`.

### `glasgow_serial_ok` [@ANCHOR: glasgow_serial_ok]

`pub fn serial_ok(s: &str) -> bool`: true when `s` has exactly the form of a Glasgow serial: a revision letter `A` to `D`, one digit, a dash, an eight-digit date, the letter `T`, six digits and the letter `Z`, 19 characters in all, for example `C3-20240904T183337Z`. This is the pattern Glasgow itself uses for `--serial`. Used by `device_list` and `revision_limits`, and so by the config check for a serial. Pure, no error case: anything else, including trailing whitespace, returns false.

See also: test `glasgow::tests::serials_revisions_and_log_problems`; `glasgow_parse_device_list`, `glasgow_revision_limits`.

### `glasgow_parse_log_problems` [@ANCHOR: glasgow_parse_log_problems]

`pub fn log_problems(stderr: &str) -> Vec<String>`: the warning and error lines of glasgow's log, so the caller sees them. A line counts when it starts with `W: ` or `E: `. Returns them unchanged, in order. Called by `ops::call` (the `log_problems` field of every answer) and by `read_ports`. No error case; an empty list means no such line, not that the run succeeded.

See also: test `glasgow::tests::serials_revisions_and_log_problems`; `glasgow_ops_call`.

### `glasgow_revision_limits` [@ANCHOR: glasgow_revision_limits]

`pub fn revision_limits(serial: &str) -> Option<(String, f64, f64)>`: the hardware revision a serial names and the port voltage range that revision allows, in volts. Revision A to C: 1.65 to 5.0 V (the voltage-limit register 1.8 to 5.0); revision D: 0.9 to 5.5 V, taken from `glasgow voltage --help` of the pinned install. Returns `None` when `serial_ok` rejects the text, otherwise `(revision, low, high)` such as `("C3", 1.65, 5.0)`; the revision is the first two characters. Called by `revision` in `mod.rs`, and through it by `port_set` to refuse a voltage outside the range (0 is always allowed, meaning off) and by `prepare` to default the build revision. Pure. The table has one branch for D and one for everything else, so a revision outside A to D cannot reach it.

See also: test `glasgow::tests::serials_revisions_and_log_problems`; `glasgow_revision`, `glasgow_port_set`.

### `glasgow_usb_device` [@ANCHOR: glasgow_usb_device]

`pub fn usb_device(root: &Path) -> Option<Value>`: finds the Glasgow among the USB devices the kernel lists and reports its link speed, so a full-speed link is known without opening the device. `root` is the sysfs directory of USB devices (`/sys/bus/usb/devices` in production, a fixture directory in tests; borrowed). It reads the directory entries in sorted order and the files `idVendor`, `idProduct`, `speed`, `product`, `manufacturer`, `serial`, `busnum` and `devnum` of each; the first device with `idVendor` 20b7 and `idProduct` 9db1 wins.

Returns `None` when the directory cannot be read or no device matches, otherwise a JSON object: `sysfs` (the entry name, for example `1-2`), `usb_id` (`"20b7:9db1"`), `product`, `manufacturer`, `serial` (strings or null), `bus` and `device` (numbers or null), `link_speed_mbps` (the `speed` file as a number, 1.5, 12, 480 or 5000, or null) and `high_speed` (true when the speed is at least 480, null when unknown). Callers: `execute` (a `high_speed` of `false`, not null, triggers the refusal or the fallback), `ops::call` (to label a fallback result) and the list tool. With two Glasgows attached only the first in sorted order is described, whatever the config's serial says. It only reads files; there is no device access and no locking.

See also: test `glasgow::tests::the_usb_device_and_its_link_speed_come_from_sysfs`; `glasgow_execute`, `glasgow_list`.

### `glasgow_parse_pins` [@ANCHOR: glasgow_parse_pins]

`pub fn pins(spec: &str) -> Result<Vec<(char, u8)>, String>`: expands a pin spec into a list of (port, number). A spec is one or more comma-separated parts, each `A0` (one pin) or `A0:3` (first to last on the same port); ports are `A` and `B` only and numbers 0 to 7. Whitespace around a part is ignored. Called by `plan` for the capture, GPIO and pattern pin specs and by `pin` for single pins. `spec` is borrowed.

Returns the pins in the order written, which is also the order of bit positions in a pattern value and the channel order of a capture. Errors: a part not of the form `A0`, `A0:3` or `B7` (the message shows the part), a range that runs backwards (`A3:1`), a pin listed twice anywhere in the spec, and a result of zero or more than 16 pins ("must name 1 to 16 pins"). A range stays on one port; to cover both ports write `A0:7,B0:7`. Pure.

Example: `pins("A0:1,B3")` returns `[('A', 0), ('A', 1), ('B', 3)]`.

See also: test `glasgow::tests::pin_specs_expand_and_bad_ones_are_refused`; `glasgow_pin_arg`, `glasgow_plan`.

### `glasgow_parse_jtag_taps` [@ANCHOR: glasgow_parse_jtag_taps]

`pub fn jtag_taps(stderr: &str) -> Vec<Value>`: reads the TAPs (test access ports, the devices on a JTAG chain) out of what a JTAG scan logs. Every line containing `TAP #` is one TAP, in a form such as `TAP #0: IR[5] IDCODE=0x4ba00477`; a TAP with no IDCODE says `BYPASS`. Called by `ops::call` on the stderr of `bench_glasgow_jtag_scan`. The log format is that of the pinned source, marked as such in the fixtures, not recorded from a device.

Returns one object per TAP: `tap` (the index, or null if unreadable), `ir` (the instruction-register length as text, or null), `idcode` (`"0x"` and eight hex digits, or null), `bypass` (true when the line contains `BYPASS`), and from the IDCODE the decoded fields `manufacturer_id` (bits 1 to 11, the JEDEC code), `part` (bits 12 to 27) and `version` (bits 28 to 31), all null without an IDCODE. An empty list when no line matches; there is no error case, so a scan that printed nothing parseable reads as zero TAPs and the caller adds `tap_count`.

See also: test `glasgow::tests::applet_log_lines_result_lines_and_hex_parse`, `glasgow::tests::spi_id_jtag_and_swd_results_are_structured`; `glasgow_ops_call`.

### `glasgow_parse_result_line` [@ANCHOR: glasgow_parse_result_line]

`pub fn result_line(stdout: &str) -> Result<Value, String>`: finds and decodes the one result line that the helper script prints, `BENCH_GLASGOW_RESULT {json}`. Called by `ops::call` for every tool that ran `ops.py`. It scans the output from the last line backwards and takes the first line with that prefix, so earlier noise cannot hide it and a later duplicate wins.

Errors: no such line ("the helper script printed no result line (it did not finish)"), or a line whose remainder is not valid JSON ("... is not JSON: <serde error>"); `ops::call` appends the last stderr lines and that the pins were parked. Returns the decoded `Value`. Pure.

See also: test `glasgow::tests::applet_log_lines_result_lines_and_hex_parse`; `glasgow_ops_main`.

### `glasgow_parse_hex_bytes` [@ANCHOR: glasgow_parse_hex_bytes]

`pub fn hex_bytes(s: &str) -> Result<Vec<u8>, String>`: converts hex text into bytes. Accepted forms: `48 65`, `4865`, `0x48,0x65`, and mixtures; groups are separated by whitespace or commas, a leading `0x` is dropped only at the start of a group (so `0x` inside a group is refused and not silently deleted), and the digits of all groups are then joined. Upper and lower case digits both work. Called by `plan` for `register_hex`, `data_hex`, `send_hex` and `until_hex`, by `ops::call` for the SPI reply and by `uart_csv_summary` for each CSV row.

Returns the bytes; the empty string gives an empty vector (so an absent optional argument is an empty list). Errors: an odd number of hex digits or any non-hex character gives `"<input>" is not hex bytes (an even number of hex digits)`. A group of an odd length is not refused by itself if the total is even, since digits are joined across groups (`4 8` reads as `48`). Pure.

Example: `hex_bytes("0x0f, 10")` returns `[0x0f, 0x10]`.

See also: test `glasgow::tests::applet_log_lines_result_lines_and_hex_parse`; `glasgow_plan`.

### `glasgow_vcd_summary` [@ANCHOR: glasgow_vcd_summary]

`pub fn vcd_summary(text: &str) -> Result<Value, String>`: summarises a VCD file (value change dump, a logic-analyzer file format) written by `glasgow run analyzer`. The numbers are what was captured, never a judgement. Called by `ops::call` for `bench_glasgow_logic_capture`; the bench_gla tools use their own parser in `src/gla/` on the same file. `text` is the whole file content, borrowed.

How it reads: the header's `$var` lines map each identifier to a pin name; after `$enddefinitions` a `#<number>` line sets the current time and a line starting with `0`, `1`, `x`, `X`, `z` or `Z` followed by an identifier is a value change of that pin. Anything else (vector values, `$dumpvars` and other `$` lines, unknown identifiers) is skipped. The time numbers are used as nanoseconds as they stand; the `$timescale` is not read, so a file with another timescale would be misreported (unknown whether the applet ever writes one).

Returns `{"duration_ns": last time minus first time, "pins": {name: {...}}}`. Per pin:

| Field | Type | Meaning |
|---|---|---|
| `transitions` | integer | Number of value changes after the first record of the pin (a change to or from `x` counts). |
| `high_fraction` | float or null | Time spent at 1 divided by the duration; null for a zero duration. The last level is carried to the last timestamp. |
| `min_pulse_ns`, `max_pulse_ns` | integer or null | Shortest and longest completed 0 or 1 interval; null when the pin never completed one. |
| `first_level`, `last_level` | text | `"0"`, `"1"`, `"x"` or `"z"` (lowercased) at the first and last record. |
| `rising_edge_frequency_hz` | float or null | From the mean period of rising edges, only when there are at least 3 rises and a positive span. |
| `unknown_samples` | integer | Count of `x` records, the marker of a FIFO overrun; `ops::call` turns any non-zero into `overrun: true`. |

Errors: a timestamp that is not a number (`bad timestamp`), and a file with no timestamp at all ("the VCD file has no timestamps (an empty capture)"). A file of reasonable size is processed in memory in one pass; the caller reads the whole file first, bounded by the config's `max_capture_mb`.

See also: test `glasgow::tests::a_vcd_capture_is_summarised_per_pin`, `glasgow::tests::captures_write_a_file_and_return_a_summary`; `glasgow_ops_call`.

### `glasgow_uart_csv_summary` [@ANCHOR: glasgow_uart_csv_summary]

`pub fn uart_csv_summary(text: &str, keep: usize) -> Result<Value, String>`: summarises the CSV that `glasgow run uart-analyzer` writes. Each non-blank line is `rx,<hex>` or `tx,<hex>` (received bytes on that line's channel), or `rx,#F` / `tx,#F` (a frame error) or `#P` (a parity error). Called by `ops::call` for `bench_glasgow_uart_sniff` with `keep` 4096.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str`, borrowed | The whole CSV file. |
| `keep` | `usize` | The most bytes per channel to put in `hex` and `text`; the rest are counted but not shown. |

Returns an object with one key per channel that appeared (`rx`, `tx`), each `{"bytes": total count, "truncated": bool, "hex": first keep bytes as hex, "text": the same bytes as text, "frame_errors": n, "parity_errors": n}`. In `text` printable ASCII is kept, newline, carriage return and tab are written as `\n`, `\r`, `\t` and any other byte as `\xNN`. A channel with no lines is absent, not zero. Errors: a line without a comma (`bad uart-analyzer line`), a channel other than `rx` or `tx`, or bad hex (from `hex_bytes`). Pure; the byte order is the order of the file.

See also: test `glasgow::tests::a_uart_capture_is_summarised_per_channel`, `glasgow::tests::captures_write_a_file_and_return_a_summary`; `glasgow_ops_call`, `glasgow_parse_hex_bytes`.

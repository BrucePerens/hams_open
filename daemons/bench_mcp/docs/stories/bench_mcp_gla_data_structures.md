# Data structures

### `Cur` [@ANCHOR: bench_mcp:bench_mcp_gla_decode_cur]

**Purpose and location.** The I2C decoder's transaction in progress (a function-local struct in `i2c`). Defined in `daemons/bench_mcp/src/gla/decode.rs:107` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `start` | `u64` | Time in nanoseconds of the START that opened the transaction. |
| `repeated` | `bool` | Whether it began with a repeated START. |
| `bytes` | `Vec<(u8, bool)>` | The bytes decoded so far as (value, acknowledged) pairs. |
| `nbits` | `u32` | Bits collected toward the next byte (nine clocks per byte; the ninth is the acknowledge). |
| `acc` | `u8` | The byte being shifted in. |

**Invariants, ownership and concurrency.** Local to one call of `i2c`, built and dropped inside it; never shared.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `i2c_transactions_are_decoded_with_addresses_acks_repeated_starts_and_stops (src/gla/tests.rs)`.

### `X` [@ANCHOR: bench_mcp:bench_mcp_gla_decode_x]

**Purpose and location.** The SPI decoder's transaction in progress (a function-local struct in `spi`): one chip-select window. Defined in `daemons/bench_mcp/src/gla/decode.rs:198` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `start` | `u64` | Time in nanoseconds when the window began. |
| `o` | `Vec<u32>` | The words decoded on the output line (COPI) so far. |
| `i` | `Vec<u32>` | The words decoded on the input line (CIPO) so far. |
| `ao` | `u32` | The output word being shifted in. |
| `ai` | `u32` | The input word being shifted in. |
| `n` | `u32` | Bits collected toward the next word. |

**Invariants, ownership and concurrency.** Local to one call of `spi`; never shared.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `spi_words_are_decoded_in_every_mode_and_bit_order (src/gla/tests.rs)`.

### `Spec` [@ANCHOR: bench_mcp:bench_mcp_gla_spec]

**Purpose and location.** The static description of one logic-analyzer tool: its name, tier, description, JSON schema properties and required argument names. The tool table is built from these. Defined in `daemons/bench_mcp/src/gla/mod.rs:19` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The tool name (`bench_gla_...`). |
| `tier` | `Tier` | The tier that must be enabled for the tool to exist. |
| `desc` | `&'static str` | The description shown to the model. |
| `props` | `&'static str` | The JSON-schema `properties` text of the tool's arguments. |
| `required` | `&'static [&'static str]` | Names of the required arguments. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced (src/gla/tests.rs)`.

### `Real` [@ANCHOR: bench_mcp:bench_mcp_gla_real]

**Purpose and location.** The production backend of the logic-analyzer tools: calls the Glasgow module's own `call` with the same instrument list. Defined in `daemons/bench_mcp/src/gla/mod.rs:69` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `0` | `&'a [Instrument]` | The configured instruments, borrowed. |

**Invariants, ownership and concurrency.** Implements `Backend`; stateless besides the borrowed list. Reaches real hardware only through the Glasgow module.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_production_backend_hands_the_call_to_the_glasgow_module (src/gla/tests.rs)`.

### `B` [@ANCHOR: bench_mcp:bench_mcp_gla_tests_b]

**Purpose and location.** A test helper that builds a waveform change by change. Defined in `daemons/bench_mcp/src/gla/tests.rs:13` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `w` | `Wave` | The waveform under construction. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/gla/tests.rs:18), `set` (src/gla/tests.rs:33), `i2c_bytes` (src/gla/tests.rs:681), `spi_frame` (src/gla/tests.rs:775).

**See also:** test `resampling_follows_the_wave_and_counts_the_pulses_it_cannot_show (src/gla/tests.rs)`.

### `Fake` [@ANCHOR: bench_mcp:bench_mcp_gla_tests_fake]

**Purpose and location.** A test backend that plays the Glasgow: a capture writes the VCD it was told to hold, and a pattern is rendered into the waveform the next capture returns, so a pattern run loops back into a capture. Defined in `daemons/bench_mcp/src/gla/tests.rs:1024` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `out` | `PathBuf` | The directory it writes files to. |
| `wave` | `RefCell<Option<Wave>>` | The waveform the next capture returns (behind a RefCell). |
| `calls` | `RefCell<Vec<(String, Value)>>` | Log of the calls received as (tool, arguments) (behind a RefCell). |
| `refuse` | `Option<&'static str>` | A fixed refusal text it answers with, if set. |

**Invariants, ownership and concurrency.** Plain data: its fields hold RefCell (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/nanovna.rs:704), `start` (src/nanovna.rs:903), `shell` (src/nanovna.rs:978), `fake` (src/gla/tests.rs:1077), `start` (tests/proc_e2e.rs:25).

**See also:** test `two_vnas_are_driven_at_once_by_device_name_each_with_its_own_tier (src/main.rs)`.

### `Stack` [@ANCHOR: bench_mcp:bench_mcp_gla_tests_stack]

**Purpose and location.** The Glasgow module's own tools over its simulated device: what `bench_gla_*` really sees in production, minus the USB cable. Defined in `daemons/bench_mcp/src/gla/tests.rs:1349` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `ctx` | `crate::glasgow::Ctx` | The Glasgow module's context. |
| `insts` | `Vec<Instrument>` | The instruments configured. |
| `sim` | `Arc<Sim>` | The simulated device (shared). |
| `dir` | `PathBuf` | The output directory. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `stack` (src/gla/tests.rs:1362).

**See also:** test `a_capture_after_the_port_is_set_is_decoded_and_the_pins_are_parked (src/gla/tests.rs)`.

### `Edge` [@ANCHOR: bench_mcp:bench_mcp_gla_wave_edge]

**Purpose and location.** One change of the pins: at `t_ns` the pins take `value` (bit n is pin n). Changes with the same timestamp are one entry. Defined in `daemons/bench_mcp/src/gla/wave.rs:13` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `t_ns` | `u64` | Time in nanoseconds. |
| `value` | `u32` | The pin state after the change, bit n for pin n. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `vcd_is_parsed_with_initial_state_coalesced_changes_and_the_overrun_marker (src/gla/tests.rs)`.
### `Plan` [@ANCHOR: bench_mcp:bench_mcp_glasgow_ops_plan]

**Purpose and location.** What a Glasgow tool will run, built and validated before any process starts: the applet, its options, the ports it uses, timeouts and the pin roles. Defined in `daemons/bench_mcp/src/glasgow/ops.rs:96` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `tool` | `&'static str` | The tool name. |
| `applet` | `&'static str` | The Glasgow applet name. |
| `opts` | `Vec<String>` | Options of the applet before the operation (pins, runtime settings). |
| `tail` | `Vec<String>` | For a `run`, the operation and its arguments; for a `script` empty. |
| `script` | `Option<(&'static str, Value)>` | For a `script`, the helper operation and its JSON payload. |
| `ports` | `BTreeSet<char>` | The I/O ports (letters) the plan drives. |
| `needs_hs` | `bool` | Whether it needs a high-speed USB link. |
| `timeout` | `Duration` | The whole call ends after this long. |
| `stop_after` | `Option<Duration>` | For a capture that runs until told to stop, SIGINT after this long. |
| `watch` | `Option<(std::path::PathBuf, u64)>` | A file and a size limit in bytes; the run is stopped when the file grows past it. |
| `hold` | `Duration` | How long the pins are held after the run. |
| `pins` | `Vec<String>` | Roles; the same pin must not fill two roles. |

**Invariants, ownership and concurrency.** Private, built per call and consumed by the runner path; it drives real pins when run, so the port voltage and park checks of the module apply before and after.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `plan` (src/gla/pattern.rs:56), `backend_args` (src/gla/pattern.rs:169), `plan` (src/glasgow/ops.rs:141), `done` (src/glasgow/ops.rs:735), `plan` (src/radioctl/control.rs:172).

**See also:** test `applet_arguments_are_checked_before_anything_is_sent (src/glasgow/tests.rs)`.

### `PortRow` [@ANCHOR: bench_mcp:bench_mcp_glasgow_parse_port_row]

**Purpose and location.** One row of `glasgow voltage`: Port, Vsupply, Vlimit, Isupply, Ialert, Vsense and Vsense alert. Defined in `daemons/bench_mcp/src/glasgow/parse.rs:12` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `port` | `String` | The port letter. |
| `vsupply_v` | `f64` | Supply voltage in volts. |
| `vlimit_v` | `f64` | Voltage limit in volts. |
| `isupply_a` | `Option<f64>` | None on a revision without current measurement ("n/a"), or a short ("(SHORT)", see `short`). |
| `short` | `bool` | The current column read "(SHORT)": a short circuit was reported. |
| `ialert_a` | `Option<f64>` | Current alert threshold in amperes, if given. |
| `vsense_v` | `f64` | Sensed voltage in volts. |
| `fault` | `bool` | Whether the row reports a fault (alert). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `read_ports` (src/glasgow/mod.rs:310), `row_json` (src/glasgow/mod.rs:359), `voltage_table` (src/glasgow/parse.rs:34).

**See also:** test `the_voltage_table_of_a_real_device_parses_and_garbage_does_not (src/glasgow/tests.rs)`.

### `Acc` [@ANCHOR: bench_mcp:bench_mcp_glasgow_parse_acc]

**Purpose and location.** The per-pin accumulator of the VCD summary (a function-local struct in `vcd_summary`). Defined in `daemons/bench_mcp/src/glasgow/parse.rs:262` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `transitions` | `u64` | Number of changes. |
| `high_ns` | `u64` | Total time spent high, in nanoseconds. |
| `min_ns` | `Option<u64>` | Shortest pulse, in nanoseconds. |
| `max_ns` | `u64` | Longest pulse, in nanoseconds. |
| `rises` | `Vec<u64>` | Times of the rising edges, for the frequency estimate. |
| `first` | `Option<char>` | The first level seen. |
| `last` | `char` | The last level seen. |
| `x` | `u64` | Count of `x` (overrun marker) values. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_vcd_capture_is_summarised_per_pin (src/glasgow/tests.rs)`.

### `Req` [@ANCHOR: bench_mcp:bench_mcp_glasgow_runner_req]

**Purpose and location.** One command line to run with its limits. Defined in `daemons/bench_mcp/src/glasgow/runner.rs:14` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `args` | `Vec<String>` | Arguments after the program name. |
| `timeout` | `Duration` | The whole call ends after this long whatever happens (SIGINT, then SIGTERM). |
| `stop_after` | `Option<Duration>` | For a capture that runs until told to stop: SIGINT after this long; the exit then counts as a success, not a timeout. |
| `watch` | `Option<(PathBuf, u64)>` | A file the program writes and a byte limit; SIGINT is sent when it grows past the limit so a capture of a busy bus cannot fill the disk. |
| `nice` | `bool` | Run at low priority (bitstream builds use every core for ten seconds). |
| `full_speed` | `Option<PathBuf>` | Run this launcher script with the install's Python instead of the `glasgow` command (the full-speed fallback). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run` (src/glasgow/mod.rs:282), `req` (src/glasgow/mod.rs:298), `run` (src/glasgow/runner.rs:44), `command` (src/glasgow/runner.rs:54), `run` (src/glasgow/runner.rs:94), `run` (src/glasgow/testkit.rs:103), `rq` (src/glasgow/tests.rs:345).

**See also:** test `the_real_runner_captures_output_stops_with_sigint_and_never_kills (src/glasgow/tests.rs)`.

### `Out` [@ANCHOR: bench_mcp:bench_mcp_glasgow_runner_out]

**Purpose and location.** What came back from a run. Defined in `daemons/bench_mcp/src/glasgow/runner.rs:31` as a pub `struct`. It derives Clone, Debug, Default, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `code` | `Option<i32>` | The exit code, `None` if it was killed by a signal. |
| `stdout` | `String` | Captured standard output. |
| `stderr` | `String` | Captured standard error. |
| `timed_out` | `bool` | Ended by this runner because `timeout` passed. |
| `stopped` | `bool` | Ended by this runner on purpose (`stop_after` or the size limit): the output is complete up to there. |
| `stuck` | `bool` | The child still ran after SIGINT and SIGTERM; its pid is named in `stderr` and nothing else was done to it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run` (src/glasgow/mod.rs:282), `run` (src/glasgow/runner.rs:44), `run` (src/glasgow/runner.rs:94), `ok` (src/glasgow/testkit.rs:93), `run` (src/glasgow/testkit.rs:103).

**See also:** test `the_real_runner_captures_output_stops_with_sigint_and_never_kills (src/glasgow/tests.rs)`.

### `RealRunner` [@ANCHOR: bench_mcp:bench_mcp_glasgow_runner_real_runner]

**Purpose and location.** The runner that starts the real `glasgow` program. Defined in `daemons/bench_mcp/src/glasgow/runner.rs:48` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `program` | `PathBuf` | Path of the `glasgow` program. |
| `cache_dir` | `PathBuf` | The cache directory it is given. |

**Invariants, ownership and concurrency.** Implements `Runner`; each `run` starts one child process and ends it, escalating SIGINT then SIGTERM and never SIGKILL (a stuck child is reported, not killed).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_real_runner_captures_output_stops_with_sigint_and_never_kills (src/glasgow/tests.rs)`.

### `State` [@ANCHOR: bench_mcp:bench_mcp_glasgow_testkit_state]

**Purpose and location.** The simulated Glasgow's state: ports, the calls made, faults to inject and scripted results. Defined in `daemons/bench_mcp/src/glasgow/testkit.rs:14` as a pub `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `ports` | `BTreeMap<String` | Port letter to (supply, limit, trip) voltages. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `volts` (src/fake_scope.rs:188), `pava` (src/fake_scope.rs:213), `num` (src/fake_scope.rs:235), `new` (src/fake_scope.rs:240), `set` (src/fake_scope.rs:271), `answer` (src/fake_scope.rs:320), `serialise` (src/fake_scope.rs:667), `restore` (src/fake_scope.rs:678).

**See also:** test `a_hung_or_failed_applet_ends_with_the_pins_parked (src/glasgow/tests.rs)`.

### `Sim` [@ANCHOR: bench_mcp:bench_mcp_glasgow_testkit_sim]

**Purpose and location.** A simulated Glasgow for tests: a state under a mutex and a temp directory. Defined in `daemons/bench_mcp/src/glasgow/testkit.rs:40` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `st` | `Mutex<State>` | The state, shared between the runner calls and the test. |
| `tmp` | `PathBuf` | The temp directory it writes files into. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `healthy` (src/vna.rs:598), `vna_sweep_ok` (src/vna.rs:757), `sim` (src/example/testkit.rs:69), `dropping_first_replies` (src/example/testkit.rs:140), `new` (src/glasgow/testkit.rs:53), `new` (src/radioctl/testkit.rs:217), `serve_sim` (src/radioctl/testkit.rs:418), `snap` (src/radioctl/tests.rs:1476).

**See also:** test `a_hung_or_failed_applet_ends_with_the_pins_parked (src/glasgow/tests.rs)`.

### `Rig` [@ANCHOR: bench_mcp:bench_mcp_glasgow_tests_rig]

**Purpose and location.** The test rig for the Glasgow tools: a simulated device, the context, the instruments and a directory. Defined in `daemons/bench_mcp/src/glasgow/tests.rs:19` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `sim` | `Arc<Sim>` | The simulated device (shared). |
| `ctx` | `Ctx` | The context built over it. |
| `insts` | `Vec<Instrument>` | The configured instruments. |
| `dir` | `PathBuf` | The output directory. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rig` (src/glasgow/tests.rs:26), `parse_description` (src/radioctl/models.rs:24), `list` (src/radioctl/models.rs:72), `preflight` (src/radioctl/txtools.rs:205), `restore` (src/radioctl/txtools.rs:307), `plan_json` (src/radioctl/txtools.rs:367).

**See also:** test `an_applet_tool_needs_the_port_set_uses_that_voltage_and_parks_afterwards (src/glasgow/tests.rs)`.

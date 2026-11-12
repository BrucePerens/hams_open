# Data structures

### `Limits` [@ANCHOR: bench_mcp:bench_mcp_procedure_backend_limits]

**Purpose and location.** The operating-system limits a user program runs under. Defined in `daemons/bench_mcp/src/procedure/backend.rs:17` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `memory_mb` | `u64` | Address space in MiB. |
| `cpu_s` | `u64` | CPU time in seconds. |
| `files` | `u64` | Number of open files. |
| `file_mb` | `u64` | Largest file the program may write, in MiB. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `limits` (src/nanovna.rs:273), `check_range` (src/nanovna.rs:387), `set` (src/psu.rs:66), `on_seconds` (src/psu.rs:119), `check_stored_setpoints` (src/psu.rs:138), `plan_on` (src/psu.rs:151), `lim` (src/psu.rs:255), `limits` (src/sdr.rs:109).

**See also:** test `limits_stop_runaway_memory_calls_and_output_and_numpy_scipy_work_inside_them (src/procedure/tests.rs)`.

### `SpawnSpec` [@ANCHOR: bench_mcp:bench_mcp_procedure_backend_spawn_spec]

**Purpose and location.** What a backend needs to start one run. Defined in `daemons/bench_mcp/src/procedure/backend.rs:30` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `run_dir` | `PathBuf` | The run's directory in the managed store: the backend finds `source.py`, `config.json` and `tools.json` there. |
| `scratch` | `PathBuf` | The program's working directory and the only place it may write. |
| `limits` | `Limits` | The resource limits to apply. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `start` (src/procedure/backend.rs:66), `start` (src/procedure/cpython.rs:223).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back (src/procedure/tests.rs)`.

### `Incoming` [@ANCHOR: bench_mcp:bench_mcp_procedure_backend_incoming]

**Purpose and location.** What `Session::recv` found. Defined in `daemons/bench_mcp/src/procedure/backend.rs:40` as a pub `enum`. It derives Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Message` | `Message(Value)` | A message (JSON value) from the program. |
| `Timeout` | `Timeout` | Nothing arrived within the wait. |
| `Closed` | `Closed` | The program closed its end: it has ended or is about to. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `recv` (src/procedure/backend.rs:52), `recv` (src/procedure/cpython.rs:290).

**See also:** test `the_wall_clock_budget_stops_a_run_and_a_program_that_ignores_the_request_is_killed_after_the_grace_time (src/procedure/tests.rs)`.

### `CPython` [@ANCHOR: bench_mcp:bench_mcp_procedure_cpython_cpython]

**Purpose and location.** The CPython backend: how the user program is started. Defined in `daemons/bench_mcp/src/procedure/cpython.rs:30` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `python` | `String` | The interpreter to run. |
| `wrapper` | `String` | `none`, or `systemd-run` to start the program inside a transient user scope with the memory and task limits. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `command_line` (src/procedure/cpython.rs:102).

**See also:** test `the_command_line_has_the_interpreter_flags_and_the_optional_systemd_wrapper (src/procedure/tests.rs)`.

### `PySession` [@ANCHOR: bench_mcp:bench_mcp_procedure_cpython_py_session]

**Purpose and location.** One running program: its child process, the pipe to it and the channel its messages arrive on. Defined in `daemons/bench_mcp/src/procedure/cpython.rs:36` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `child` | `Child` | The interpreter process. |
| `stdin` | `Option<ChildStdin>` | Its standard input, `None` once closed. |
| `rx` | `Receiver<Value>` | The channel on which a reader thread delivers the messages the program sent (JSON values). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back (src/procedure/tests.rs)`.

### `Example` [@ANCHOR: bench_mcp:bench_mcp_procedure_examples_example]

**Purpose and location.** One bundled example procedure. Defined in `daemons/bench_mcp/src/procedure/examples.rs:11` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | Its name. |
| `tier` | `&'static str` | The tier it needs. |
| `description` | `&'static str` | What it does. |
| `source` | `&'static str` | Its Python source text. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `find` (src/procedure/examples.rs:40).

**See also:** test `the_sweep_series_example_retries_power_cycles_and_tabulates_the_spread_on_the_mock_bench (src/procedure/tests.rs)`.

### `Params` [@ANCHOR: bench_mcp:bench_mcp_procedure_run_params]

**Purpose and location.** The parameters of a run after validation. Defined in `daemons/bench_mcp/src/procedure/run.rs:32` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `source` | `String` | The program text. |
| `args` | `Value` | The arguments given to the program (JSON). |
| `label` | `String` | A label for the run. |
| `cap` | `Tier` | The highest tier of tool the run may call. |
| `wall_s` | `f64` | Wall-clock budget in seconds. |
| `max_calls` | `u64` | The most tool calls the run may make. |
| `limits` | `Limits` | The OS limits. |
| `trace` | `bool` | Whether to log every call. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/audio/events.rs:26), `levels_for` (src/audio/events.rs:93), `detect` (src/audio/events.rs:118), `run_params` (src/procedure/mod.rs:296).

**See also:** test `run_arguments_are_checked_against_the_runner_before_anything_starts (src/procedure/tests.rs)`.

### `Rec` [@ANCHOR: bench_mcp:bench_mcp_procedure_run_rec]

**Purpose and location.** The persisted record of a run (`state.json`): state, timing, counters and who owned it. Defined in `daemons/bench_mcp/src/procedure/run.rs:44` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `state` | `String` | The state word (running, succeeded, failed and so on). |
| `label` | `String` | The run's label. |
| `cap` | `Tier` | Tier cap. |
| `started_unix` | `f64` | Start time, Unix seconds. |
| `ended_unix` | `Option<f64>` | End time, if ended. |
| `step` | `String` | The step name the program last reported. |
| `line` | `Option<u64>` | The source line the program was on, if known. |
| `last_call` | `Option<Value>` | The last tool call, if any. |
| `calls` | `u64` | Number of calls made. |
| `call_errors` | `u64` | Number of calls that returned an error. |
| `violations` | `u64` | Number of audit-hook violations (blocked operations). |
| `wall_s` | `f64` | The wall budget. |
| `max_calls` | `u64` | The call budget. |
| `pid` | `u32` | The program's process id. |
| `note` | `String` | A free note, for example why it ended. |
| `server_pid` | `u32` | The pid of the server that owns the run; a run left active by a dead server is marked lost and its instruments made safe. |
| `server_start` | `u64` | The server's start time, to tell a reused pid from the owner. |
| `paused_s` | `f64` | Seconds spent waiting for the operator (not counted against the wall budget). |
| `awaiting` | `Option<Value>` | The operator prompt the run waits on, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `runs_left_active_by_a_dead_server_are_marked_lost_and_their_instruments_made_safe (src/procedure/tests.rs)`.

### `RunCtx` [@ANCHOR: bench_mcp:bench_mcp_procedure_run_run_ctx]

**Purpose and location.** The live context of one run inside the server: its identity and directory, the configuration, the instruments and tool index, and the mutex-protected state shared by the supervisor and worker threads (record, event log, emitted values, recent calls, violations, cancel request, touched instruments, held locks, operator prompt and answer). Defined in `daemons/bench_mcp/src/procedure/run.rs:66` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | The run id (`r` and 14 hex digits). |
| `dir` | `PathBuf` | The run's directory in the managed store. |
| `cfg` | `Config` | The server configuration. |
| `insts` | `Arc<Vec<Instrument>>` | The configured instruments (shared). |
| `index` | `ToolIndex` | The tool index for tier checks. |
| `rec` | `Mutex<Rec>` | The persisted record, behind a mutex. |
| `log` | `Mutex<EventLog>` | The event log, behind a mutex. |
| `emitted` | `Mutex<Map<String, Value>>` | Values the program published with `emit`, by key (at most 500 keys of at most 64 KiB each), behind a mutex. |
| `last_calls` | `Mutex<VecDeque<Value>>` | The most recent calls, kept as evidence for a failure report, behind a mutex. |
| `hook_violations` | `Mutex<Vec<Value>>` | Operations the audit hook blocked, behind a mutex. |
| `cancel` | `Mutex<Option<String>>` | The reason for a cancel request, if one was made, behind a mutex. |
| `touched` | `Mutex<Vec<Value>>` | Instruments the run called, so their safe state can be run if it does not succeed, behind a mutex. |
| `held` | `Mutex<Vec<(String, File)>>` | The instrument locks the run holds (name and open lock file), behind a mutex. |
| `ask` | `Mutex<Option<Ask>>` | The operator prompt currently waiting, behind a mutex. |
| `answer` | `Mutex<Option<String>>` | The operator's answer, taken by the waiting run, behind a mutex. |

**Invariants, ownership and concurrency.** Shared by `Arc` between the supervisor thread and worker threads; every mutable field has its own mutex taken poison-tolerantly (`unwrap_or_else(|e| e.into_inner())`). When a run does not succeed the safe state is run for the touched instruments.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `registry` (src/procedure/run.rs:96), `lookup` (src/procedure/run.rs:104), `exec_call` (src/procedure/run.rs:291), `count_error` (src/procedure/run.rs:374), `reserve_op` (src/procedure/run.rs:383), `handle` (src/procedure/run.rs:414), `finish` (src/procedure/run.rs:660).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back (src/procedure/tests.rs)`.

### `Ask` [@ANCHOR: bench_mcp:bench_mcp_procedure_run_ask]

**Purpose and location.** An operator prompt a run is waiting on. Defined in `daemons/bench_mcp/src/procedure/run.rs:85` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `id` | `Value` | The id of the program's request, to answer it. |
| `since` | `Instant` | When the wait began. |
| `limit` | `Duration` | The longest the run waits for the operator; past it the run stops. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_prompt_nobody_answers_stops_the_run_at_the_operator_limit_and_a_cancel_ends_the_wait (src/procedure/tests.rs)`.

### `Internal` [@ANCHOR: bench_mcp:bench_mcp_procedure_run_internal]

**Purpose and location.** What a worker thread tells the supervisor. Defined in `daemons/bench_mcp/src/procedure/run.rs:92` as a private `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Reply` | `Reply(Value)` | A reply (JSON) to send to the program. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `handle` (src/procedure/run.rs:414).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back (src/procedure/tests.rs)`.

### `Rule` [@ANCHOR: bench_mcp:bench_mcp_procedure_safe_rule]

**Purpose and location.** One safe-state rule: which tool calls make a safe state necessary and which tool puts the instrument back in it. Defined in `daemons/bench_mcp/src/procedure/safe.rs:18` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `triggers` | `&'static [&'static str]` | Tools whose call makes the safe state necessary. |
| `arg_true` | `Option<&'static str>` | The call only counts when this boolean argument is true. |
| `arg_in` | `Option<(&'static str, &'static [&'static str])>` | The call only counts when this argument has one of these values. |
| `safe_tool` | `&'static str` | The tool that puts the instrument in its safe state. |
| `safe_arg` | `Option<(&'static str, &'static str)>` | An argument the safe tool needs, as (name, value). |
| `psu_gate` | `bool` | A supply rule: applies only when the runner's configuration allows it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_safe_state_rules_name_real_tools_and_only_count_calls_that_change_something (src/procedure/tests.rs)`.

### `EventLog` [@ANCHOR: bench_mcp:bench_mcp_procedure_store_event_log]

**Purpose and location.** The append-only list of a run's events. Each line is `{"seq", "t", "kind", ...}`; `seq` counts from 1 and is the cursor of `bench_proc_output`. Defined in `daemons/bench_mcp/src/procedure/store.rs:94` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `file` | `File` | The open events file. |
| `seq` | `u64` | The number of the last event written. |
| `bytes` | `u64` | Bytes written so far; past `EVENTS_CAP` events are counted and dropped, with a final marker. |
| `started` | `f64` | The start time, to compute each event's `t`. |
| `dropped` | `u64` | Events dropped because the file was full. |

**Invariants, ownership and concurrency.** Plain data: its fields hold File (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/procedure/store.rs:103).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back (src/procedure/tests.rs)`.

### `Bench` [@ANCHOR: bench_mcp:bench_mcp_procedure_tests_bench]

**Purpose and location.** The test bench for the procedure tests: instruments, a directory and two fake devices. Defined in `daemons/bench_mcp/src/procedure/tests.rs:12` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `insts` | `Vec<Instrument>` | The configured example instruments. |
| `dir` | `PathBuf` | The managed directory. |
| `gen` | `FakeDevice` | The fake generator. |
| `dmm` | `FakeDevice` | The fake multimeter. |
| `_extra` | `Vec<FakeDevice>` | More fake devices that must outlive the bench (the coupled mock bench's). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tune` (src/elad/measure.rs:21), `detect` (src/elad/measure.rs:56), `sweep` (src/elad/measure.rs:157), `open_tuned` (src/elad/mod.rs:370), `list` (src/elad/mod.rs:527), `identify` (src/elad/mod.rs:543), `capture` (src/elad/mod.rs:578), `spectrum` (src/elad/mod.rs:627).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back (src/procedure/tests.rs)`.

### `World` [@ANCHOR: bench_mcp:bench_mcp_procedure_tests_world]

**Purpose and location.** The mock bench of the example procedures: three example instruments whose fake devices share one simulated world. Defined in `daemons/bench_mcp/src/procedure/tests.rs:1711` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `level` | `f64` | The generator's programmed level. |
| `scope_drift` | `f64` | Drift added to the scope reading (the example tests a drifting reading). |
| `vna_broken` | `bool` | Whether the VNA is broken until it is power-cycled. |
| `vna_cycles` | `u32` | How many power cycles it got. |
| `vna_reads` | `u32` | How many reads it got. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_sweep_series_example_retries_power_cycles_and_tabulates_the_spread_on_the_mock_bench (src/procedure/tests.rs)`.

### `Coupled` [@ANCHOR: bench_mcp:bench_mcp_procedure_tests_coupled]

**Purpose and location.** A bench whose fake devices are coupled through a shared simulated world. Defined in `daemons/bench_mcp/src/procedure/tests.rs:1719` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `bench` | `Bench` | The bench. |
| `world` | `std::sync::Arc<std::sync::Mutex<World>>` | The shared world, behind a mutex. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `coupled_bench` (src/procedure/tests.rs:1724).

**See also:** test `the_sweep_series_example_retries_power_cycles_and_tabulates_the_spread_on_the_mock_bench (src/procedure/tests.rs)`.

### `ToolInfo` [@ANCHOR: bench_mcp:bench_mcp_procedure_tiers_tool_info]

**Purpose and location.** What the tier checks know about one tool. Defined in `daemons/bench_mcp/src/procedure/tiers.rs:14` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `tier` | `Tier` | The tier needed to call it. |
| `description` | `String` | Its description. |
| `schema` | `Value` | Its JSON input schema. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tool_registry` (src/manual.rs:336), `registered_tools` (src/manual.rs:362).

**See also:** test `tools_are_listed_by_tier_and_refused_above_it (src/procedure/tests.rs)`.

### `ToolIndex` [@ANCHOR: bench_mcp:bench_mcp_procedure_tiers_tool_index]

**Purpose and location.** The index of every tool by name with its tier, description and schema; used to refuse calls above the run's cap and to describe tools to the program. Defined in `daemons/bench_mcp/src/procedure/tiers.rs:20` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `tools` | `BTreeMap<String, ToolInfo>` | The tools by name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build` (src/procedure/tiers.rs:49).

**See also:** test `tools_are_listed_by_tier_and_refused_above_it (src/procedure/tests.rs)`.

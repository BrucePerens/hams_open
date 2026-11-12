# The Procedure Runtime Is the Program a Procedure Runs Inside: It Talks to the Host, Offers the `bench` Module and Keeps Accidents Out

As the person (or the AI agent) who writes a bench procedure, I want a small Python module called `bench` that lets my program call bench tools, save progress, wait for a person, hold instruments and fail with a useful report, and as the person at the bench I want a program that goes wrong, runs too long or reaches for a serial port to be stopped and explained, not to be left running unattended. This story documents `daemons/bench_mcp/src/procedure/bench_runtime.py`, the one Python file the CPython backend (`cpython.rs`) writes into each run's directory and starts as `python3 -I -B -u bench_runtime.py <run_dir>` in a fresh scratch directory with a stripped environment, process group and resource limits (`proc_cpython_start`). The Rust side (`run.rs`) is the supervisor on the other end of the pipe. The file reads `config.json`, `tools.json` and `source.py` from the run directory, installs an audit hook, builds the `bench` module, runs the procedure source and says how it ended. `python3 bench_runtime.py --check <source>` only compiles a source and lists the tools it names.

None of the functions here keys a transmitter or touches hardware. Everything a procedure does to an instrument goes through `bench.call`, which the host dispatches through the normal tool path: the tier cap of the run, the call budget, the instrument reservation, and for a transmitting tool the same tier, lock, band check and transmit gate as any other call. This file adds none of those checks and bypasses none of them. THE AUDIT HOOK IS NOT A SANDBOX: it catches accidents and tells the author to use `bench.call`; determined code can get around it.

The pipe protocol, seen from this side. Requests go out as one JSON line on a duplicate of the original stdout (`op` is `call`, `emit`, `log`, `step`, `checkpoint`, `reserve`, `release`, `ask`, `violation`, `trace`, `stdout`, `stderr`, `ready`, and the final `done`, `failed` or `cancelled`); a request that wants an answer carries an integer `id`. Replies (`{"op":"reply","id":..,"ok":..,"value"|"error","kind"}`) and `{"op":"cancel","reason":..}` come in on stdin, one JSON line each. Exit codes of the process: 0 normal (including a procedure that failed, which is reported on the pipe), 3 when stdin closes (the host is gone), 4 when the pipe to the host cannot be written, anything else a crash the host reports as `crash`.

Module state used by several entries: `STATE` is one dict per process (`cleanup` is true while the cleanup handlers run, `args` the procedure arguments, `started` and `wall_s` and `paused` the wall-clock budget in monotonic seconds, `cleanups` the registered handlers, `result` the value set by `bench.result`, `violations` the audit-hook count, `tools` and `tools_path` the lazily read tool listing, `trace_lines` and `trace_flushed` the trace buffer). `WIRES` is a list holding the one `Wire`. Constants: `USER_FILE` ("procedure.py", the file name the source is compiled under), `REPR_LIMIT` (200 characters of a variable's repr), `LOCALS_PER_FRAME` (30 variables), `FRAME_BYTES` (8192 characters of names and reprs per frame), `VIOLATION_LIMIT` (50 violation messages sent to the host), `DEVICE_ALLOWED` (the five `/dev` nodes a procedure may open), `PATH_EVENTS` (audit events that name a path to be checked, with the argument positions) and `BLOCKED` (audit events refused outright, with the wording used in the message). These have no anchor of their own; they are described here where they are used.

## Making a value fit in JSON

### `jsonable` [@ANCHOR: proc_py_jsonable]

The `default=` function of every `json.dumps` in this file. JSON cannot hold numpy values, sets or bytes, and a procedure result or `emit` value often contains them; this turns them into something JSON can hold instead of failing. It is called by `Wire.send` and by `execute` and `failure_report` for the result and error context, never by a procedure directly.

| Argument | Type | Meaning |
|---|---|---|
| `value` | any object `json.dumps` could not encode | Borrowed; not modified. |

Return value: for anything with a `tolist` method (numpy scalars and arrays) the result of `tolist()` (a plain number or nested list); for a `set` or `frozenset` (and a `tuple`, which `json` already treats as a list, so that case rarely arrives here) a list in the container's iteration order, which is not stable for sets; for `bytes` the lower-case hex string; for everything else `repr(value)`, a string. It raises only what `tolist()` or `repr()` raise. No I/O, no locks, no side effects. Note that NaN and infinity are not handled here: they are floats, and `json.dumps(..., allow_nan=False)` refuses them with `ValueError` before `default` is consulted.

See also: `proc_py_wire_send`, `proc_py_execute`. Tested in `src/procedure/tests.rs`.

## The exceptions a procedure sees

### `BenchError.__init__` [@ANCHOR: proc_py_bench_error_init]

Builds the exception `bench.call`, `bench.reserve`, `bench.release` and `bench.ask_operator` raise when the host answers `ok: false`. A procedure catches it as `bench.BenchError` (or `Exception`) to decide whether to retry, skip or stop; an uncaught one ends the run as failed with the error in the report.

| Argument | Type | Meaning |
|---|---|---|
| `tool` | `str` | The tool name (`reserve`, `release` and `ask_operator` for those three operations). Shown in the message and kept as `.tool`. |
| `message` | `str` | The host's error text, kept as `.message`. |
| `kind` | `str`, default `"tool"` | Why it failed: `tool` (the tool itself returned an error, or its reply was larger than the host's reply limit), `tier` (the tool is above the run's tier cap, or is a procedure tool, or unknown), `budget` (the call budget is used up), `lock` (an instrument is held by another run, or a reserve or release was refused), `policy` (an operator prompt was refused). Kept as `.kind`. The set is the host's (`run.rs`), not checked here. |

The exception's `str` is `"<tool>: <message>"`. It has no return value and cannot itself fail. `failure_report` copies `tool` and `kind` into the run's failure report. Not tied to any thread or lock.

See also: `proc_py_call`, `proc_py_failure_report`, `proc_py_retry` (retries this exception by default), `proc_py_series` (counts it as a failed point).

### `ProcedureFailure.__init__` [@ANCHOR: proc_py_failure_init]

Builds the exception `bench.expect` raises when a check is false. It exists so a failed check reports what the procedure knew at the time, not only a message: the context dict is carried into the failure report.

| Argument | Type | Meaning |
|---|---|---|
| `message` | `str` | The text of the failed check; becomes the exception message. |
| `context` | `dict` or `None` (default) | Values to show in the report, kept as `.context`; `None` becomes an empty dict. Borrowed, not copied. It must be JSON-encodable after `jsonable`'s conversions or the report cannot be sent (see the notes at `proc_py_failure_report`). |

Raised by `bench.expect` and by `bench.series` (when more points failed than allowed). Not an error code of the host: the run ends `failed` with `kind` `exception`. Not tied to any thread or lock.

See also: `proc_py_expect`, `proc_py_series`, `proc_py_failure_report`. The classes `BenchCancelled` (derives from `BaseException` so a procedure's `except Exception` does not swallow a cancel) and `PolicyError` (a `RuntimeError` raised by the audit hook) have no anchor; they are described at `proc_py_check_cancel` and `proc_py_make_hook`.

## The pipe to the host

### `Wire.__init__` [@ANCHOR: proc_py_wire_init]

Creates the object that owns the process's side of the pipe: one per process, made by `main` and stored in `WIRES`. It holds the write end, the read end, a lock that keeps lines from interleaving, the table of requests waiting for a reply and the cancel flag.

| Argument | Type | Meaning |
|---|---|---|
| `out_fd` | `int` | A file descriptor to write protocol lines to: in `main`, a duplicate of the original stdout, so the procedure's own prints (which go through `Output`) cannot corrupt the protocol. Owned by the process; never closed. |
| `in_file` | text file | Where replies and cancel requests are read: `main` passes file descriptor 0 opened with `closefd=False`. |

Fields: `lock` (a `threading.Lock` held while the id counter, the pending table or a write is touched), `pending` (request id to `{"event", "reply"}`), `next_id` (last id given; starts at 0 so the first request is 1), `cancel` (a `threading.Event`, set when the host asks the run to stop), `cancel_reason` (text from the host, or `None`) and `closed` (set to false and not read anywhere in this file; unknown whether the host side or a test uses it). No I/O happens in the constructor.

See also: `proc_py_wire`, `proc_py_wire_send`, `proc_py_wire_request`, `proc_py_wire_reader`. Tested in `src/procedure/tests.rs`.

### `Wire.send` [@ANCHOR: proc_py_wire_send]

Writes one protocol message as one line of JSON. Everything the procedure reports (emit, log, step, checkpoint, output, trace, the final report) and every request goes through here.

| Argument | Type | Meaning |
|---|---|---|
| `message` | JSON-compatible `dict` | The message, with an `op` key. Encoded with `jsonable` for unknown objects. Borrowed. |

No return value. `ValueError` when the message holds NaN or infinity (`allow_nan=False`, so the host never receives a line that is not strict JSON) and `TypeError` for a dict key that is not a string, number or bool; the error is raised in the caller, so a procedure that emits `float("nan")` gets the exception in its own code. When the pipe cannot be written (`OSError`, normally because the host is gone) the whole process ends at once with `os._exit(4)`; no cleanup handler runs, because there is nobody to receive anything. Writes are repeated until the whole line is out (a pipe may take a partial write). Takes `self.lock` for the full write, so lines from several threads of the procedure never interleave. It can block while the host is slow to read the pipe. Safe to call from any thread, but not retry-safe in the sense that it has no sequence number: a message sent is sent.

See also: `proc_py_wire_request`, `proc_py_output_write`, `proc_py_jsonable`.

### `Wire.request` [@ANCHOR: proc_py_wire_request]

Sends a message that expects an answer and waits for the matching reply. Used by `bench.call`, `reserve`, `release` and `ask_operator`; it is how a procedure blocks on the host.

| Argument | Type | Meaning |
|---|---|---|
| `message` | `dict` | The request without an `id`; `request` sets `message["id"]` to the next integer (the caller's dict is modified). |

Returns the reply dict (`{"op":"reply","id":..,"ok":bool,...}`) exactly as the host sent it; it does not look at `ok`, the callers do. Waits on a per-request `threading.Event` in steps of 0.2 s so a cancel is noticed within that time. If the cancel flag is set and the run is not yet in cleanup (`STATE["cleanup"]` false) the wait ends and `BenchCancelled` is raised (with the host's reason, or "cancelled"). During cleanup a cancel does not interrupt a request, so cleanup handlers can still call tools; but then a reply that never comes blocks the process until the host's grace-time kill. The pending slot is removed in every case. Thread safe: several threads of a procedure may each wait on their own request; the host answers `call` requests from separate threads, so replies can arrive in any order. The wait has no time limit of its own: the limits are the host's (wall-clock budget, operator timeout, grace time).

See also: `proc_py_wire_reader` (delivers the reply), `proc_py_wire_send`, `proc_py_call`, `proc_py_check_cancel`.

### `Wire.reader` [@ANCHOR: proc_py_wire_reader]

The thread that reads the host's messages: `main` starts it as a daemon thread before running any procedure code. It is the only reader of stdin.

No arguments, no return value. For every input line it parses JSON (a line that is not JSON is ignored); `{"op":"reply"}` is stored in the pending slot with the matching `id` and wakes the waiting `request` (a reply with an unknown `id`, for instance one for a request already given up on, is dropped); `{"op":"cancel","reason":..}` sets `cancel_reason` (default "cancelled") and then the `cancel` event, which `check_cancel`, `Wire.request`, `bench_sleep` and the others watch; any other `op` is ignored. When stdin reaches end of file, meaning the host closed the pipe or died, the thread ends the process with `os._exit(3)` without running cleanup. The `cancel_reason` and the event are not set under a lock in this branch, which is fine for a plain string written before the event. It runs for the life of the process.

See also: `proc_py_wire_request`, `proc_py_check_cancel`, `proc_py_main`.

### `wire` [@ANCHOR: proc_py_wire]

Returns the process's one `Wire`: the first element of `WIRES`. Every function of the `bench` API reaches the pipe this way instead of holding it, so the module's functions can be defined before `main` has opened the pipe.

No arguments. Returns the `Wire` object; raises `IndexError` if called before `main` appended it (only possible when the module is imported without running `main`). No I/O, no lock. The host's `--check` mode never creates a wire and never calls it.

See also: `proc_py_wire_init`, `proc_py_main`.

### `check_cancel` [@ANCHOR: proc_py_check_cancel]

Raises `BenchCancelled` once the host has asked the run to stop. It is the poll the API functions make before doing anything, so a cancelled run stops at its next `bench` call instead of continuing.

No arguments, no return value. Raises `BenchCancelled(reason)` (the host's reason, for example `cancel`, `timeout` or `operator_timeout`, else "cancelled") when the cancel event is set and `STATE["cleanup"]` is false. During cleanup it never raises, so cleanup handlers run to the end. `BenchCancelled` derives from `BaseException`, not `Exception`, on purpose: a procedure that writes `except Exception` does not swallow the stop; only a bare `except:` or `except BaseException` can, and `finally` blocks and registered cleanup handlers still run. After a cancel the host allows a grace time and then kills the process group, so a procedure stuck in a long CPU loop that never calls `bench` is killed, not cancelled politely. No lock, no I/O.

Not every function polls: `bench_log`, `bench_cleanup`, `bench_tools`, `bench_deadline`, `bench_retry` (directly), `bench_expect`, `bench_stats`, `bench_result` and `bench_release` do not call it.

See also: `proc_py_wire_reader`, `proc_py_sleep`, `proc_py_run_cleanups`.

## The `bench` API a procedure calls

### `bench_call` [@ANCHOR: proc_py_call]

`bench.call(tool, **arguments)`: calls any bench tool the run may use, through the host's normal dispatch path, and returns its result. This is the one way a procedure reaches an instrument (the audit hook tells it so).

| Argument | Type | Meaning |
|---|---|---|
| `tool` | `str` | A tool name as listed by `bench.tools()`, for example the names of `bench_list` style tools. Procedure tools (`bench_proc_*`) are not callable from inside a procedure. |
| `**arguments` | JSON values | The tool's own arguments, sent as the `args` object. Borrowed, encoded with `jsonable`. |

Returns the `value` of the host's reply, which is the tool's result as JSON data (dict, list, number, string or `None`). Raises `BenchCancelled` if a cancel is pending (checked first, and while waiting); `BenchError(tool, error, kind)` when the host answers `ok: false`: `tier` for a tool above the run's cap or not allowed, `budget` when the run's call budget (`max_calls`, default 1000, plus a small allowance for cleanup calls) is used up, `lock` when the instrument is held by another run, `tool` for the tool's own error or a reply that is too large. Blocks the calling thread until the host replies; the host runs each call on its own thread. Side effects are whatever the tool does: for a tool that can key a transmitter the host applies exactly what it applies to any caller (tier, instrument lock, band check, transmit gate); the runtime adds nothing and skips nothing. For a tool that changes an instrument the host also records the undo action, so the safe-state routine can run it when the run ends other than by success. Not idempotent unless the tool is; wrap a read in `bench.retry` when repeating is safe.

Example: `rssi = bench.call("some_read_tool", instrument="rx1")["dbm"]`.

See also: `proc_py_wire_request`, `proc_py_retry`, `proc_py_bench_error_init`, host side `proc_run_exec_call`.

### `bench_emit` [@ANCHOR: proc_py_emit]

`bench.emit(key, value)`: stores a progress value or partial result under a key. The host keeps it in the run's `emitted.json`, so it survives a kill or a crash and can be read while the run is going.

| Argument | Type | Meaning |
|---|---|---|
| `key` | any, converted with `str` | The name. The host keeps only the first 100 characters. |
| `value` | JSON value | The value; a later emit of the same key replaces it. |

No return value; it does not wait for the host. Raises `BenchCancelled` if a cancel is pending and `ValueError` for NaN or infinity in the value. The host silently drops a value whose JSON is over 65 536 bytes or a new key past 500 distinct keys, and writes a `log` event saying so; the procedure is not told. Safe to call often, but each call is a file write on the host.

See also: `proc_py_series` (emits `<name>.progress`), `proc_py_checkpoint`, host side `proc_run_handle`.

### `bench_log` [@ANCHOR: proc_py_log]

`bench.log(message)`: appends one line to the run's event list (the ordered list of events the host keeps for the run and the agent reads by cursor). Meant for notes about what the procedure is doing.

| Argument | Type | Meaning |
|---|---|---|
| `message` | any, converted with `str` | The text of the line. |

No return value, no reply. It does not check for cancel, so it works in cleanup handlers and after a cancel. The host's event list is capped, and an overflow drops events (the count is shown by the host, not here). `bench.retry`, `bench.series` and `run_cleanups` use it for their own notes. Does not block beyond the pipe write.

See also: `proc_py_emit`, `proc_py_step`, `proc_py_output_write` (printed text goes to a different stream).

### `bench_step` [@ANCHOR: proc_py_step]

`bench.step(name)`: names the step the procedure is in. The host shows it in the run's status and adds a `step` event, so someone watching sees "calibrating" rather than only "running".

| Argument | Type | Meaning |
|---|---|---|
| `name` | any, converted with `str` | The step's name; any length (the host stores it as sent). |

No return value. Raises `BenchCancelled` if a cancel is pending. No reply is awaited. Each call makes the host write the run state to disk, so do not call it in a tight loop.

See also: `proc_py_log`, `proc_py_emit`.

### `bench_sleep` [@ANCHOR: proc_py_sleep]

`bench.sleep(seconds)`: waits for a time, but ends at once with `BenchCancelled` if the host asks the run to stop. Use it instead of `time.sleep`, which would not notice a cancel.

| Argument | Type | Meaning |
|---|---|---|
| `seconds` | number, converted with `float` | The time in seconds, finite and zero or more. |

No return value. Raises `ValueError("sleep needs a finite, non-negative time")` for NaN, infinity or a negative number, and `BenchCancelled` when a cancel is pending before or arrives during the wait (the wait is `Event.wait(seconds)` on the cancel event, so a cancel ends it immediately). In a cleanup handler (`STATE["cleanup"]` true) it is a plain `time.sleep`, uninterruptible, so a cleanup can wait for hardware to settle. The sleep counts against the wall-clock budget, unlike `ask_operator`. Blocks the calling thread only.

See also: `proc_py_check_cancel`, `proc_py_retry`, `proc_py_run_cleanups`. Tested in `src/procedure/tests.rs`.

### `bench_checkpoint` [@ANCHOR: proc_py_checkpoint]

`bench.checkpoint(name, data)`: saves a JSON document under a name in the run's `checkpoints` directory, so a long run keeps its intermediate data even when it ends badly; a later run or the agent can read the file.

| Argument | Type | Meaning |
|---|---|---|
| `name` | any, converted with `str` | The file name. The host keeps only ASCII letters, digits, `_` and `-`, at most 60 characters, and writes `<name>.json`; a name with none of those is dropped. |
| `data` | JSON value | The document. The host drops it when its JSON is over 262 144 bytes. |

No return value; no reply. Raises `BenchCancelled` if a cancel is pending and `ValueError` for NaN or infinity. A dropped checkpoint is only noted as a `log` event; the procedure is not told. A second call with the same name replaces the file (the host writes it atomically).

See also: `proc_py_emit`, host side `proc_run_handle`.

### `bench_cleanup` [@ANCHOR: proc_py_cleanup]

`bench.cleanup(function)`: registers a function that must run when the procedure ends however it ends: return, exception, cancel or time-out. The place to switch a source off or restore a setting. It can be used as a decorator because it returns its argument.

| Argument | Type | Meaning |
|---|---|---|
| `function` | callable taking no arguments | Retained until the run ends; called once. |

Returns `function`. Raises `TypeError("bench.cleanup needs a function")` for a non-callable. Handlers run newest first (a stack), from `run_cleanups`, after the procedure has ended and the trace has been flushed; a handler that raises is reported in `cleanup_problems` and the others still run. During cleanup `check_cancel` does not raise, `bench.sleep` really sleeps and tool calls are still allowed (the host gives a small extra call allowance). A cleanup does not replace the host's own safe-state routine, which also undoes recorded instrument changes after a run that did not succeed. A handler registered while cleanups are running (from another handler) is run too, because the list is drained until empty.

See also: `proc_py_run_cleanups`, `proc_py_execute`.

### `bench_tools` [@ANCHOR: proc_py_tools]

`bench.tools()`: the tools this run may call, as the host listed them when the run started: name to tier, description and argument schema. It lets a procedure check that a tool exists before using it.

No arguments. Returns the decoded content of the run directory's `tools.json` (a dict of tool name to `{"tier", "description", "schema"}`, written by the host's `proc_tool_index_json`), read on first use and kept in `STATE["tools"]`; the same object is returned every time, so do not modify it. Raises `OSError` or `ValueError` if the file cannot be read or decoded (not expected: the host writes it before starting). Does not poll for cancel. The listing is the one clients see, minus the procedure tools, and a tool above the run's tier cap is listed but refused at call time.

See also: `proc_py_call`, `proc_py_check_source`.

### `bench_deadline` [@ANCHOR: proc_py_deadline]

`bench.deadline()`: how many seconds of the run's wall-clock budget are left, so a long procedure can decide to stop its sweep early instead of being cancelled.

No arguments. Returns a float: the budget `wall_s` from the run's `config.json`, minus the time since the program became ready, minus the time spent waiting for the operator (`STATE["paused"]`). It can be negative when the budget is already over (the host then asks the run to stop with reason `timeout`). Monotonic clock, no I/O, no lock, no cancel poll. Note it reflects this process's view; the host measures the same budget on its own clock, so the two can differ by a fraction of a second.

See also: `proc_py_ask_operator`, `proc_py_main` (sets `started` and `wall_s`).

### `bench_retry` [@ANCHOR: proc_py_retry]

`bench.retry(function, tries=3, delay=0.5, backoff=2.0, exceptions=(BenchError,))`: calls a function up to a number of times, waiting longer after each failure; for an instrument read that sometimes fails once.

| Argument | Type | Meaning |
|---|---|---|
| `function` | callable, no arguments | Called each attempt; its return value is returned. Use `lambda` to bind arguments. |
| `tries` | `int`, at least 1, default 3 | Total attempts, including the first. |
| `delay` | number, default 0.5 | Seconds to wait after the first failure. Converted with `float`. |
| `backoff` | number, default 2.0 | Multiplier applied to the wait after each failure. |
| `exceptions` | exception class or tuple, default `(BenchError,)` | Only these are retried; anything else, and `BenchCancelled`, propagates at once. |

Returns what `function()` returns. Raises `ValueError("retry needs at least one try")` for `tries < 1`; the last exception when every attempt failed. Between attempts it writes a `retry n/tries after: <error>` log line and waits with `bench.sleep`, so a cancel ends the wait. Not safe for functions with side effects that must not repeat (for instance a call that changes an instrument), unless repeating is harmless.

See also: `proc_py_sleep`, `proc_py_log`, `proc_py_series`.

### `bench_expect` [@ANCHOR: proc_py_expect]

`bench.expect(condition, message, **context)`: a check that stops the run with a structured failure carrying the values that explain it. Use it in place of `assert`, whose message has no context.

| Argument | Type | Meaning |
|---|---|---|
| `condition` | any | Truth-tested. |
| `message` | `str` | The failure text. |
| `**context` | JSON-able values | Named values shown in the report's `context`. |

Returns `condition` when it is true, so `x = bench.expect(value > 0, "...")` works. When it is false raises `ProcedureFailure(message, context)`, which, uncaught, ends the run as `failed` with `kind` `exception` and the context in the report. No I/O, no cancel poll.

See also: `proc_py_failure_init`, `proc_py_failure_report`.

### `bench_stats` [@ANCHOR: proc_py_stats]

`bench.stats(values)`: the usual summary numbers of a list of measurements, used by `bench.series` and by procedures that report a result.

| Argument | Type | Meaning |
|---|---|---|
| `values` | iterable of numbers | Each converted with `float`. Not modified. |

Returns a dict: `n` (count), `mean`, `std` (sample standard deviation, divisor n minus 1; 0.0 for a single value), `min`, `max` and `spread` (max minus min, the peak-to-peak value). Sums use `math.fsum`. Raises `ValueError("stats needs at least one value")` for an empty input and whatever `float()` raises for a non-number. NaN inputs give NaN results, which then cannot be sent (`ValueError` in `Wire.send`). Pure computation: no I/O, no cancel poll.

See also: `proc_py_series`.

### `bench_series` [@ANCHOR: proc_py_series]

`bench.series(name, count, measure, retries=2, cycle=None, cycle_after=2, max_failed=None)`: takes a fixed number of measurements, one per index, riding out failures that come from a flaky instrument, with an optional power-cycle step; this is what a sweep procedure is built on.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | Label for log lines and for the progress key `<name>.progress`. |
| `count` | `int` | Number of points; `measure` is called with `0 .. count-1`. |
| `measure` | callable `measure(i)` returning a number | One measurement; its result is converted with `float`. |
| `retries` | `int`, default 2 | Extra tries per point after a `BenchError`. |
| `cycle` | callable or `None` | Called to recover (for instance a power cycle) after `cycle_after` failures in a row. |
| `cycle_after` | `int`, default 2 | Consecutive failures that trigger `cycle`; the streak counts across points and resets after a success or a cycle. |
| `max_failed` | `int` or `None` | How many points may fail for good; `None` means `count`, so the series never stops early. |

Only `BenchError` is retried; any other exception, `BenchCancelled` included, propagates. `cycle` runs at most once per point and earns one more try. After each point `bench.emit("<name>.progress", {"done", "of", "failed", "cycles"})` is sent. When more than `max_failed` points have failed it raises `ProcedureFailure("<name>: more than N points failed", {"failed": [...], "values": [...]})`. Returns `{"name", "values" (the numbers, in order, failed points left out), "failed" ([{"index", "error"}]), "cycles", "stats" (from `bench.stats`, or `None` when nothing succeeded)}`. Note that `values` are not aligned to indexes once a point has failed. Blocks for as long as the measurements take; the calls inside `measure` count against the call budget and the wall-clock budget.

See also: `proc_py_stats`, `proc_py_emit`, `proc_py_retry`, `proc_py_failure_init`. Tested in `src/procedure/tests.rs` (cited twice there).

### `bench_reserve` [@ANCHOR: proc_py_reserve]

`bench.reserve(*names, timeout=30)`: takes the reservation of instruments for this run before using them, so another run cannot change them meanwhile. Useful when a procedure does several steps on a generator and a scope that must not be interleaved with another run.

| Argument | Type | Meaning |
|---|---|---|
| `*names` | `str` | Names of configured instruments (as `bench_list` shows). At least one. |
| `timeout` | number, seconds, default 30 | How long to wait for another run that holds them. The host clamps it to 0 .. 3600. |

No return value. All or none: the host takes several instruments in a sorted order so two runs asking for overlapping sets cannot deadlock. Raises `BenchCancelled` if a cancel is pending; `BenchError("reserve", message, "lock")` when the list is empty, a name is not a configured instrument, or an instrument is still held when the time is up (the message names the run that holds it). It blocks for up to `timeout`. A tool call that changes an instrument reserves it by itself for the call's duration; this function keeps it for longer. The host releases anything a run still holds when the run ends.

See also: `proc_py_release`, `proc_py_reserved`, host side `proc_run_reserve_op`.

### `bench_release` [@ANCHOR: proc_py_release]

`bench.release(*names)`: gives back instruments taken with `bench.reserve`, so another run can use them.

| Argument | Type | Meaning |
|---|---|---|
| `*names` | `str` | Instrument names. |

No return value. Raises `BenchError("release", message, "lock")` for an empty list or an unknown instrument. It does not poll for cancel, so it can run from a cleanup handler or the `finally` of `bench.reserved` after a cancel. Releasing an instrument the run does not hold is not an error: the host's `locks::release` does nothing for a name that is not held.

See also: `proc_py_reserve`, `proc_py_reserved`.

### `bench_reserved` [@ANCHOR: proc_py_reserved]

`with bench.reserved("gen", "scope"):` reserves for the block and always releases at its end. A context manager (built with `contextlib.contextmanager`), the preferred way to use reservations because the release cannot be forgotten.

| Argument | Type | Meaning |
|---|---|---|
| `*names` | `str` | Instrument names. |
| `timeout` | number, seconds, default 30 | Passed to `bench.reserve`. |

Yields nothing. Raises what `bench.reserve` raises (`BenchError` kind `lock`, `BenchCancelled`) on entry, in which case nothing is held and the block does not run. The `finally` calls `bench.release`, even when the block raised, including `BenchCancelled`; if the release itself raises, that exception replaces the block's. Nesting a second block on the same names is not advised: whether the inner exit releases the outer's hold is decided by the host's lock model (one hold per instrument per run, as `locks::release` removes by name), so keep one block per instrument.

See also: `proc_py_reserve`, `proc_py_release`.

### `bench_ask_operator` [@ANCHOR: proc_py_ask_operator]

`bench.ask_operator(message, timeout=None)`: pauses the run until the person at the bench has done something (plug in a cable, read a front panel) and answered. The agent relays the answer with the `bench_proc_input` tool; the run shows state `waiting_operator`.

| Argument | Type | Meaning |
|---|---|---|
| `message` | any, converted with `str` | What the person is asked to do. The host keeps the first 1000 characters. |
| `timeout` | number or `None` (default) | Seconds to wait, converted with `float`; the host clamps it to 1 .. the runner's `operator_timeout_s` (default 3600). `None` means the runner's limit. |

Returns the answer text (the reply's `value`). Raises `BenchCancelled` if a cancel is pending or arrives while waiting; `BenchError("ask_operator", message, "policy")` when another prompt is pending or the run is being stopped. When the answer takes longer than the limit the host cancels the whole run with reason `operator_timeout`, so the call ends with `BenchCancelled`, not an error value. The time spent waiting is added to `STATE["paused"]`, even when the wait ends in an exception, and so is not counted against the wall-clock budget or by `bench.deadline`. Blocks the calling thread. The answer is whatever the agent supplied: treat it as data, not as proof the person did the step.

See also: `proc_py_deadline`, `proc_py_wire_request`, host side `proc_run_poll_ask`, `proc_run_answer`.

### `bench_result` [@ANCHOR: proc_py_result]

`bench.result(value)`: sets the procedure's final value, the thing returned to whoever asks for the run's result.

| Argument | Type | Meaning |
|---|---|---|
| `value` | JSON-able | The result; a later call replaces an earlier one. |

Returns `value`. It does not end the procedure and does not poll for cancel. The value is encoded when the procedure finishes (`execute`), not here, so a value that cannot be encoded (NaN, infinity) ends the run as `failed` then, not at this call. A procedure may instead leave a global variable named `result`; `bench.result` takes precedence, and when its value is `None` the global `result` is used.

See also: `proc_py_execute`.

## Reporting a failure

### `short` [@ANCHOR: proc_py_short]

A repr of any value, bounded in length and guaranteed not to raise. It is used for the variables in a failure report, where a hostile or broken `__repr__` must not hide the real error.

| Argument | Type | Meaning |
|---|---|---|
| `value` | any | Borrowed. |

Returns `repr(value)`; when that raises, `<repr failed: ErrorClassName>`; when longer than `REPR_LIMIT` (200) the first 200 characters followed by `... (N characters)` with the full length. Catches `Exception` only, so `BaseException` from a `__repr__` still propagates. No I/O.

See also: `proc_py_frame_report`, `proc_py_run_cleanups` (names a failed handler with it).

### `frame_report` [@ANCHOR: proc_py_frame_report]

Describes one stack frame of the procedure's own code with its local variables, so a failure report shows what the program knew at each level.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | frame object | A traceback frame; borrowed. |
| `line` | `int` | The line number reached in it (from the traceback entry). |

Returns `{"function": <code name>, "line": line, "locals": {name: short repr}}`. Variables whose names start with `__`, the name `bench` and modules are left out. At most 30 variables and 8192 characters of names plus reprs are kept; when either limit is hit the last entry is `"...": "more variables left out"`. Takes a snapshot with `list(frame.f_locals.items())`, so it does not fail if the frame changes. Calls `repr` of each value through `short`. No I/O.

See also: `proc_py_failure_report`, `proc_py_short`.

### `failure_report` [@ANCHOR: proc_py_failure_report]

Builds the structured description of how a procedure ended badly: it becomes the `report` of the final `failed` or `cancelled` message and then the run's failure record, which the agent reads to fix the procedure.

| Argument | Type | Meaning |
|---|---|---|
| `error` | exception instance | The exception that ended the run (also a `SyntaxError` found when compiling, or `SystemExit`). Borrowed. |

Returns a dict: `type` (class name), `message` (`str(error)`), `traceback` (formatted text, only the last 8000 characters), `frames` (a `frame_report` for each traceback entry whose file is `procedure.py`, outermost first; runtime and library frames are not listed), plus `context` for a `ProcedureFailure` (the context after a JSON round trip through `jsonable`), `tool` and `kind` for a `BenchError`, and `line` for a `SyntaxError`. The host adds its own `kind` (`exception`, `cancelled`, `timed_out`) afterwards. No I/O. Cannot fail on odd variables (uses `short`). Two oddities, reported rather than fixed: the context is encoded without `allow_nan=False` and then the text "NaN" is replaced by "null" everywhere, including inside strings, so a context string containing NaN is altered; and infinity in a context survives that round trip as a float and then makes the later `Wire.send` of the report raise `ValueError`, which ends the process with a crash instead of a report.

See also: `proc_py_execute`, `proc_py_main`, `proc_py_failure_init`, `proc_py_frame_report`.

## The audit hook (not a sandbox)

### `path_inside` [@ANCHOR: proc_py_path_inside]

Decides whether a path lies in the scratch directory, for the audit hook's rule that a procedure may change files only there.

| Argument | Type | Meaning |
|---|---|---|
| `path` | str, bytes, path-like | The path an audit event names. |
| `scratch` | `str` | The scratch directory after `realpath`, as `main` computes it. |

Returns true when `os.path.realpath(path)` is the scratch directory or below it (so `..` and symbolic links out of it do not count as inside). Returns false when the path cannot be decoded (`TypeError` or `ValueError`), for instance an integer file descriptor, so an event with a descriptor where a path is expected is refused. It checks only; it touches no file other than resolving links. Not a check for existence.

See also: `proc_py_open_refusal`, `proc_py_make_hook`.

### `open_refusal` [@ANCHOR: proc_py_open_refusal]

The rule for the `open` audit event: returns the reason the open must be refused, or `None` to allow it. It is what stops a procedure from opening a serial port or writing outside its scratch directory by accident.

| Argument | Type | Meaning |
|---|---|---|
| `args` | tuple | The event's arguments `(path, mode, flags)`. |
| `scratch` | `str` | The real path of the scratch directory. |

Returns `None` when the target is not a path (an integer descriptor, for instance) or the open is allowed; otherwise `"open the device node <real path>"` or `"write <real path> (outside the scratch directory)"`. Rules: any character or block device is refused, for reading too, except `/dev/null`, `/dev/zero`, `/dev/urandom`, `/dev/random` and `/dev/full` (`DEVICE_ALLOWED`); an open for writing (flags with write, read-write, create or append, or a text mode containing `w`, `a`, `x` or `+`) is allowed only inside the scratch directory or onto `/dev/null`. Reading ordinary files anywhere (the system's files, the run directory) is allowed, which is one of the ways the hook is not a sandbox. Symbolic links are followed (`realpath`) before the test. Does one `stat` of the target.

See also: `proc_py_path_inside`, `proc_py_make_hook`.

### `make_hook` [@ANCHOR: proc_py_make_hook]

Builds the function that `sys.addaudithook` installs, which looks at every audit event of the process and raises `PolicyError` for the ones a procedure should not cause. It keeps the tier model honest: instruments are reached through `bench.call`, where tiers, locks and the transmit gate apply, and not by opening a serial port or starting a program from inside the procedure.

| Argument | Type | Meaning |
|---|---|---|
| `scratch` | `str` | The scratch directory's real path. |

Returns the hook function `hook(event, args)`. The events refused (`BLOCKED`): starting a process (`subprocess.Popen`, `os.system`, `os.exec`, `os.posix_spawn`, `os.spawn`, `os.fork`, `os.forkpty`, `pty.spawn`, `_posixsubprocess.fork_exec`), signalling (`os.kill`, `os.killpg`), network use (`socket.connect`, `bind`, `sendto`, `sendmsg`, `getaddrinfo`, `gethostbyname`, `gethostbyaddr`), any event starting `ctypes.`, an `open` per `open_refusal`, and the path events in `PATH_EVENTS` (remove, rename, mkdir, rmdir, chmod, chown, truncate, symlink, link, utime and the `shutil` copy, move and remove calls) whose every named path is not inside the scratch directory. A refusal raises `PolicyError("blocked: this procedure tried to <why> (<event>). Use bench.call(tool, ...) for instruments; the files you may write are in the scratch directory.")` in the offending code, counts it in `STATE["violations"]` and, for the first 50 only (`VIOLATION_LIMIT`), sends a `violation` message that the host stores as an event and in the run's violation list. A re-entrancy guard (a thread-local flag) lets events raised while the hook itself works pass. Installed by `main` after the source is compiled and `ctypes` is imported; cannot be removed. Limits: it sees only Python audit events, so a C extension, an already-open descriptor or a library that does its own system calls is not caught; it is an aid, not a security boundary, and the real limits are the host's resource limits, process group and the stripped environment.

See also: `proc_py_open_refusal`, `proc_py_path_inside`, `proc_py_main`.

## Captured output

### `Output.__init__` [@ANCHOR: proc_py_output_init]

Creates a file-like object that stands in for `sys.stdout` or `sys.stderr`, so a procedure's `print` goes to the host as protocol messages and cannot corrupt the pipe. `main` makes two: `Output("stdout")` and `Output("stderr")`.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | The `op` used for messages: `"stdout"` or `"stderr"`. The host appends the text to a file of that name, capped in size (a note records a cut). |

Stores `name`; no I/O.

See also: `proc_py_output_write`, `proc_py_main`.

### `Output.write` [@ANCHOR: proc_py_output_write]

Sends printed text to the host, which stores it with a size cap; read later with the run's output tool by offset.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | The text. Empty text sends nothing. |

Returns `len(text)`, as a file's `write` should. Sends `{"op": name, "text": str(text)}` with `Wire.send` (so it ends the process with exit code 4 if the host is gone, and can block on a full pipe). Does not poll for cancel. Called from any thread; the wire's lock keeps messages whole. Output after the host's size limit is dropped there and the status notes it.

See also: `proc_py_output_init`, `proc_py_wire_send`.

### `Output.flush` [@ANCHOR: proc_py_output_flush]

Does nothing and returns `None`: `write` sends at once, so there is nothing buffered to push out. It exists because `print(..., flush=True)`, the `logging` module and many libraries call `flush` on `sys.stdout` and `sys.stderr`, and a file-like object without it would raise `AttributeError` in the procedure. No arguments, no I/O, no lock; safe from any thread. Data already written is as delivered as it will get: the host stores it as it arrives, and a kill of the process loses nothing that `write` had returned from, except what was still in the OS pipe.

See also: `proc_py_output_write`.

### `Output.isatty` [@ANCHOR: proc_py_output_isatty]

Returns `False`, always, so libraries that choose colours, line editing or progress bars by `isatty` choose plain text output, which is what the host's stored output file needs. No arguments, no I/O, no lock; the answer never changes, because output is never a terminal. Returned for both `Output("stdout")` and `Output("stderr")`.

See also: `proc_py_output_write`.

## Building the `bench` module

### `make_bench` [@ANCHOR: proc_py_make_bench]

Builds the module object that the procedure gets as `bench` (also registered as `sys.modules["bench"]`, so `import bench` works): a `types.ModuleType` named `bench` with the API of this file.

| Argument | Type | Meaning |
|---|---|---|
| `arguments` | `dict` | The JSON arguments the agent gave the run; becomes `bench.args`. Not copied. |

Returns the module with these attributes: `call`, `emit`, `log`, `step`, `sleep`, `checkpoint`, `cleanup`, `tools`, `deadline`, `retry`, `expect`, `stats`, `series`, `ask_operator`, `reserve`, `release`, `reserved`, `result` (the functions documented above, without their `bench_` prefix), `args`, and the exception classes `BenchError`, `BenchCancelled`, `ProcedureFailure` and `PolicyError`. A name not in this list does not exist on `bench`; a procedure that mistypes one gets `AttributeError`. No I/O, cannot fail. The procedure's global namespace also gets `bench` directly and `__name__ == "__main__"`.

See also: `proc_py_main`, and the manual pages of the procedure tools for the agent-facing description.

## Tracing

### `tracer` [@ANCHOR: proc_py_tracer]

The `sys.settrace` function that records which lines of the procedure's own code run, when the run was started with `trace: true` (slower). It lets the status show the line a stuck or failed run reached.

| Argument | Type | Meaning |
|---|---|---|
| `frame`, `event`, `argument` | as `sys.settrace` | The interpreter's trace arguments. |

Returns `None` for any frame not in `procedure.py` (so runtime and library code is not traced), otherwise itself. On a `line` event it appends the line number to `STATE["trace_lines"]` and sends the batch with `flush_trace` when 50 lines have collected or more than 0.5 s passed since the last flush. Called only from the thread `sys.settrace` was set on (the main thread; other threads of a procedure are not traced). It slows the procedure by a large factor and adds one `trace` event per flush to the host's capped event list. A line reached while only the last flush is outstanding is sent at the end of the run.

See also: `proc_py_flush_trace`, `proc_py_main`.

### `flush_trace` [@ANCHOR: proc_py_flush_trace]

Sends the traced line numbers collected so far and empties the buffer. No arguments, no return value. Does nothing when the buffer is empty; otherwise sends `{"op":"trace","lines":[...]}` (the host keeps the last number as the run's current `line` and records an event). Called by `tracer` and once by `main` after the procedure ends and before cleanups run. Same pipe failure behaviour as `Wire.send`.

See also: `proc_py_tracer`.

## Ending a run

### `run_cleanups` [@ANCHOR: proc_py_run_cleanups]

Runs the handlers registered with `bench.cleanup`, newest first, after the procedure has ended; it is how a procedure puts instruments back whatever happened.

No arguments. Sets `STATE["cleanup"]` true (so cancels no longer interrupt `bench.sleep`, `check_cancel` or a request), then pops and calls each handler. A handler that raises anything, `BaseException` included, is not allowed to stop the others: the text `cleanup <function repr> failed: <error repr>` is logged and added to the returned list. Returns that list of problem strings (empty when all handlers succeeded); `main` stores it as `cleanup_problems` in the final report. It runs on the main thread, with no time limit of its own: the host's grace time after a cancel, then a kill, applies.

See also: `proc_py_cleanup`, `proc_py_main`, `proc_py_sleep`.

### `execute` [@ANCHOR: proc_py_execute]

Runs the compiled procedure and decides how it ended, as the pair `(outcome, payload)` that `main` sends to the host.

| Argument | Type | Meaning |
|---|---|---|
| `code` | code object | The procedure compiled by `main` as `procedure.py`. |
| `namespace` | `dict` | Globals for the run: `bench`, `__name__`, `__builtins__`. A global named `result` is used as the result when `bench.result` was not called. |

Returns `("done", {"value": <result as plain JSON>})` when the code finishes, or on `SystemExit` with code `None` or 0 (in that case the value is `STATE["result"]` as is, not re-encoded or checked); `("cancelled", {"reason": text, ...failure_report})` when `BenchCancelled` ended it; `("failed", failure_report)` for any other exception, a non-zero `SystemExit` or any other `BaseException`. The value is encoded with `jsonable` and `allow_nan=False`, so a NaN in the result is a `ValueError` and the run ends `failed`. No exception leaves the function. Runs the procedure on the calling (main) thread; blocks until it ends. The host decides the final state name (`succeeded`, `failed`, `cancelled`, `timed_out`) from this outcome together with whether it had asked for a stop.

See also: `proc_py_failure_report`, `proc_py_result`, `proc_py_main`, `proc_py_run_cleanups`.

### `check_source` [@ANCHOR: proc_py_check_source]

`--check` mode: compiles a source and lists the tool names it passes as string literals to `.call(...)`, without running anything. The host uses it for a dry run and to compare a saved procedure's declared tier with the tools its source names.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | A file holding the Python source. Opened read-only, UTF-8. |

Returns `{"ok": True, "calls": [sorted names]}`, or `{"ok": False, "error": text, "line": number}` for a `SyntaxError`. The scan finds every call whose function is an attribute named `call` and whose first argument is a string constant: `bench.call("x")` is found, and so is `anything.call("x")`; `bench.call(name_variable)`, a name built at run time, an alias such as `c = bench.call; c("x")` and `bench_call` are not found, so the list is what the source names literally, not a guarantee of what it will call. Raises `OSError` if the file cannot be read. It does not run the procedure, the audit hook or any import beyond the compile. Run by the backend with the interpreter's `-I` flag, a stripped environment and a 20 s limit.

See also: `proc_py_main`, host side `proc_cpython_check`. Tested in `src/procedure/tests.rs`.

### `main` [@ANCHOR: proc_py_main]

The entry point of the runtime process: either `bench_runtime.py --check <source>` (prints the `check_source` result as one JSON line on stdout and returns 0) or `bench_runtime.py <run_dir>` for a run.

| Argument | Type | Meaning |
|---|---|---|
| `argv` | `list[str]` | `sys.argv`: `[program, "--check", source]` or `[program, run_dir]`. Anything else with fewer than two items raises `IndexError`. |

Returns 0 in every normal case, including a procedure that failed; the verdict is on the pipe. For a run it: reads `<run_dir>/config.json` (`args`, `wall_s` seconds, `trace` flag, `scratch` directory); duplicates stdout as the protocol pipe and points file descriptor 1 at `/dev/null`; makes the `Wire`; replaces `sys.stdin` with `/dev/null` and `sys.stdout` and `sys.stderr` with `Output` objects; reads `tools.json` lazily and `source.py` now; starts the reader thread; builds `bench` and puts it in `sys.modules`; starts the wall-clock timer and sends `ready` with the Python version; compiles the source as `procedure.py` (a `SyntaxError` sends `failed` with its report and returns 0 without cleanup); imports `ctypes` (before the hook, because numpy imports it) and `linecache`, fills the linecache so tracebacks show the procedure's lines, installs the audit hook, enables the tracer if asked, runs `execute`, stops tracing, flushes the trace, runs the cleanups and sends the final message (`done`, `failed` or `cancelled`) with `cleanup_problems` added to its report. The syntax-error report has no `cleanup_problems` key. Errors reading the run directory raise and end the process with a traceback, which the host reports as a crash with the tail of stderr. The process is single-purpose and short-lived: one run per process. Time out and cancel are the host's job; this function only reacts.

See also: `proc_py_wire_init`, `proc_py_make_hook`, `proc_py_make_bench`, `proc_py_execute`, `proc_py_run_cleanups`, host side `proc_cpython_start`, `proc_run_supervise`.

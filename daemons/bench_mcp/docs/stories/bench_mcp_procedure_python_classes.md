# Procedure runtime classes (Python)

The classes of `src/procedure/bench_runtime.py`, the program that runs inside the procedure sandbox next to the server (the Rust side is `src/procedure/cpython.rs`). The processes talk over a newline-delimited JSON pipe.

### `BenchError` [@ANCHOR: bench_mcp:proc_py_bench_error_class]

**Purpose and location.** The exception a procedure sees when a bench tool call fails. Defined in `daemons/bench_mcp/src/procedure/bench_runtime.py` as a subclass of `Exception`, so a procedure's `except Exception` catches it. Its `kind` says why the call failed: `tool` (the instrument or tool reported an error), `tier` (above the run's tier cap), `budget` (the call or wall-clock budget is used up), `lock` (another run holds the instrument) or `policy` (refused by policy).

| Attribute | Type | Meaning |
|---|---|---|
| `tool` | `str` | The name of the tool that was called. |
| `message` | `str` | The failure text from the host. |
| `kind` | `str` | `tool` (default), `tier`, `budget`, `lock` or `policy`. |

**Invariants, ownership and concurrency.** Built by the code that unpacks a host reply, raised in the procedure's thread, never shared; the string form is `"<tool>: <message>"`. It has no serialized form of its own; the failure report that the host writes carries these three fields as text.

**Functions that touch it.** `__init__` (`proc_py_bench_error_init`) and the tool-call wrapper that raises it.

**See also:** test `an_uncaught_error_gives_a_report_with_the_locals_of_the_procedures_own_frames_and_the_last_calls` (src/procedure/tests.rs).

### `BenchCancelled` [@ANCHOR: bench_mcp:proc_py_bench_cancelled_class]

**Purpose and location.** Raised in the procedure when the host asks the run to stop (a cancel or a time-out). Defined in `bench_runtime.py` as a subclass of `BaseException` on purpose: a procedure's `except Exception` does not swallow it, and the registered cleanup handlers still run before the process ends. It carries the reason text given by the host, or `cancelled` when none was given.

| Attribute | Type | Meaning |
|---|---|---|
| `args[0]` | `str` | The reason for the cancel (`Wire.cancel_reason`, or `cancelled`). |

**Invariants, ownership and concurrency.** Raised by `Wire.request` and `check_cancel` in the procedure's thread after the reader thread set the cancel flag. While cleanup handlers run (`STATE["cleanup"]` true) the flag no longer interrupts requests, so the cleanup's own tool calls (for example switching an output off) can finish.

**Functions that touch it.** `Wire.request` (`proc_py_wire_request`), `check_cancel` (`proc_py_check_cancel`).

**See also:** test `cancel_ends_a_run_after_its_cleanup_handlers_have_made_their_tool_calls` (src/procedure/tests.rs).

### `ProcedureFailure` [@ANCHOR: bench_mcp:proc_py_procedure_failure_class]

**Purpose and location.** Raised by `bench.expect` when a check in the procedure fails. Defined in `bench_runtime.py` as a subclass of `Exception`. Besides the message it carries a `context` dictionary that the host includes in the failure report, so a failed check says what was measured.

| Attribute | Type | Meaning |
|---|---|---|
| `context` | `dict` | Values recorded with the failure; empty dictionary when none were given. |

**Invariants, ownership and concurrency.** Created and raised in the procedure's thread. The context is converted to JSON by the report writer, which replaces non-finite numbers, so the report is valid JSON.

**Functions that touch it.** `__init__` (`proc_py_failure_init`) and the `expect` helper that raises it.

**See also:** test `an_uncaught_error_gives_a_report_with_the_locals_of_the_procedures_own_frames_and_the_last_calls` (src/procedure/tests.rs).

### `PolicyError` [@ANCHOR: bench_mcp:proc_py_policy_error_class]

**Purpose and location.** Raised by the audit hook when the procedure tries an operation the sandbox forbids: starting a process, opening a socket, opening a device or writing outside its scratch directory. Defined in `bench_runtime.py` as a subclass of `RuntimeError`. The text names the refused operation; the host counts each refusal as a violation in the run record.

| Attribute | Type | Meaning |
|---|---|---|
| `args[0]` | `str` | The text describing the refused operation. |

**Invariants, ownership and concurrency.** Raised inside the hook, in whichever thread attempted the operation; a re-entrancy flag (`busy`) keeps the hook from refusing its own work. Calling `bench` tools through the wire is allowed.

**Functions that touch it.** The audit hook built in `bench_runtime.py` (`make_hook`, which returns the `hook` closure).

**See also:** test `the_audit_hook_blocks_processes_sockets_devices_and_writes_outside_scratch_and_allows_bench_call` (src/procedure/tests.rs).

### `Wire` [@ANCHOR: bench_mcp:proc_py_wire_class]

**Purpose and location.** The newline-delimited JSON pipe from the procedure process to the host. Defined in `bench_runtime.py`; one instance per process, kept in `WIRES[0]` and read with `wire()`. The host writes replies and cancel requests to the process's standard input, the process writes requests and events to a file descriptor.

| Attribute | Type | Meaning |
|---|---|---|
| `out_fd` | `int` | File descriptor messages are written to. |
| `in_file` | text file | The pipe the host's messages arrive on. |
| `lock` | `threading.Lock` | Serializes writes and protects `pending` and `next_id`. |
| `pending` | `dict[int, dict]` | Requests waiting for a reply, by request id: each slot has an `event` and the `reply`. |
| `next_id` | `int` | The last request id used. |
| `cancel` | `threading.Event` | Set by the reader when the host asks the run to stop. |
| `cancel_reason` | `str or None` | The reason text sent with the cancel. |
| `closed` | `bool` | Initialised false; not changed in this module (unknown whether other code uses it). |

**Invariants, ownership and concurrency.** Shared between the procedure thread (`send`, `request`) and one reader thread (`reader`). `send` holds the lock for the whole write; a write error ends the process with exit code 4, and the end of the input ends it with exit code 3, so the program never outlives the host. `request` waits in 0.2 s steps and gives up with `BenchCancelled` when cancel is set, unless cleanup is running. Messages are JSON, one per line, with `allow_nan` off.

**Functions that touch it.** `wire` (`proc_py_wire`), `__init__` (`proc_py_wire_init`), `send` (`proc_py_wire_send`), `request` (`proc_py_wire_request`), `reader` (`proc_py_wire_reader`).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back` (src/procedure/tests.rs).

### `Output` [@ANCHOR: bench_mcp:proc_py_output_class]

**Purpose and location.** A file-like object that replaces the procedure's standard output and error: each write is sent to the host as an event, which stores the text with a size cap (the events file stops growing past `EVENTS_CAP` and counts what it dropped). Defined in `bench_runtime.py`.

| Attribute | Type | Meaning |
|---|---|---|
| `name` | `str` | The wire operation to send the text with (`stdout` or `stderr`). |

**Invariants, ownership and concurrency.** Writes go through `wire().send`, so they are serialized by the wire's lock; `write` returns the length of the text as a file object must, and an empty write sends nothing.

**Functions that touch it.** `__init__` (`proc_py_output_init`), `write` (`proc_py_output_write`).

**See also:** test `a_procedure_runs_to_the_end_and_everything_it_did_can_be_read_back` (src/procedure/tests.rs).

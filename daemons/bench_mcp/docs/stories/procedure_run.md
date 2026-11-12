# A Procedure Run Is a Python Program in a Process of Its Own, Supervised From the Server: This Is the Host Side, `run.rs` and `cpython.rs`

As the person (or the AI agent) who starts a bench procedure, I want the server to start my program in a separate, limited process, answer its tool calls under the run's tier cap and call budget, keep a record I can read while it runs and after it ends, wait for the operator when the program asks, stop it cleanly when I cancel or the time is up, and put the instruments it touched in a safe state when it does not succeed, even if the server itself died. This story documents the two Rust files that do that: `daemons/bench_mcp/src/procedure/cpython.rs`, the CPython backend (one `python3 -I` process per run, in a fresh scratch directory, with a stripped environment, a process group and `setrlimit` limits), and `daemons/bench_mcp/src/procedure/run.rs`, the supervisor (one thread per run, the run's record, the registry of live runs, the host half of the run protocol, the final state, shutdown and crash recovery). The Python side of the same pipe is `bench_runtime.py`, documented in `procedure_runtime.md`; the traits the backend implements (`Backend`, `Session`, `Limits`, `SpawnSpec`, `Incoming`) are in `backend.rs`; the run directory files are written by `store.rs`, the reservations by `locks.rs`, the safe-state rules by `safe.rs`, and the tool index by `tiers.rs`.

Nothing here keys a transmitter or touches an instrument itself. Every tool call a program makes comes in as a `call` message and is dispatched by `exec_call` through `crate::call`, the same dispatcher an agent's own tool call uses, after three host checks: the tool is not a procedure tool and is within the run's tier cap (`ToolIndex::allow`), the call budget is not used up, and, for a tool above `read_only`, the instrument is reserved for this run. A tool that can transmit therefore keeps its own tier, lock, band check and transmit gate inside the dispatcher; this file adds none of them and bypasses none. THE LIMITS AND THE PROCESS GROUP ARE NOT A SANDBOX: they keep accidents and runaways out and make a kill reach everything the program started; code that tries can get around them.

Things the entries below share. `RUNTIME` (`include_str!("bench_runtime.py")`) is the Python runtime written into each run directory. `TASK_MARGIN` (2048) is the number of threads and processes the program may add to those the user already has. `MAX_LINE` (16 MiB) is the longest protocol line read from the program. `CPython { python, wrapper }` is the backend: `python` is the interpreter path (`Config.python`, default `/usr/bin/python3`), `wrapper` is `none` or `systemd-run` (`Config.wrapper`). `PySession { child, stdin, rx }` is the live session: the child process, the write end (`None` once the session was killed) and the channel the reader threads feed. `Rec` is the persisted record of a run (`state.json`); `RunCtx` is the run's shared context (its directory, config, instruments, tool index, the `Rec` and event log behind mutexes, the emitted values, the last calls, the hook violations, the cancel reason, the touched instruments, the held reservations, the pending prompt and its answer). `Params` (anchor `proc_run_params`) and the tool (`proc_run_tool`) are documented where they are defined in `mod.rs`. Run states: `starting`, `running`, `waiting_operator`, `cancelling` (the four active states, see `proc_run_is_active`) and the final `succeeded`, `failed`, `cancelled`, `timed_out`, `lost`. Locking: a `RunCtx` field is locked for the shortest time possible; the code never holds two of them across a call that can block, except `held` while `locks::reserve` waits and `ask` while a prompt is read or cleared (and then `rec` inside it, always in that order). A poisoned mutex is recovered with `into_inner`, never a panic.

## The environment, the limits and the command line of a program

### `stripped_env` [@ANCHOR: proc_cpython_env]

Builds the whole environment of a program. The server's environment is never inherited (`env_clear` is called by the callers), so no key, token or path of the server reaches a procedure, and a program's numerical libraries do not start a thread per core. Private function of `check` and `start`.

| Argument | Type | Meaning |
|---|---|---|
| `scratch` | `&Path` | The run's scratch directory (`<run_dir>/scratch`); borrowed. It becomes `HOME` and `TMPDIR`. |

Returns a list of `(name, value)` pairs: `PATH=/usr/bin:/bin`, `LANG=C.UTF-8`, `OPENBLAS_NUM_THREADS=1`, `OMP_NUM_THREADS=1`, `MKL_NUM_THREADS=1`, `HOME=<scratch>`, `TMPDIR=<scratch>`. Nothing else is set. A scratch path that is not valid UTF-8 is converted lossily. Pure function: no I/O, no locks, no errors. For a path with no valid-UTF-8 form the variables name a different path than the real one (unknown whether that can happen: the run directory is made by `store::new_run_dir` from the configured root).

See also: `proc_cpython_start`, `proc_cpython_check`. Tested in `src/procedure/tests.rs`.

### `user_tasks` [@ANCHOR: proc_cpython_user_tasks]

Counts the threads and processes the current user has right now, so that the program's `RLIMIT_NPROC` can be set to that count plus `TASK_MARGIN`. `RLIMIT_NPROC` counts every task of the real user, other programs' included, so a fixed small limit would stop a program that starts a few threads (numpy) on a busy machine; a limit relative to the present count still stops a runaway fork loop at about `TASK_MARGIN` more tasks.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | Reads `/proc` and the real user id (`getuid`). |

Returns the sum of the `Threads:` line of `/proc/<pid>/status` over every numeric entry of `/proc` whose first `Uid:` field (the real uid) is this user's; a status file that cannot be read or has no `Threads:` line counts as skipped or 1. Special value: 4096 when `/proc` itself cannot be listed. No error is reported. The count is a snapshot: tasks started between the scan and the program's first thread are why the margin is large (found by repeating the tests under load). Cost: one directory listing and one small file read per process of the machine, on every run start; it blocks the calling thread (the supervisor's) for that time. No locks and no side effects. One `unsafe` call, `getuid`, which has no preconditions.

See also: `proc_cpython_start` (the only caller), `proc_cpython_limit`. Tested in `src/procedure/tests.rs`.

### `limit` [@ANCHOR: proc_cpython_limit]

Sets one resource limit with `setrlimit`. It exists so the five limits of a program are set by one small function inside the closure that runs in the child between `fork` and `exec`.

| Argument | Type | Meaning |
|---|---|---|
| `resource` | `libc::__rlimit_resource_t` | Which limit: `RLIMIT_AS`, `RLIMIT_CPU`, `RLIMIT_NOFILE`, `RLIMIT_FSIZE`, `RLIMIT_NPROC`, `RLIMIT_CORE`. |
| `soft` | `u64` | The soft limit (what is enforced); the units are the resource's own (bytes, seconds, files, processes). Cast to `rlim_t`. |
| `hard` | `u64` | The hard limit (the ceiling the program could raise its soft limit to; a program cannot raise the hard one). |

No return value. The result of `setrlimit` is ignored: a call that fails (for instance a hard limit already lower than the one asked for) leaves the old limit in place and nothing reports it. Only async-signal-safe work is done (building a struct and one system call), which is why it may run after `fork`. Affects only the process it runs in, the child. See the caution at `proc_cpython_start`.

See also: `proc_cpython_start`. Tested in `src/procedure/tests.rs` by programs that exceed a limit.

### `command_line` [@ANCHOR: proc_cpython_command_line]

Builds the argument vector of a run or of a source check. Public so tests can assert it.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&CPython` | The interpreter path and the wrapper (`none` or `systemd-run`); borrowed. |
| `runtime` | `&Path` | The `bench_runtime.py` written into the run directory. |
| `tail` | `&[&str]` | The arguments after the runtime: the run directory for a run, `--check <file>` for a check. |
| `limits` | `&Limits` | Only `memory_mb` is used, and only for the wrapper's `MemoryMax`. |

Returns the vector, first element the program to execute. With `wrapper == "systemd-run"` it starts with `systemd-run --user --scope --quiet --collect -p MemoryMax=<memory_mb>M -p TasksMax=64`; any other `wrapper` value (including `none`) adds nothing and is not checked here. Then `<python> -I -B -u <runtime> <tail...>`: `-I` isolated mode (no user site, no environment variables, no script directory on the path), `-B` no bytecode files, `-u` unbuffered. No I/O and no errors. Note that `TasksMax=64` is fixed while `TASK_MARGIN` allows 2048 more tasks: under the wrapper the stricter limit decides (see the report note).

See also: `proc_cpython_start`, `proc_cpython_check`, `proc_cpython_no_limits`. Tested in `src/procedure/tests.rs` (two tests).

### `no_limits` [@ANCHOR: proc_cpython_no_limits]

Builds the `Limits` value passed to `command_line` for a source check, which runs no procedure code. Private helper of `check`.

No arguments. Returns `Limits { memory_mb: 1024, cpu_s: 20, files: 64, file_mb: 1 }`. Of these only `memory_mb` has any effect, and only through the wrapper's `MemoryMax`; `check` always uses the `none` wrapper and sets no `setrlimit` limit, so the other three values are not applied to the check process (it is bounded by the 20 s wait in `check` instead). No I/O, no errors.

See also: `proc_cpython_check`, `proc_cpython_command_line`. Tested in `src/procedure/tests.rs`.

## Starting, talking to and ending a program

### `pump` [@ANCHOR: proc_cpython_pump]

Starts the two reader threads of a session and returns the channel they feed. The program's stdout carries protocol lines, its stderr raw text; both reach the supervisor as JSON values on one channel, so the supervisor has one place to wait.

| Argument | Type | Meaning |
|---|---|---|
| `child` | `&mut Child` | The started program; its `stdout` and `stderr` handles are taken out of it (a second call finds none). |

Returns a `Receiver<Value>`. Thread one reads `stdout` line by line, at most `MAX_LINE` (16 MiB) per read, and sends each line parsed as JSON; a line that is not JSON becomes `{"op":"protocol_error","msg":"unreadable line from the program: ..."}` (a line longer than `MAX_LINE` is therefore delivered in pieces, each reported as unreadable). Thread two reads `stderr` in 8 KiB blocks and sends `{"op":"stderr","text":...}` (lossy UTF-8; a multi-byte character split between two blocks becomes replacement characters). Each thread ends at end of file, on a read error, or when the receiver has been dropped (a failed send). The channel is closed when both threads end, which `recv` reports as `Incoming::Closed`. No locks; the threads are detached and never joined. Order is kept within one stream, not between the two.

See also: `proc_cpython_start`, `proc_cpython_recv`, `proc_run_handle` (reads `stdout`, `stderr`, `protocol_error`). Tested in `src/procedure/tests.rs`.

### `check` [@ANCHOR: proc_cpython_check]

`Backend::check` for CPython: compiles a source and lists the tools it names, without running any of it, by running `python3 bench_runtime.py --check <file>`. It lets a caller refuse a bad source before a run is made.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `&CPython` | Only `python` is used; the wrapper is forced to `none` for the check. |
| `source` | `&str` | The procedure source text; written to `check_source.py` in `scratch`. |
| `scratch` | `&Path` | An existing directory the check may write to and runs in; the runtime file and the source are written there. Borrowed. |

Returns `Ok(value)` with the JSON the runtime printed: `{"ok": true, "calls": [tool names it calls literally]}` or `{"ok": false, ...}` with the reason (the exact fields are the runtime's, see `procedure_runtime.md`). Errors, all `Err(String)`: a file in `scratch` cannot be written (`<path>: <io error>`); the interpreter cannot be started (`<python>: <io error>`); the check does not finish in 20 s (the process is killed and reaped: "the source check did not finish in 20 s"); the output is not JSON ("the source check gave no result (...)"). Blocking: it polls `try_wait` every 10 ms for up to 20 s on the calling thread, so call it from a thread that may block. Side effects: two files in `scratch`, one short-lived process with the stripped environment, stdin from `/dev/null`, no process group and no `setrlimit` limits. Output is read only after the process has ended; a check that prints more than a pipe buffer holds (about 64 KiB) blocks on its write until the 20 s end (see the report note). The source is only compiled, never executed, so no tool is called and nothing can transmit.

See also: `proc_cpython_no_limits`, `proc_cpython_command_line`, `proc_py_main` in `procedure_runtime.md`. Tested in `src/procedure/tests.rs`.

### `start` [@ANCHOR: proc_cpython_start]

`Backend::start` for CPython: writes the runtime into the run directory, starts the program with its limits and returns the session the supervisor will drive. Called once per run, by `supervise` on the run's own thread.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `&CPython` | The interpreter and the wrapper. |
| `spec` | `&SpawnSpec` | `run_dir` (where `source.py`, `config.json` and `tools.json` already are, and the argument the runtime gets), `scratch` (the working directory, `HOME` and `TMPDIR`; must exist), `limits` (see below). Borrowed. |

The limits applied in the child, between `fork` and `exec` (through `limit`): `RLIMIT_AS` = `memory_mb` MiB (soft and hard), `RLIMIT_CPU` = `cpu_s` seconds soft and `cpu_s + 2` hard (so the program gets `SIGXCPU` first and `SIGKILL` two seconds later), `RLIMIT_NOFILE` = `files`, `RLIMIT_FSIZE` = `file_mb` MiB, `RLIMIT_NPROC` = `user_tasks() + TASK_MARGIN`, `RLIMIT_CORE` = 0, and `PR_SET_PDEATHSIG` = `SIGKILL`, so the program dies when the server thread that started it ends (the supervisor thread lives as long as the run). The process is made leader of a new process group (`process_group(0)`) so `kill` can reach everything it started; stdin, stdout and stderr are pipes; the environment is cleared and replaced by `stripped_env(scratch)`.

Returns `Ok(Box<dyn Session>)`. Errors (`Err(String)`): the runtime file cannot be written (`<path>: <io error>`), or the program cannot be started ("cannot start <python>: <io error>", for instance a missing interpreter). Side effects: writes `bench_runtime.py` (overwriting) into `run_dir`, reads `/proc` (`user_tasks`), starts the process and two reader threads (`pump`). The program is not sandboxed: the limits stop runaways, not an attacker. Not idempotent: each call starts another process. With `wrapper == "systemd-run"` the limits above still apply to the `systemd-run` process before it executes the interpreter, and the scope adds `MemoryMax` and `TasksMax=64`. Nothing here transmits: a tool call the program makes arrives later as a message handled by `exec_call`.

See also: `proc_cpython_command_line`, `proc_cpython_env`, `proc_cpython_limit`, `proc_cpython_user_tasks`, `proc_cpython_pump`, `proc_run_supervise`. Tested in `src/procedure/tests.rs`.

### `send` [@ANCHOR: proc_cpython_send]

`Session::send` for CPython: writes one message to the program's stdin as one JSON line. The supervisor uses it for `reply` and `cancel` messages.

| Argument | Type | Meaning |
|---|---|---|
| `msg` | `&Value` | The message; borrowed, serialized with a trailing newline. |

Returns `Ok(())` once the line is written and flushed. Errors (`Err(String)`): "the program has closed its input" when the session was killed (stdin is `None`); a serialization error; "cannot write to the program: ..." (a broken pipe when the program has ended). The supervisor ignores these errors (`let _ =`), so a reply to a program that has just ended is dropped silently. Blocking: the write blocks if the program does not read its stdin and the pipe is full; the supervisor thread is the only caller. No locks.

See also: `proc_cpython_recv`, `proc_run_supervise`. Tested in `src/procedure/tests.rs`.

### `recv` [@ANCHOR: proc_cpython_recv]

`Session::recv` for CPython: waits for the next message from the program (or its stderr text).

| Argument | Type | Meaning |
|---|---|---|
| `wait` | `Duration` | The longest wait. The supervisor passes 20 ms, which is also the granularity of its timing checks. |

Returns `Incoming::Message(value)` (a protocol line or a `stderr` or `protocol_error` object made by `pump`), `Incoming::Timeout` when nothing came within `wait`, or `Incoming::Closed` when both reader threads have ended (the program closed stdout and stderr, which normally means it ended). A message already queued is returned at once even if `wait` is zero. No errors. Blocks the calling thread up to `wait`.

See also: `proc_cpython_pump`, `proc_cpython_send`, `proc_run_supervise`. Tested in `src/procedure/tests.rs`.

### `kill` [@ANCHOR: proc_cpython_kill]

`Session::kill` for CPython: ends the program and everything it started, at once, and reaps it. Called by the supervisor when the grace time is over or the program lingers after its final message, and by `Drop`.

No arguments beyond `&mut self`; no return value; no error is reported. It sends `SIGKILL` to the program's process group (`kill(-pid)`, which reaches the children and grandchildren the program started, because `start` made it a group leader), then `SIGKILL` to the child itself, waits for it (`wait`, so no zombie is left) and drops stdin so later `send` calls fail. It is safe to call more than once. Blocking: the final `wait` returns quickly after `SIGKILL` unless the process is stuck in uninterruptible I/O. Side effect: a program killed this way runs no cleanup handlers, which is why the safe-state step in `proc_run_finish` exists. The group signal is sent even if the child was already reaped by an earlier `exit_status` or `try_wait`; a process id reused in the meantime would be signalled (unknown how likely; see the report note). The signal goes to the program's own group only, never to the server's.

See also: `proc_cpython_drop`, `proc_run_supervise`. Tested in `src/procedure/tests.rs`.

### `exit_status` [@ANCHOR: proc_cpython_exit_status]

`Session::exit_status` for CPython: says how the program ended, as text for the run's result.

No arguments beyond `&mut self`. Returns the text of the exit status (for example `exit status: 0`, or `signal: 9 (SIGKILL)`), which `proc_run_finish` stores as `exit` in `result.json` and searches for `SIGKILL` and `SIGXCPU` to explain a crash. When the program has not ended after 3 seconds it returns `still running` (and does not kill it). Blocking: polls `try_wait` every 10 ms for up to 3 s on the calling thread. Reaps the child if it has ended. Errors from `try_wait` are also reported as `still running`.

See also: `proc_run_supervise` (calls it after the loop), `proc_run_finish`. Tested in `src/procedure/tests.rs`.

### `pid` [@ANCHOR: proc_cpython_pid]

`Session::pid` for CPython: the operating-system process id of the program (with the `systemd-run` wrapper, of the wrapper process).

No arguments beyond `&self`; returns a `u32`. No errors, no blocking, no side effects. The supervisor stores it in the run record (`pid` in `state.json`) so an operator or a recovery pass can see which process belonged to the run. It stays the same after the process has ended, and the number may later be reused by another process.

See also: `proc_run_supervise`. Tested in `src/procedure/tests.rs`.

### `drop` [@ANCHOR: proc_cpython_drop]

`Drop for PySession`: makes sure no program outlives its session. When the session value is dropped (the supervisor drops it after `exit_status`, or an error path drops it early) a program that is still running is killed with `kill`.

No arguments, no return value, cannot fail. A program that has already ended (`try_wait` reports an exit) is left alone; a `try_wait` error is also treated as "not running" and nothing is killed. Together with the parent-death signal set in `start` this keeps a run from outliving its supervisor.

See also: `proc_cpython_kill`. Tested in `src/procedure/tests.rs`.

## The registry of live runs

### `registry` [@ANCHOR: proc_run_registry]

The process-wide table of live runs, id to `Arc<RunCtx>`. It lets the tools find a run another thread supervises (`lookup`), counts runs per managed directory (`active_in`), lets shutdown find every run (`stop_all`) and lets recovery skip runs this very process owns.

Returns a reference to a `static Mutex<HashMap<String, Arc<RunCtx>>>`, created on first use (`OnceLock`); it lives until the process ends. A run is inserted by `start` before its supervisor thread starts and removed by `supervise` after `finish`, or by `start` itself when the program could not be started. Concurrency: every user locks it briefly and never calls out while holding it, except `stop_all`'s wait loop, which re-locks each time round. A poisoned lock is recovered. Memory only: nothing is persisted here (the record on disk is `state.json`), so a restart begins with an empty registry.

See also: `proc_run_lookup`, `proc_run_active_in`, `proc_run_start`, `proc_run_stop_all`. Tested in `src/procedure/tests.rs`.

### `lookup` [@ANCHOR: proc_run_lookup]

Finds the live run with an id, if this server process has it.

| Argument | Type | Meaning |
|---|---|---|
| `id` | `&str` | The run id (`r` and 14 hex digits, from `store::new_run_dir`); borrowed. Not validated here. |

Returns `Some(Arc<RunCtx>)` (a clone of the shared handle) or `None` when there is no such run in this process: a run that has ended and been removed, a run of an earlier server process, or an unknown id. No errors. Locks the registry briefly. A caller that needs the final state of an ended run reads the run directory instead (`state.json`, `result.json`).

See also: `proc_run_registry`, `proc_run_answer` (uses it), `proc_run_recover_now` (skips a run this process has). Tested in `src/procedure/tests.rs`.

### `active_in` [@ANCHOR: proc_run_active_in]

Counts the live runs that use one managed directory, so the tool can enforce `max_runs` per directory.

| Argument | Type | Meaning |
|---|---|---|
| `root` | `&Path` | A managed directory as `store::root` returns it. Compared with `==` to each run's root; borrowed. |

Returns the number of runs in the registry whose root equals `root` and whose state is active (`starting`, `running`, `waiting_operator`, `cancelling`). A run whose final state is already written but which the supervisor has not yet removed from the table is not counted. Locks the registry, then each run's record in turn. No errors, no side effects.

See also: `proc_run_is_active`, `proc_run_registry`. Tested in `src/procedure/tests.rs`.

### `is_active` [@ANCHOR: proc_run_is_active]

Says whether a run state means the run is still going.

| Argument | Type | Meaning |
|---|---|---|
| `state` | `&str` | A run state as stored in `state.json`. |

Returns true for `starting`, `running`, `waiting_operator` and `cancelling`; false for everything else, including the final states `succeeded`, `failed`, `cancelled`, `timed_out` and `lost` and any unknown text. Pure function. It is used to decide whether a state change sets `ended_unix`, whether a run counts against `max_runs`, and whether `recover_now` must treat a run from an earlier server as lost.

See also: `proc_run_set_state`, `proc_run_active_in`, `proc_run_recover_now`. Tested in `src/procedure/tests.rs`.

### `clip` [@ANCHOR: proc_run_clip]

Cuts a JSON value to a size that is safe to keep in an event or a failure report.

| Argument | Type | Meaning |
|---|---|---|
| `v` | `&Value` | Any JSON value; borrowed, not modified. |

Returns a clone of `v` when its JSON text is at most `CLIP` (2000) characters. Otherwise it returns a JSON string: the first 2000 characters of the JSON text, then `... (N characters in all)` with the full length. A cut value is therefore a string, not the original type. Counts characters, not bytes. Pure function with a cost proportional to the size of `v` (it is serialized once or twice). Used for the arguments and results of calls in the event list, the last-calls list and `last_call` of the record.

See also: `proc_run_exec_call`. Tested in `src/procedure/tests.rs`.

## The run record and its helpers

### `event` [@ANCHOR: proc_run_event]

`RunCtx::event`: appends one entry to the run's event list (`events.jsonl`). The list is what `bench_proc_output` reads, with `seq` as the cursor.

| Argument | Type | Meaning |
|---|---|---|
| `kind` | `&str` | The event kind: `state`, `call`, `emit`, `log`, `step`, `checkpoint`, `trace`, `violation`, `ask`, `answer`, `reserve`, `release`, `safe_state`. |
| `data` | `Value` | The payload; an object has its fields merged into the entry, anything else is stored as `data`. Owned. |

No return value. Locks the event log for the append. The log has a size cap in `store.rs`: past it events are counted (`events_dropped` in the record) and not stored, silently. A failed file write is ignored. Never panics.

See also: `proc_store_event_append` (in `store.rs`), `proc_run_persist` (publishes the counts). Tested in `src/procedure/tests.rs`.

### `persist` [@ANCHOR: proc_run_persist]

`RunCtx::persist`: rewrites `state.json` from the run's record so a reader who has no handle on the running server (another process, an agent after a restart) sees the state, step, counts and the time it began and ended.

No arguments, no return value. Takes the record lock and, inside it, the event log lock, builds the JSON, releases both and then writes the file through `store::write_json` (atomic replace: a reader sees the old or the new file). Fields written: `run`, `state`, `label`, `tier_cap`, `started_unix`, `ended_unix` (null while active), `step` (the last `bench.step` name), `line` (the last traced source line), `last_call` (the clipped last tool call entry), `calls`, `call_errors`, `hook_violations`, `wall_s`, `max_calls`, `pid` (0 until the program started), `note`, `server_pid` and `server_start` (the pid and boot-relative start ticks of this server process, which let `recover_now` tell a live owner from a dead one), `paused_s`, `awaiting` (the pending prompt or null), `cancel_reason`, `events`, `events_dropped` and `language` (always `python`). A write error is ignored. Called after every state change, call, step and violation. Takes the cancel lock for `cancel_reason` while the record lock is held (the cancel lock is never held when the record lock is requested, so there is no cycle).

See also: `proc_run_set_state`, `proc_run_cancel_reason`, `proc_store_write_json`. Tested in `src/procedure/tests.rs`.

### `set_state` [@ANCHOR: proc_run_set_state]

`RunCtx::set_state`: moves the run to a new state, records the change as an event and saves the record.

| Argument | Type | Meaning |
|---|---|---|
| `state` | `&str` | The new state (see the list in the opening of this story). Not checked against the previous state: any change is accepted. |
| `note` | `&str` | Short text for the record's `note`; an empty string keeps the previous note. |

No return value. Sets `ended_unix` to now whenever the new state is not active (a later change to another inactive state moves it again). Appends a `state` event with `state` and `note`, then calls `persist`. The final state is set last by `finish`, after `result.json` has been written, so a reader that sees a final state can read the result.

See also: `proc_run_is_active`, `proc_run_event`, `proc_run_persist`, `proc_run_finish`. Tested in `src/procedure/tests.rs`.

### `request_cancel` [@ANCHOR: proc_run_request_cancel]

Asks a run to stop. Public: the cancel tool calls it with `cancelled`, the supervisor with `timeout` and `operator_timeout`, `stop_all` with `shutdown`.

| Argument | Type | Meaning |
|---|---|---|
| `reason` | `&str` | Why: `timeout` (wall-clock budget), `operator_timeout`, `shutdown`, or any text the cancel tool supplies. Copied. |

No return value, no error. The first reason wins: a later call while a reason is set does nothing, so a run that timed out and is then asked to shut down keeps `timeout`. Only sets a flag; the supervisor notices it within about 20 ms, sends `{"op":"cancel","reason":...}` to the program, gives its cleanup handlers `grace_s` seconds and then kills it. The reason decides the final state: `timeout` and `operator_timeout` give `timed_out`, anything else `cancelled`. Idempotent and safe from any thread; takes the cancel lock briefly.

See also: `proc_run_cancel_reason`, `proc_run_supervise`, `proc_run_finish`, `proc_run_stop_all`. Tested in `src/procedure/tests.rs`.

### `hold` [@ANCHOR: proc_run_hold]

`RunCtx::hold`: takes the instrument reservations this run does not hold yet, for a tool call or an explicit `bench.reserve`. A reservation is an exclusive `flock` on a lock file in the managed directory, so one run at a time may change an instrument.

| Argument | Type | Meaning |
|---|---|---|
| `names` | `&[String]` | Instrument names. Names the run already holds are skipped. Borrowed. |
| `wait` | `Duration` | The longest to wait for another run to let go (`lock_wait_s` for an implicit tool-call reservation; the program's own timeout, 0 to 3600 s, for `reserve`). |

Returns `Ok(())` when every name is held (all or none: `locks::reserve` sorts and de-duplicates the names and releases what it took on failure). Errors (`Err(String)`): an instrument is reserved by another run and the wait ran out or the run was cancelled (the message names the owner and says to wait or call `bench.reserve` with a timeout), or the lock directory or file cannot be created or opened. Blocking: waits up to `wait` in 25 ms steps, polling `cancel_reason` so a cancelled run stops waiting. Holds the run's `held` mutex for the whole wait (so two threads of one run reserve one after the other) and takes the record lock briefly for the label. The held locks stay until `release` or the end of the run (`finish` releases them all). Side effect: lock and `.owner` files under `<root>/locks`.

See also: `proc_locks_reserve`, `proc_run_reserve_op`, `proc_run_exec_call`. Tested in `src/procedure/tests.rs`.

### `paused` [@ANCHOR: proc_run_paused]

`RunCtx::paused`: seconds the run has spent waiting for the operator, so the wait is not charged to the wall-clock budget.

No arguments. Returns the sum of the finished waits (`paused_s` of the record) and the age of the prompt in progress, in seconds as `f64`. 0.0 when the run has never asked. Locks the record, then the prompt, one after the other. The supervisor uses it in `elapsed - paused > wall_s`.

See also: `proc_run_poll_ask`, `proc_run_clear_ask`, `proc_run_supervise`. Tested in `src/procedure/tests.rs`.

### `poll_ask` [@ANCHOR: proc_run_poll_ask]

`RunCtx::poll_ask`: the supervisor's per-tick check of a pending operator prompt. It turns an answer given by `bench_proc_input` into the reply the program is waiting for, and stops a run whose operator never answers.

No arguments. Returns `None` when no prompt is pending, when it is pending and unanswered, or when it has just timed out (it then calls `request_cancel("operator_timeout")`; the limit is the program's own timeout clamped to 1 s up to `operator_timeout_s`). Returns `Some(reply)` with `{"op":"reply","id":<the prompt's id>,"ok":true,"value":<the answer text>}` once an answer is waiting: it clears the prompt and the stored answer, adds the wait to `paused_s`, clears `awaiting`, appends an `answer` event (`answer`, `waited_s`) and sets the state back to `running`. Locks the prompt, the answer and the record in short steps; releases the prompt lock before it writes state. The supervisor sends the reply to the program. Does not run while a cancel has been sent.

See also: `proc_run_answer` (stores the answer), `proc_run_clear_ask`, `proc_run_paused`, `proc_run_handle` (`ask`). Tested in `src/procedure/tests.rs`.

### `clear_ask` [@ANCHOR: proc_run_clear_ask]

`RunCtx::clear_ask`: drops a pending operator prompt when the run is being stopped, so a program that is cancelled while it waits for the operator does not leave the record claiming it still waits.

No arguments, no return value, no error. If a prompt is pending it is removed, the time it had waited is added to `paused_s` and `awaiting` is cleared; with no prompt it does nothing. The unanswered question gets no reply (the program is being cancelled). Locks the prompt, then the record, in that order. Called once, when the supervisor sends the cancel.

See also: `proc_run_poll_ask`, `proc_run_paused`, `proc_run_supervise`. Tested in `src/procedure/tests.rs`.

### `cancel_reason` [@ANCHOR: proc_run_cancel_reason]

`RunCtx::cancel_reason`: the reason a stop was requested, if one was.

No arguments. Returns `Some(reason)` (a copy) or `None`. Locks the cancel flag briefly. It is the test "is this run being stopped": `exec_call` grants the cleanup allowance, `hold` stops waiting, `ask` refuses a new prompt, `finish` picks the final state, and `persist` writes it as `cancel_reason`.

See also: `proc_run_request_cancel`. Tested in `src/procedure/tests.rs`.

## Handling what the program sends

### `exec_call` [@ANCHOR: proc_run_exec_call]

Runs one tool call for the program and builds the reply message. Runs on a short-lived worker thread (one per `call` message) so a slow call does not stop the supervisor from watching the clock and the cancel flag.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&RunCtx` | The run; shared. |
| `tool` | `&str` | The tool name, e.g. `bench_read`. |
| `args` | `&Value` | The tool arguments as the program gave them; borrowed. |

Returns the reply without its `id` (the caller adds it): `{"op":"reply","ok":true,"value":...}` or `{"op":"reply","ok":false,"kind":...,"error":...}`. Error kinds: `budget` (the call count has reached `max_calls`; once a stop has been requested 25 more calls, `CLEANUP_CALLS`, are allowed for cleanup handlers), `policy` or `tier` or `tool` from `ToolIndex::allow` (a procedure tool; above the run's tier cap; not a tool of this bench), `lock` (the instrument is held by another run and `lock_wait_s` ran out), and `tool` (the tool returned an error, or its reply is larger than `MAX_REPLY`, 8 MiB). The order is: budget check and count (a refused call still uses one unit of the budget, including those refused by the tier check), tier check, safe-state bookkeeping (`safe::action_for` and `safe::record`: a call that needs undoing at the end is written to `touched.json` before it runs, so a crash still leaves the record), reservation for any tool above `read_only` whose instrument can be named, then `crate::call` on the dispatcher. The dispatcher applies the tool's own checks, so for a tool that can transmit its tier, lock, band check and transmit gate all apply as for any call; the supervisor adds the run's cap and reservation on top and never lowers one. Afterwards it counts an error, keeps the entry (clipped args and result, milliseconds) in the last-calls list (10), the record's `last_call` and a `call` event, and persists. Blocking: it can wait for a lock and for the instrument. Failed calls are not retried here.

See also: `proc_run_hold`, `proc_run_count_error`, `proc_run_clip`, `proc_tool_allow`, `proc_safe_action_for`, `proc_safe_record`, `proc_run_handle`, `proc_py_call` in `procedure_runtime.md`. Tested in `src/procedure/tests.rs`.

### `count_error` [@ANCHOR: proc_run_count_error]

Adds one to the run's count of failed tool calls (`call_errors`), which appears in `state.json` and in the result.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&RunCtx` | The run. |

No return value, no error. Locks the record briefly. Called for a tier or policy refusal, a lock refusal, a tool error and a too-large reply; a budget refusal is not counted here.

See also: `proc_run_exec_call`, `proc_run_persist`. Tested in `src/procedure/tests.rs`.

### `reserve_op` [@ANCHOR: proc_run_reserve_op]

Carries out the program's `reserve` or `release` request.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&RunCtx` | The run. |
| `op` | `&str` | `release` releases; any other value (the caller passes `reserve`) reserves. |
| `names` | `&[String]` | Instrument names; each must be a configured instrument. |
| `wait` | `f64` | Seconds to wait for another run to let go (reserve only); the caller has already clamped it to 0..3600. |

Returns the reply (without its id): `{"op":"reply","ok":true,"value":null}`, or `ok:false` with `kind:"lock"` and an error text for an empty list ("name at least one instrument"), a name that is not a configured instrument ("no instrument named ... (see bench_list)"), or a reservation that could not be had (from `hold`). Release of a name the run does not hold is accepted silently. A successful call appends a `reserve` or `release` event. Blocking: a reserve may wait up to `wait` seconds (the supervisor runs this on a worker thread). Side effects: lock files, as at `proc_run_hold`.

See also: `proc_run_hold`, `proc_locks_release`, `proc_py_reserve` in `procedure_runtime.md`. Tested in `src/procedure/tests.rs`.

### `handle` [@ANCHOR: proc_run_handle]

Handles one message from the program, by its `op`. The supervisor calls it for every message `recv` returns.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Arc<RunCtx>` | The run; cloned for worker threads. |
| `msg` | `Value` | The message, a JSON object. A missing or non-string `op` is the empty string and falls into the unknown case. |
| `tx` | `&Sender<Internal>` | Where worker threads (and the refused-prompt case) put replies for the supervisor to forward. |

Returns `Some((op, report))` when the message is the program's last one (`done`, `failed` or `cancelled`; `report` is the message's `report` or `{}`), otherwise `None`. By `op`: `call` starts a worker thread running `exec_call` and sends its reply with the call's `id`; `reserve` and `release` likewise run `reserve_op` on a worker thread (timeout default 30 s, clamped 0 to 3600); `emit` stores a value under a key of at most 100 characters in `emitted.json`, refusing a value over 65536 bytes of JSON text or a 501st key with a `log` event; `ask` begins an operator prompt (message cut to 1000 characters, timeout clamped to 1 s up to `operator_timeout_s`, state `waiting_operator`, `awaiting` set with `answer_with: bench_proc_input`) or replies `policy` when a prompt is already pending or the run is being stopped; `log` and `step` record an event (and `step` the record's `step`); `checkpoint` writes `checkpoints/<name>.json` (name reduced to letters, digits, `_` and `-`, 60 characters; data at most 262144 bytes) or logs that it was dropped; `stdout` and `stderr` append to `stdout.txt` or `stderr.txt` with a size cap, setting the record's `note` when text was cut; `trace` records the last line number and an event; `violation` records an audit-hook violation (a list, a count and an event); `ready` logs the Python version; `protocol_error` logs the reader's message; anything else logs "unknown message". The reply to a `call`, `reserve` or `release` is sent by a worker later, so one slow call never holds up the others. Replies are never sent from here directly except the refused prompt. Errors: none returned; every problem is a `log` event.

See also: `proc_run_exec_call`, `proc_run_reserve_op`, `proc_run_poll_ask`, `proc_run_supervise`, `proc_cpython_pump`, and the sending side in `procedure_runtime.md`. Tested in `src/procedure/tests.rs`.

## The supervisor, the final state and the start of a run

### `supervise` [@ANCHOR: proc_run_supervise]

The supervisor of one run, on its own thread from `start` until the run is over. It owns the backend session. It starts the program, tells `start` whether that worked, then loops every 20 ms until the program has ended, forwarding replies, watching the clocks and handling messages; then it calls `finish` and removes the run from the registry.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `Arc<RunCtx>` | The run. |
| `backend` | `Arc<dyn Backend>` | The backend that starts the program (`CPython` in production). |
| `spec` | `SpawnSpec` | Directory, scratch directory and limits for `Backend::start`. Owned. |
| `started` | `Sender<Result<(), String>>` | Receives `Ok(())` once the program is started or the backend's error, once. |

No return value. One loop round: forward the replies the worker threads made to the program; if no final message has come and no cancel was sent, poll for an answered prompt and request `timeout` when `elapsed - paused` passes `wall_s`; when a cancel reason has been set, send `{"op":"cancel","reason":...}` once, clear the prompt and set the state `cancelling`; once `grace_s` has passed since the cancel without a final message, kill the program and note it; once 3 s have passed since the final message and the program has not closed, kill it; then `recv` up to 20 ms and hand a message to `handle`. The loop ends on `Closed` or after a kill. Then `exit_status`, drop the session (which kills a program still running), `finish`, and removal from the registry. If the backend cannot start the program only `started` gets the error and the thread returns without a state change; `start` marks the run `failed`. Timing: the wall-clock budget is measured from the start of the loop, minus operator waits; the supervisor is the only thread that talks to the session. A worker's reply sent after the loop ends is lost.

See also: `proc_run_start`, `proc_run_finish`, `proc_run_handle`, `proc_run_poll_ask`, `proc_run_clear_ask`, `proc_run_paused`, `proc_cpython_start`. Tested in `src/procedure/tests.rs`.

### `finish` [@ANCHOR: proc_run_finish]

Works out the final state of a run from what the program reported and how it ended, makes the instruments safe when needed, writes `result.json`, releases the reservations and sets the final state. Called once by `supervise`.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&RunCtx` | The run. |
| `ended` | `Option<(String, Value)>` | The program's final message `(op, report)`, if it sent one. |
| `killed` | `bool` | True when the supervisor killed the program after the grace time. |
| `exit` | `&str` | The text from `Session::exit_status`. |

No return value. The state: `done` gives `succeeded` (value from the report). `failed` or `cancelled` gives `cancelled` or `timed_out` when a cancel reason was set or the message was `cancelled` (`timed_out` for the reasons `timeout` and `operator_timeout`), otherwise `failed`; the failure report's `kind` is `exception` for `failed`, else the stop kind. No final message and killed gives the stop state with the message that the program did not stop within the grace time. No final message and not killed gives `failed` with kind `crash`, the exit text, a hint when it names `SIGKILL` or `SIGXCPU` (a memory or CPU limit or the out-of-memory killer) and the last 4000 bytes of `stderr.txt`. Unless the run succeeded, every recorded safe-state call (`touched.json`) is run through `safe::run` and logged as `safe_state` events; if any of them failed the failure gets `safe_state_failed` ("SAFE STATE NOT CONFIRMED...") and the state note says so. A failure also gets `last_calls` and `emitted`. `result.json` holds `run`, `state`, `ok`, `value`, `failure`, `emitted`, `hook_violations`, `exit`, `safe_state`, `safe_state_ok`, `calls`, `call_errors`, `elapsed_s`. All reservations are released, then `set_state` records the final state. Blocking: safe-state calls go to the instruments and may take seconds each (three tries each); the supervisor thread waits for them, which is the point: the instruments are safe before the run counts as ended. A file write error is ignored.

See also: `proc_run_supervise`, `proc_safe_run`, `proc_safe_record`, `proc_locks_release`, `proc_run_set_state`. Tested in `src/procedure/tests.rs`.

### `start` [@ANCHOR: proc_run_start]

Starts a run: creates its directory and files, builds its context, registers it and starts the supervisor. The caller (the start tool, `proc_run_tool`) has already validated the parameters and checked `max_runs`.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | The procedure configuration; cloned into the run. |
| `insts` | `&[Instrument]` | The configured instruments; copied into the run. |
| `backend` | `Arc<dyn Backend>` | The backend to run the program in. |
| `p` | `Params` | The validated run parameters: `source`, `args` (JSON), `label`, `cap` (tier cap), `wall_s`, `max_calls`, `limits`, `trace`. Owned. |

Returns `Ok(run id)`. Errors (`Err(String)`): the run directory or one of its files cannot be created or written (`source.py`, `args.json`, `tools.json`, `config.json`, the event log), the program cannot be started (the backend's message; the run is removed from the registry, marked `failed` with that note, and its directory stays), or the program did not start within 20 s ("run <id> did not start within 20 s"; in this case the run stays registered and its supervisor continues). Steps: purge old finished runs (`store::purge`, so the retention and keep limits are applied at each start), make the id and directory with its `scratch`, build the tool index for the cap, write the four files, open the event log, build the `RunCtx` with state `starting` and the server's pid and start ticks, persist, register, start the supervisor thread and wait for its first answer. Blocking: up to 20 s for the program to start. Side effects: files in the run directory, one thread, one process. Nothing is validated again here; a bad `Params` is not detected.

See also: `proc_run_supervise`, `proc_run_registry`, `proc_run_start_ticks`, `proc_run_params` and `proc_run_tool` (in `mod.rs`), `proc_cpython_start`, `proc_store_new_run_dir`. Tested in `src/procedure/tests.rs`.

### `start_ticks` [@ANCHOR: proc_run_start_ticks]

Reads the start time of a process in clock ticks since boot (field 22 of `/proc/<pid>/stat`). With the pid it names one process for good (a pid can be reused, a pid and start time cannot), which is how recovery tells a run whose server is still alive from one whose server died.

| Argument | Type | Meaning |
|---|---|---|
| `pid` | `u32` | The process id. |

Returns `Some(ticks)`, or `None` when `/proc/<pid>/stat` cannot be read (no such process, not Linux) or does not parse. The name field is skipped by splitting after the last `)`, so a program name with spaces or brackets does not shift the fields. Does a small file read; no locks. `start` stores the result for the server's own pid as `server_start` (0 when it cannot be read).

See also: `proc_run_start`, `proc_run_recover_now`. Tested in `src/procedure/tests.rs`.

## Shutdown and recovery

### `shutdown` [@ANCHOR: proc_run_shutdown]

Stops every live run of this server process and waits for them, so the instruments are left in a safe state when the server ends. Called when the server is told to exit.

No arguments, no return value. It is `stop_all(None)`: every run is asked to cancel with reason `shutdown`; see `proc_run_stop_all` for the wait (grace time plus 60 s at most) and what a run that does not finish in time means. Blocking for as long as the runs need.

See also: `proc_run_stop_all`, `proc_run_request_cancel`. Tested in `src/procedure/tests.rs`.

### `stop_all` [@ANCHOR: proc_run_stop_all]

`shutdown` for the runs of one managed directory, or for all of them.

| Argument | Type | Meaning |
|---|---|---|
| `only` | `Option<&Path>` | A managed directory as `store::root` gives it, to stop only the runs that use it; `None` stops all. Borrowed. |

No return value and no error. It takes the matching runs from the registry, calls `request_cancel("shutdown")` on each, then polls every 50 ms until none of them is in the registry any more, for at most the longest `grace_s` of the runs plus 60 s. It returns after that time even if a run is still going (nothing reports it; its supervisor continues and its program is limited by `PR_SET_PDEATHSIG`, which fires when the server exits). Blocking: sleeps in the calling thread. Idempotent: repeating asks again with the same reason (the first reason wins, so a run already stopping keeps its reason). Runs started while it waits are not asked.

See also: `proc_run_shutdown`, `proc_run_registry`, `proc_run_request_cancel`. Tested in `src/procedure/tests.rs`.

### `recover_now` [@ANCHOR: proc_run_recover_now]

Finds runs left active by a server process that no longer exists, marks them `lost` and puts the instruments they touched in their safe state.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | The configured instruments; the managed directories are those of the procedure instruments, and the safe-state calls go through them. Borrowed. |

Returns one JSON entry per run found: `{"run", "safe_state": [results], "retention_days"}`. A run is recovered when its `state.json` is readable, its state is active, its `server_pid` and `server_start` do not match a live process (`start_ticks`) and this process does not have it in the registry. For each: the recorded safe-state calls (`touched.json`, read as an empty list if missing) are run (`safe::run`, blocking on the hardware); `state.json` is rewritten with state `lost`, `ended_unix`, and the note "the server that ran it ended" or "SAFE STATE NOT CONFIRMED"; `result.json` is written with failure kind `lost` and the emitted values; the program of that run is not looked for or stopped (a surviving program should have died with the server through the parent-death signal; unknown otherwise). A directory it cannot read is skipped. Not safe to run twice at once on the same directory (both would act on one run); `recover` runs it once at start. A run recovered with `safe_state_ok` false needs an operator to check the instruments by hand.

See also: `proc_run_recover`, `proc_run_is_active`, `proc_run_start_ticks`, `proc_safe_run`. Tested in `src/procedure/tests.rs`.

### `recover` [@ANCHOR: proc_run_recover]

Runs `recover_now` on a detached thread of its own, so that a safe-state call that waits on hardware does not hold up the server's start.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | Copied (`to_vec`) for the thread. |

Returns at once, nothing. The result of `recover_now` is discarded and the thread is never joined, so the caller cannot learn which runs were recovered except from their `state.json` files. A panic in the thread ends only that thread.

See also: `proc_run_recover_now`. Tested in `src/procedure/tests.rs`.

### `answer` [@ANCHOR: proc_run_answer]

Gives the operator's answer to the prompt a live run is waiting on. It is the host side of `bench_proc_input`.

| Argument | Type | Meaning |
|---|---|---|
| `id` | `&str` | The run id. |
| `text` | `&str` | The answer; at most 4096 characters (counted as characters). Borrowed. |

Returns `Ok({"run": id, "answered": true})`. Errors (`Err(String)`): the answer is longer than 4096 characters; the run is not live in this server (see `bench_proc_status`); the run is not waiting for the operator (the message gives its state). It stores the answer and then waits up to 5 s, polling every 10 ms, for the supervisor to take it; it returns `Ok` after 5 s even if the supervisor has not (for instance when the run is being cancelled), so `answered: true` means "stored", not "the program received it". Blocking up to 5 s. The program gets the reply from `poll_ask` on the next supervisor tick.

See also: `proc_run_poll_ask`, `proc_run_lookup`, `proc_py_ask_operator` in `procedure_runtime.md`. Tested in `src/procedure/tests.rs`.

---
title: The procedure runner
summary: Programs that call bench tools in loops run next to the server; how to write, start, poll and stop one, what the safety model is and what it is not.
see_also: [concepts/safety-tiers, concepts/errors, tools/bench_proc_run, tools/bench_proc_status, tools/bench_proc_output, tools/bench_proc_result, tools/bench_proc_cancel, tools/bench_proc_list, tools/bench_proc_save, tools/bench_proc_load, tools/bench_proc_delete, tools/bench_proc_input]
tools: [bench_proc_run, bench_proc_status, bench_proc_output, bench_proc_result, bench_proc_cancel, bench_proc_list, bench_proc_save, bench_proc_load, bench_proc_delete, bench_proc_input]
verified: fixtures only, 2026-10-29
---

# The procedure runner

A long or repetitive job (a sweep series with retries, a soak round, "step the generator, measure on the scope, stop when the reading is wrong") should not pass through your context one tool call at a time. Write it once as a Python program, start it with [bench_proc_run](tools/bench_proc_run), and poll. The program runs as a process of its own next to this server and calls the same tools you call.

## Start, poll, read

1. `bench_proc_run` with `source` (and `args`, a tier cap, budgets). It returns a run id at once. Use `dry_run` first: it compiles the source and lists the tools it calls with the tier each needs.
2. [bench_proc_status](tools/bench_proc_status): the state, the current step or last tool call, counters.
3. [bench_proc_output](tools/bench_proc_output): progress events after a cursor (what the program emitted, logged, called), stdout and stderr after byte offsets. Pass back the returned cursor and offsets.
4. [bench_proc_result](tools/bench_proc_result): the final value, or the structured failure report.
5. [bench_proc_cancel](tools/bench_proc_cancel) stops a run; [bench_proc_list](tools/bench_proc_list) lists runs.

States: `starting`, `running`, `cancelling`, `succeeded`, `failed`, `cancelled`, `timed_out`. Everything is kept in the managed directory (`dir` of the config entry) for `retention_days`, so the run id and its results stay readable after the run ended and after a restart of the server.

## What the program sees

The program is a script. A module named `bench` is already imported:

| Call | Meaning |
|---|---|
| `bench.args` | the JSON `args` object you passed |
| `bench.call(tool, **arguments)` | call a bench tool; returns the tool's JSON result; a refusal or failure raises `bench.BenchError` with `.tool`, `.message` and `.kind` (`tool`, `tier`, `policy`, `budget`) |
| `bench.emit(key, value)` | store a progress or partial result under `key`; the latest value per key is kept and survives a kill |
| `bench.log(text)`, `bench.step(name)` | a line in the event list; the step name status shows |
| `bench.sleep(seconds)` | sleep; ends early with `bench.BenchCancelled` on a cancel |
| `bench.cleanup(function)` | register a function that runs when the program ends in any way, newest first |
| `bench.checkpoint(name, data)` | save JSON data in the run's directory |
| `bench.tools()` | the tools this run may call: name to tier, description and argument schema |
| `bench.deadline()` | seconds left of the wall-clock budget |
| `bench.retry(function, tries=3, delay=0.5, backoff=2.0)` | call until it stops raising `BenchError` |
| `bench.expect(condition, message, **context)` | raise a structured failure carrying `context` when false |
| `bench.stats(values)` | n, mean, standard deviation, min, max, spread |
| `bench.series(name, count, measure, retries=2, cycle=None, cycle_after=2, max_failed=None)` | take `count` measurements with retries, call `cycle()` (a power cycle) after `cycle_after` failures in a row, list the points that still failed, emit `<name>.progress` |
| `bench.reserve(*instruments, timeout=30)`, `bench.release(...)`, `with bench.reserved(...)` | reserve instruments for this run, waiting up to the timeout for another run |
| `bench.ask_operator(message, timeout=None)` | pause the run until the person at the bench answers through `bench_proc_input`; returns the answer text |
| `bench.result(value)` | the final value of the run (or leave a global named `result`) |

`print()` goes to the run's stdout. numpy and scipy are importable (FFTs, filters, curve fits, complex arithmetic); the standard library is there. The program starts in an empty scratch directory, which is the only place it may write.

## The safety model, and its limits

* **Every tool call goes through the same dispatcher as yours.** The instrument's tier, each tool's own `confirm`, `dry_run` and attestation arguments, reserved-device rules and logging apply to a program exactly as to you. A program cannot skip a confirmation: it has to pass it, and a `confirm` in a program's source is as good as yours, so the person at the bench decides whether a run may use full-tier tools (`tier: full` needs `confirm_full_tier: true`).
* **The tier cap.** A run has a tier cap (default `read_config`, at most the tier of the runner's own entry). A call above it is refused with `kind: tier`. A program can never start or control programs (`bench_proc_*` are refused with `kind: policy`).
* **Budgets.** A wall-clock budget (`wall_s`), a tool-call budget (`max_calls`), a CPU-time limit and an address-space limit (`memory_mb`). At the wall-clock limit or on `bench_proc_cancel` the program is asked to stop: `bench.sleep` and the next `bench.call` raise `BenchCancelled`, the cleanup handlers run for the grace time (`grace_s`, default 5 s), and then the process group is killed. A program that never calls into `bench` cannot hear the request and is killed after the grace time.
* **The audit hook is not a sandbox.** Inside the program an audit hook refuses starting processes, network connections and lookups, `ctypes`, opening device nodes (serial ports, USB, sound) and writing outside the scratch directory, and the error says to use `bench.call`. Each refusal is counted in `bench_proc_status` (`hook_violations`) and listed in the result. **It is not a security boundary.** Code that tries can get round it, the program runs with the server's user rights, and the agent that writes it already has a shell on this machine. It catches accidents and keeps the tier model honest; do not run programs you would not run as that user. For a deployment that wants more, `"wrapper": "systemd-run"` starts each program in a transient user scope with memory and task limits.
* **Several runs at once** (`max_runs`, default 2). More are refused with a clear error; instruments are reserved per run, see below.

## When a run fails

`bench_proc_result` returns a structured report instead of only a traceback: the error type and message, the traceback text, **the local variables of every frame in the program's own code** (each value cut to 200 characters, a bounded number of variables), the `context` given to `bench.expect`, the last ten tool calls with their arguments and results, and everything that had been emitted. The state tells how it ended: `failed` (an error in the program, or the program died: kind `crash`), `cancelled`, `timed_out` (`reason`: `timeout`; the report is from the place the program was stopped).

## Safe state

When a run ends other than by success (an error, a cancel, a time-out, a crash of the program, a shutdown of the server) the server puts the instruments the run changed in their safe state, through the normal dispatcher, after the cleanup handlers had their turn: generator off (`bench_vna_generator_off`), the transmit-test rig released (`bench_hamlib_safe_state`), the rotator stopped (`bench_rotator_stop`), the scope's trigger back to AUTO, the example instrument off. The supply output is switched off only if the run switched it on (`bench_psu_output` with `on: true`, not a `dry_run`) and the runner's `safe_state_psu_off` is not false. The calls a run makes are recorded before they are dispatched, so a run that died in the middle of one is still covered. The outcome is in `bench_proc_result` as `safe_state`; if a safe state could not be confirmed the failure carries `safe_state_failed` saying so, the state note reads `SAFE STATE NOT CONFIRMED`, and the instrument must be checked by hand. A run that succeeds is not touched: its program decided what to leave on. A tool that switches something on and has no row in the safe-state table does not get this; a program that uses one must undo it itself in a cleanup handler. The Hamlib module's own transmit watchdog and its confirm and attestation checks apply to every call as for any caller.

## Several runs and instrument reservations

A tool that changes an instrument (any tier above `read_only`) reserves the instrument for the run until the run ends. A second run that calls such a tool on the same instrument is refused with `kind: lock` and a message that names the run holding it; for `lock_wait_s` seconds (default 2) it waits first. Reads share. A program can reserve for a block of code and wait for it: `with bench.reserved("gen", "scope", timeout=60): ...`, or `bench.reserve(...)` and `bench.release(...)`. Several names are taken all or none, in sorted order, with the timeout, so two runs that want the same instruments in opposite order cannot deadlock. The reservations are lock files in the managed directory, held by the operating system, so they are released when the run ends even if the server was killed, and they hold between several servers that share the directory. They bind procedure runs only: a direct call by an agent, or by another program on the machine, does not look at them, and exclusive use of the hardware against other agents is the caller's business.

## Restart of the server

The program is killed when its server dies. The next server to start finds the runs that were active when a server process ended (it checks that process by its id and start time, so a live server's runs are left alone), marks them `lost`, and puts the instruments they touched in their safe state; `bench_proc_result` of such a run holds what had been emitted and the safe-state outcome. A SIGTERM or an interrupt makes the server cancel its runs with the reason `shutdown` and wait for their cleanup and safe state before it exits. A SIGKILL leaves the safe state to the next server, which may be later.

## Saved procedures

[bench_proc_save](tools/bench_proc_save) keeps a program under a name with a description and the tier it needs (checked against the tools it names), as versions. [bench_proc_load](tools/bench_proc_load) reads one, [bench_proc_delete](tools/bench_proc_delete) removes one (with `confirm`), [bench_proc_list](tools/bench_proc_list) lists them with the runs. `bench_proc_run` with `name` (and optionally `version`) runs one: the tier cap defaults to the declared tier, the label to `name@version`, and `args` are the caller's.

## Operator steps

A step only a person can do (connect a load, move a cable, press a button on an instrument) is `answer = bench.ask_operator("message")`. The run pauses in the state `waiting_operator`; [bench_proc_status](tools/bench_proc_status) shows the message in `awaiting`. Tell the person at the bench, then [bench_proc_input](tools/bench_proc_input) with their answer. The wait is not part of the wall-clock budget (`bench.deadline()` leaves it out), and has its own limit, `operator_timeout_s`.

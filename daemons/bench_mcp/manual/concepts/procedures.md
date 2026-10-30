---
title: The procedure runner
summary: Programs that call bench tools in loops run next to the server; how to write, start, poll and stop one, what the safety model is and what it is not.
see_also: [concepts/safety-tiers, concepts/errors, tools/bench_proc_run, tools/bench_proc_status, tools/bench_proc_output, tools/bench_proc_result, tools/bench_proc_cancel, tools/bench_proc_list]
tools: [bench_proc_run, bench_proc_status, bench_proc_output, bench_proc_result, bench_proc_cancel, bench_proc_list]
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
| `bench.result(value)` | the final value of the run (or leave a global named `result`) |

`print()` goes to the run's stdout. numpy and scipy are importable (FFTs, filters, curve fits, complex arithmetic); the standard library is there. The program starts in an empty scratch directory, which is the only place it may write.

## The safety model, and its limits

* **Every tool call goes through the same dispatcher as yours.** The instrument's tier, each tool's own `confirm`, `dry_run` and attestation arguments, reserved-device rules and logging apply to a program exactly as to you. A program cannot skip a confirmation: it has to pass it, and a `confirm` in a program's source is as good as yours, so the person at the bench decides whether a run may use full-tier tools (`tier: full` needs `confirm_full_tier: true`).
* **The tier cap.** A run has a tier cap (default `read_config`, at most the tier of the runner's own entry). A call above it is refused with `kind: tier`. A program can never start or control programs (`bench_proc_*` are refused with `kind: policy`).
* **Budgets.** A wall-clock budget (`wall_s`), a tool-call budget (`max_calls`), a CPU-time limit and an address-space limit (`memory_mb`). At the wall-clock limit or on `bench_proc_cancel` the program is asked to stop: `bench.sleep` and the next `bench.call` raise `BenchCancelled`, the cleanup handlers run for the grace time (`grace_s`, default 5 s), and then the process group is killed. A program that never calls into `bench` cannot hear the request and is killed after the grace time.
* **The audit hook is not a sandbox.** Inside the program an audit hook refuses starting processes, network connections and lookups, `ctypes`, opening device nodes (serial ports, USB, sound) and writing outside the scratch directory, and the error says to use `bench.call`. Each refusal is counted in `bench_proc_status` (`hook_violations`) and listed in the result. **It is not a security boundary.** Code that tries can get round it, the program runs with the server's user rights, and the agent that writes it already has a shell on this machine. It catches accidents and keeps the tier model honest; do not run programs you would not run as that user. For a deployment that wants more, `"wrapper": "systemd-run"` starts each program in a transient user scope with memory and task limits.
* **Several runs at once** (`max_runs`, default 2). More are refused with a clear error.

## When a run fails

`bench_proc_result` returns a structured report instead of only a traceback: the error type and message, the traceback text, **the local variables of every frame in the program's own code** (each value cut to 200 characters, a bounded number of variables), the `context` given to `bench.expect`, the last ten tool calls with their arguments and results, and everything that had been emitted. The state tells how it ended: `failed` (an error in the program, or the program died: kind `crash`), `cancelled`, `timed_out` (`reason`: `timeout`; the report is from the place the program was stopped).

## Not done yet in this version

Safe-state actions on cancel, time-out, crash and restart; instrument locks between runs; saved procedures (`bench_proc_save`, `bench_proc_load`, `bench_proc_delete`); operator prompts (`bench.ask_operator`, `bench_proc_input`); a restart of the server marking runs lost. Until they ship, **a program that switches something on must switch it off itself in a cleanup handler**, and a run killed by the kill (not the cancel request) leaves the instruments as they were.

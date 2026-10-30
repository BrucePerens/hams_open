---
title: Calibration with operator steps
summary: The shipped procedure example-calibration-sequence: the VNA calibration where a person connects each standard and answers through bench_proc_input.
see_also: [concepts/procedures, tools/bench_proc_run, tools/bench_proc_load, tools/bench_proc_input, tools/bench_vna_calibrate_measure, tools/bench_vna_calibrate_build, procedures/calibrate-the-vna]
verified: fixtures only, 2026-10-29
---

# Calibration with operator steps

The saved example `example-calibration-sequence` (tier `read_config`) asks the operator to connect each standard (`open`, `short`, `load`, `thru`) and waits. The run is in the state `waiting_operator`; [bench_proc_status](tools/bench_proc_status) shows the message in `awaiting`. Tell the person, wait until they have done it, then answer with [bench_proc_input](tools/bench_proc_input) (`done`, or `abort` to stop). The waiting does not use the run's wall-clock budget. After each answer the standard is measured with `bench_vna_calibrate_measure`, and at the end `bench_vna_calibrate_build` builds the calibration file; apply it as in [calibrate-the-vna](procedures/calibrate-the-vna).

```python
# example-calibration-sequence: the VNA calibration with the steps only a person can do. For each standard the
# run asks the operator to connect it and waits for the answer (bench_proc_input), measures it, and at the end builds
# the calibration file. The operator can answer "abort" to stop; the standards connected are then not measured.
#
# Arguments (bench.args), all optional; the defaults are for the VNA calibration tools:
#   session       the name the standards are stored under (default "cal")
#   cable         what is between the ports and the reference plane, as text (default "none")
#   start_hz, stop_hz, points   the sweep of every standard (default 1 MHz to 30 MHz, 21 points)
#   standards     the order (default open, short, load, thru)
#   measure_tool  default bench_vna_calibrate_measure, measure_args extra arguments for it (for example the instrument)
#   build_tool    default bench_vna_calibrate_build, build_args extra arguments for it
args = bench.args
session = args.get("session", "cal")
settings = {
    "start_hz": args.get("start_hz", 1e6),
    "stop_hz": args.get("stop_hz", 30e6),
    "points": args.get("points", 21),
    "cable": args.get("cable", "none"),
}
measured = []
for standard in args.get("standards", ["open", "short", "load", "thru"]):
    bench.step(f"connect {standard}")
    answer = bench.ask_operator(
        f"Connect the {standard} standard at the reference plane (cable: {settings['cable']}), then answer 'done'. Answer 'abort' to stop."
    )
    bench.expect(answer.strip().lower() != "abort", "the operator stopped the calibration", measured=measured)
    bench.step(f"measure {standard}")
    bench.call(
        args.get("measure_tool", "bench_vna_calibrate_measure"),
        session=session,
        standard=standard,
        **settings,
        **args.get("measure_args", {}),
    )
    measured.append(standard)
    bench.emit("measured", list(measured))
bench.step("build")
built = bench.call(args.get("build_tool", "bench_vna_calibrate_build"), session=session, **args.get("build_args", {}))
bench.result({"measured": measured, "built": built})
```

If the operator answers `abort` the run fails with `the operator stopped the calibration` and the standards measured so far in its report. Not run on a real VNA: the tests answer the prompts through `bench_proc_input` against example instruments.

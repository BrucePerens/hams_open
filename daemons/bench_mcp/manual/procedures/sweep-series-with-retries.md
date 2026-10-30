---
title: Sweep series with retries and a power cycle
summary: The shipped procedure example-sweep-series: repeat a VNA sweep, retry failures, power-cycle after failures in a row, tabulate the spread.
see_also: [concepts/procedures, tools/bench_proc_run, tools/bench_proc_load, tools/bench_vna_sweep, tools/bench_vna_power_cycle]
verified: fixtures only, 2026-10-29
---

# Sweep series with retries and a power cycle

The saved example `example-sweep-series` (`bench_proc_run` with `name: "example-sweep-series"`, tier `read_config`) repeats one sweep `count` times. A sweep that fails is tried again `retries` times; after `cycle_after` failures in a row the unit is power-cycled once (`bench_vna_power_cycle`, through its hub) and the sweep gets one more try. A point that still fails is listed, and the run fails with the failed points in its report if more than `max_failed` points fail. The result is a table with the mean, minimum and maximum magnitude of every sweep and the spread of the means over the sweeps: the number to compare between two cables or two days. Every tool name and argument is a default that `args` can replace; the tests run this same text against a mock bench in which an example instrument stands in for the VNA.

```python
# example-sweep-series: take the same VNA sweep several times, retry a failed sweep, power-cycle the unit after
# a few failures in a row, and tabulate how much the magnitude spreads from sweep to sweep.
#
# Arguments (bench.args), all optional; the defaults are for the VNA on this bench:
#   count          sweeps to take (default 5)
#   retries        retries of one sweep before it is given up (default 2)
#   cycle_after    failures in a row that trigger one power cycle (default 2); 0 turns power cycling off
#   sweep_tool     the tool that measures (default bench_vna_sweep), sweep_args its arguments
#   cycle_tool     the tool that power-cycles the unit (default bench_vna_power_cycle), cycle_args its arguments
#   rows_key       where the sweep reply lists its rows (default "rows"), row_key the magnitude in each row
#                  (default "s21_db"); a reply with a plain number under value_key is used as it is
# Returns: a table with one line per sweep (mean, min and max magnitude over frequency) and the spread of the
# means over the sweeps, the failures and the power cycles. Emits "sweep.progress" after every sweep.
args = bench.args
sweep_tool = args.get("sweep_tool", "bench_vna_sweep")
sweep_args = args.get("sweep_args", {"start_hz": 1e6, "stop_hz": 30e6, "points": 21})
cycle_tool = args.get("cycle_tool", "bench_vna_power_cycle")
cycle_args = args.get("cycle_args", {})
rows_key = args.get("rows_key", "rows")
row_key = args.get("row_key", "s21_db")
value_key = args.get("value_key", "voltage_v")
cycle_after = int(args.get("cycle_after", 2))
table = []


def one_sweep(index):
    bench.step(f"sweep {index + 1}")
    reply = bench.call(sweep_tool, **sweep_args)
    rows = reply.get(rows_key)
    if rows:
        magnitudes = [row[row_key] for row in rows]
    else:
        magnitudes = [reply[value_key]]
    line = bench.stats(magnitudes)
    table.append({"sweep": index + 1, "mean": line["mean"], "min": line["min"], "max": line["max"]})
    return line["mean"]


def power_cycle():
    bench.log(f"power cycling with {cycle_tool}")
    bench.call(cycle_tool, **cycle_args)


series = bench.series(
    "sweep",
    int(args.get("count", 5)),
    one_sweep,
    retries=int(args.get("retries", 2)),
    cycle=power_cycle if cycle_after > 0 else None,
    cycle_after=max(cycle_after, 1),
    max_failed=int(args.get("max_failed", 1)),
)
bench.result(
    {
        "table": table,
        "spread": series["stats"]["spread"] if series["stats"] else None,
        "stats": series["stats"],
        "failed": series["failed"],
        "power_cycles": series["cycles"],
    }
)
```

Start it with `args` such as `{"count": 10, "sweep_args": {"start_hz": 1e6, "stop_hz": 100e6, "points": 51}}`. Poll `bench_proc_output` for the `sweep.progress` events (done, of, failed, cycles). A real VNA run on hardware has **not** been done: the VNA tools are exercised by their own tests and by hand, and this procedure only against the mock bench.

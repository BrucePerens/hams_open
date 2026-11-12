# bench_mcp anchor-doc oddities: triage (2026-11-11)

The anchor-doc pass over `daemons/bench_mcp` (docs/stories/*.md) reported code oddities. Each was checked against the code and
sorted as a real bug (fixed, with a test that failed before), intended behaviour (story wording fixed), or documentation that was
itself wrong. PR numbers are hams_open pull requests.

| Oddity | Verdict | PR |
|---|---|---|
| `parse_transmit` rejects a config that sets `operator_alert` | bug, fixed | 1696 |
| non-numeric `frequency_hz`/`power_w`/`seconds` reported as "is required" | bug, fixed | 1696 |
| TX `reason` length rule counted bytes | bug, fixed | 1696 |
| power sweep did not read the mode back | bug, fixed | 1697 |
| sweep reported the requested power when a step was raised to the rig minimum | bug, fixed (`raised_to_minimum`, `setting` is the one used) | 1697 |
| preflight cap check used `f_req.min(1.0)` (over-full-scale request stopped late) | bug, fixed | 1697 |
| sweep total keyed time limited to 60 s, `max_seconds` per step only | intended (each step is one keyed interval); wording | 1698 |
| `watch` compared polls with `zip` (events on wrong names when `passband_hz` came and went) | bug, fixed (by name) | 1698 |
| `safe_state` fresh-connection path lowered power with `lower_power` false; address looked up twice | bug, fixed | 1698 |
| `rigd` ignores the result of `release_after_crash` before killing the leftover daemon | intended (the daemon belongs to a dead server); wording | 1698 |
| `watch` loses its events when the link fails | intended (error with next step); wording | 1698 |
| `bench_gla_capture` always refuses `pull` up/down (`confirm` not forwarded, not in schema) | bug, fixed | 1701 |
| NanoVNA `stop_hz` defaults to `start_hz` then always fails `check_range` | dead default, now required with a clear message | 1701 |
| `stderr_tail` is the first 2000 bytes, not the last | bug, fixed | 1702 |
| purge deletes a run whose `state.json` has no `state` | bug, fixed (left alone) | 1702 |
| purge: missing `ended_unix` counts as 0; `retention_days` "not validated" | first intended; second was wrong documentation (config validates 0 to 3650) | 1702 |
| runtime failure context: "NaN" replaced inside strings; infinity crashed `Wire.send` | bug, fixed (`finite`) | 1702 |
| `clip_around` sliced the original text at a byte offset of the lower-cased copy | bug, fixed (panic or off-window with U+0130) | 1703 |
| checklist scans failing on main: literal credential-word list and a serial-shaped example in two stories | doc bug, fixed | 1703 |
| glasgow `revision` runs read-only `glasgow list` on a dry run | intended (needs the revision limits); wording | 1703 |
| audio config: a mistyped value (for example a string `volume_cap`) silently kept the default | bug, fixed (error naming the key) | 1704 |
| PSU `set` sent the voltage before the current was checked; non-number setpoint ignored | bug, fixed | 1706 |
| rx888 and radioctl TX `mode` that is not text treated as absent | bug, fixed | 1706 |
| gla `decode` options of the wrong JSON type replaced by defaults | bug, fixed | this PR |
| procedure config: unknown keys ignored | intended (project-wide convention for instrument entries); not changed | - |
| `checklist` oddities (README prose only entry, legacy driver list in a function, tests citing manual anchors) | intended; wording | - |
| event/log write failures are dropped (procedure events, TX keyed-interval log) | intended best effort; not changed | - |
| non-string `instrument`, `bus`, `tier` arguments treated as absent in several picks | intended leniency (a named instrument still has to match, or the single one is used); not changed | - |
| vnwa sweep config of the wrong JSON type ignored; scope measurement names that are not text dropped; others of this kind | not fixed (low risk, measurement settings only); left in the stories | - |

The story files were the source: the separate report file named in the assignment
(`tmp/reports/bench_mcp_anchor_slices.md`) was not on disk, so the triage works from the 26 story files. About 28 of the
reported items are covered above; the remaining ones are of the "wrong JSON type counts as absent" kind in measurement-setting
arguments and were left as documented.

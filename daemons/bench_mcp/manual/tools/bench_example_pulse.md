---
title: bench_example_pulse
summary: Example instrument: drive the output at a level for a short time, then off and verified off. Full tier, dry_run, confirm.
see_also: [instruments/example/limits, tools/bench_example_safe_state, concepts/safety-tiers]
tools: [bench_example_pulse]
instrument: example
tier: full
verified: fixtures only
---

# bench_example_pulse

The model of a tool that actuates. Order of events: the arguments are checked against the rating and the owner's config; with `dry_run: true` the plan is returned and nothing is sent; without `confirm: true` the call is refused; then the instrument is identified, refused if its output is already on, given its own time limit (`OUTP:TMO`), set to `level_v`, switched on for `pulse_s`, and switched off and read back, on every path including failure.

## Arguments

<!-- arguments -->

`confirm` must come from the person at the bench, not from your own judgement. `level_v` is in volts, `pulse_s` in seconds.

## Returns

`{"level_v": 0.5, "pulse_s": 1.0, "output": "off", "verified_off": true}`. With `dry_run`: `{"dry_run": true, "sent": [], "would": [...]}`.

## Failure modes

* `the pulse failed (...); the output was switched off and verified off`: nothing is left on.
* `SAFE STATE NOT CONFIRMED`: do what the message says by hand, at once. The instrument's own time limit ends the pulse within one second after `pulse_s` even if the server died.
* `the output is already ON`: call [bench_example_safe_state](tools/bench_example_safe_state), find out who switched it on.

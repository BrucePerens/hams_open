---
title: bench_gla_pattern
summary: Pattern generator on the Glasgow: square wave, PWM, pulse train, bit pattern or steps, planned with the real frequency and duty, host-timed, bounded, pins released at the end.
see_also: [tools/bench_glasgow_pattern, tools/bench_glasgow_safe_state, instruments/glasgow/logic-analyzer]
tools: [bench_gla_pattern]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_gla_pattern

Drives pins. `pattern.type` is `square` or `pwm` (`frequency_hz` 0.1 to 2000, `duty`, and `cycles` or `duration_s`), `pulses` (`width_ms`, `gap_ms`, `count`), `bits` (`bits` as text of 0 and 1, `bit_ms`, `repeat`) or `steps` (`value` masks and `hold_ms`). `pin` is the index among `pins`; other pins stay low. Holds are 0.5 to 5000 ms in 0.1 ms steps and come from the host's timer (about 1 ms of jitter): the tool refuses a frequency that needs a shorter hold, and the answer's `planned.achieved` gives the frequency and duty that will really come out after rounding. Faster or more exact timing needs an FPGA-timed generator, which this tool does not have.

The rules of [bench_glasgow_pattern](tools/bench_glasgow_pattern) apply and are enforced there: the port's I/O voltage must have been set by `bench_glasgow_port_set`, `confirm` must be true, the whole run is at most `max_capture_s`, and every pin is released at the end, even on failure. `dry_run` returns the plan and sends nothing. Use [bench_glasgow_safe_state](tools/bench_glasgow_safe_state) if anything looks wrong.

## Arguments

<!-- arguments -->

## Returns

`{"planned": {"steps", "repeat", "total_s", "achieved": {"frequency_hz", "duty", ...}, "pins": [...]}, "backend": {...}}`.

## Failure modes

A pattern the host timer cannot make, more than 256 steps in a round, a total longer than `max_capture_s`, more than 8 pins, plus the backend's refusals (no voltage set, no confirm).

## Example

`bench_gla_pattern {"pins": "A0", "pattern": {"type": "pwm", "frequency_hz": 200, "duty": 0.25, "duration_s": 2}, "confirm": true}`

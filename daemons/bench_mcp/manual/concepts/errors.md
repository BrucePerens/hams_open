---
title: Error model and recovery
summary: What a tool error means, which errors to retry, how the VNA and USB recovery escalate, and when to stop and ask.
see_also: [concepts/safety-tiers, concepts/etiquette, instruments/vna/monitoring-and-recovery, instruments/usb-hubs/troubleshooting]
verified: code only, 2026-10-29
---

# Error model and recovery

A failed tool call returns an error result whose text says what failed. There are no error codes; read the text. Classes:

1. **Refused before any hardware is touched**: wrong tier (`needs tier ...`), an unknown or ambiguous instrument, a bad or out-of-range argument (the message names the argument and the allowed range), a safety refusal (a supply setpoint above the cap, `confirm_inputs_disconnected` missing, a protected USB device). Do not retry the same call. Fix the argument, or stop: a tier or protection refusal is a decision of the person who owns the bench.
2. **The instrument did not answer**: a timeout or connection error from the scope or supply (check power, cabling, the address in the configuration), a VNA that is not found or not answering. One retry after a few seconds is reasonable; a second identical failure means the instrument is the problem.
3. **A measurement that refused its own result**: `bench_capture` on a stopped scope, a VNA with no reference signal, a SDR capture whose samples did not arrive in real time. The text says what to change.

Each instrument has its own lock: one call at a time per instrument, and a long call on one instrument does not hold up another.

## Escalation for the VNA and its USB link

The VNA driver retries a failed point itself (re-measurement of the point, up to 10 attempts when the cause is an audio buffer overrun). After repeated failures of a point it cold-resets the unit once through its power-switching hub (if `hub` and `hub_port` are configured) and tries again; only then does the call fail, with every attempt listed. If a sweep fails after that:

1. `bench_status` on the VNA: is the USB device present and answering? Are both codecs found?
2. `bench_vna_power_cycle` (needs the hub configured; about 12 s; the unit loses all settings, a running generator is off). The "silent state" (reference channels read about zero, the sweep says there is no signal on the reference channel) is cleared by it.
3. `bench_usb_diagnose` and `bench_usb_history` for a device that is slow, flapping or missing; the USB ladder (reset, deauthorize and authorize, driver unbind and bind, then port power) escalates from least to most disruptive.
4. Still failing: stop, report the evidence (the error text, the diagnose findings) and ask the person at the bench. Do not loop on power cycles.

Never power-cycle or reset a device that another job is using; see [several instruments](concepts/several-instruments).

## What an error never means

An error does not mean the settings were left half applied in a harmful way: tools that change several settings report in `sent` what was sent. After an error from a settings tool, read the state back with `bench_status` before assuming anything.

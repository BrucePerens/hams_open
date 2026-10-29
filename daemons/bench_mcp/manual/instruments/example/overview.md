---
title: Example instrument overview
summary: What the imaginary EXAMPLE-1000 is, which tools drive it and what each tier allows. The model for an overview page.
see_also: [instruments/example/setup, instruments/example/limits, concepts/safety-tiers]
tools: [bench_example_status, bench_example_measure, bench_example_pulse, bench_example_safe_state]
instrument: example
verified: fixtures only
---

# Example instrument overview

An overview page says, in this order: what the instrument is and which model and firmware the driver was written against; what it is for; the tools, one line each, with their tier; what the driver deliberately does not do.

The EXAMPLE-1000 is an imaginary signal source with a switchable output (0 to 5 V rated), a level setting and a voltmeter input, spoken to in SCPI over TCP port 5025. The driver was written against a simulator and one recorded session, not against hardware.

| Tool | Tier | Does |
|---|---|---|
| [bench_example_status](tools/bench_example_status) | read_only | identity, output on or off, level |
| [bench_example_measure](tools/bench_example_measure) | read_only | the meter voltage, mean and spread of 1 to 10 readings |
| [bench_example_safe_state](tools/bench_example_safe_state) | read_config | output off, read back to prove it |
| [bench_example_pulse](tools/bench_example_pulse) | full | output on at a level for a short time, then off, verified |

Not done on purpose: no raw command tool, no output that stays on after a call returns, no level above the owner's cap.

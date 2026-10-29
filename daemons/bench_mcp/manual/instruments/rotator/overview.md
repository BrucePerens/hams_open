---
title: Rotator tools
summary: What the bench_rotator_* tools do, the tiers, and what is left out.
see_also: [instruments/rotator/setup, instruments/rotator/operation, instruments/rotator/limits, instruments/rotator/troubleshooting, instruments/rotator/protocol]
tools: [bench_rotator_status, bench_rotator_point, bench_rotator_stop, bench_rotator_park]
instrument: rotator
tier: read_only
verified: dummy rotator only, 2026-10-29
---

# Rotator tools

A rotator turns an antenna in azimuth and elevation. The tools speak to Hamlib's `rotctld` daemon over its documented text protocol, the same protocol and the same client as the [Hamlib rig tools](instruments/hamlib/overview); Hamlib itself is not linked (it is LGPL). One configured instrument is one rotator.

| Tool | Tier | Does |
|---|---|---|
| [bench_rotator_status](tools/bench_rotator_status) | read_only | position, limits, info and whether it is moving |
| [bench_rotator_point](tools/bench_rotator_point) | read_config | point at an azimuth and elevation inside the limits; optional wait for arrival; `dry_run` |
| [bench_rotator_stop](tools/bench_rotator_stop) | read_config | stop the motion and check that it stopped; works while another call runs |
| [bench_rotator_park](tools/bench_rotator_park) | read_config | send the rotator to its park position |

## What is left out

The daemon is not managed by this server: the owner starts `rotctld` for the rotator and the config names its port (a managed `rotctld`, like the one the rig tools have, is not done). There is no raw command, no calibration, no sun or satellite tracking and no speed setting.

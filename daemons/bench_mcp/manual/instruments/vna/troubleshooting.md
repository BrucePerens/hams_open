---
title: VNA troubleshooting
summary: Symptom, cause and action for the VNWA: silent state, EPROTO, unit not found, audio overruns.
see_also: [instruments/vna/monitoring-and-recovery, concepts/errors, tools/bench_vna_power_cycle]
instrument: vna
verified: in progress
---

# VNA troubleshooting

In progress. The essentials until it is written:

| Symptom | Cause | Action |
|---|---|---|
| a sweep says there is no signal on the reference channel(s) and the message says all four channels are dead | nothing leaves the TX port: a loose TX SMA connector, pad, tee or lead (2026-10-29: the TX SMA was loose; the unit answered every request, a power cycle and a USB reset did not help, the scope saw no carrier) | check the TX connector, the pad, the tee and the leads first, then run a sweep again |
| a sweep says there is no signal on the reference channel(s) but the measured channels have signal | the unit's "silent state", seen after power-up or an interrupted run | [bench_vna_power_cycle](tools/bench_vna_power_cycle); the unit keeps no settings across it |
| `bench_status` says `present but not answering` or `not found or no permission` | the unit hung, or the USB permission is missing | power cycle; check `bench_usb_topology` for the device |
| intermittent transfer error (EPROTO, -71) on the unit's control requests | USB link trouble; about 0.6 % of attempts still fail at the first try | the driver measures the point again; see [monitoring and recovery](instruments/vna/monitoring-and-recovery) |
| a generator will not switch off after AM | the unit modulates in real time and refuses most requests | `bench_vna_power_cycle` |

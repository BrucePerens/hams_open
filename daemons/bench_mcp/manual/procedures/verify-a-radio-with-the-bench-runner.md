---
title: Verify a radio with the bench runner
summary: How the radioctl bench runner and the bench_mcp transmit tools fit together on a bench day, and which one gates which step.
see_also: [procedures/test-a-transmitter-into-a-dummy-load, procedures/check-a-receiver, instruments/hamlib/limits]
tools: [bench_hamlib_stations, bench_hamlib_load_baseline, bench_hamlib_load_check, bench_hamlib_tx_key_test, bench_hamlib_safe_state]
instrument: hamlib
verified: code only, 2026-11-02 (runner tested against virtual rigs; nothing run on hardware)
---

# Verify a radio with the bench runner

The list of hardware hand-checks, their order and what each one unlocks is `daemons/radioctl/docs/BENCH_VERIFICATION.md`. Its guided runner is `python3 daemons/radioctl/tools/bench_verify.py verify RIG_ID --port usb:VID:PID`. It speaks to `radioctld`, not to Hamlib, so it checks radioctl's own descriptions; the tools of this MCP check the radio and the load from outside.

1. Receive and control first: the runner's phases A and B (gate closed, `radioctl-selftest`, which uploads its report automatically, reads through `radioctld`). `bench_hamlib_stations` and [check a receiver](procedures/check-a-receiver) cover the receive audio.
2. Before any transmit step, with the dummy load connected: `bench_hamlib_load_baseline` with `confirm_dummy_load` from the person at the bench, then `bench_hamlib_load_check` (it should say `consistent_with_a_load`).
3. Transmit steps of the runner (phase C) are dry runs unless `--transmit` is given and the phrase `DUMMY LOAD ATTACHED` is typed; the runner also refuses them unless phase A and the read-only checks have passed in the same run, the port is a by-id path or `usb:VID:PID` (never a bare `ttyUSBn`) and `--max-transmit-s` is 10 or less. The power and spectrum on the load come from the scope (`bench_screenshot`, `bench_scope_spectrum`) or `bench_hamlib_tx_key_test` run from this MCP.
4. If anything looks wrong: `bench_hamlib_safe_state`, then the radio's power switch.
5. The runner writes `BENCH-REPORT-<rig>-<date>.txt` and `verified.toml.proposed`. Neither is uploaded by the runner (the self-test step uploads its own report); a maintainer applies the proposal by hand.

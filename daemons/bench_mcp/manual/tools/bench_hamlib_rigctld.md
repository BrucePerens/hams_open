---
title: bench_hamlib_rigctld
summary: Status, start, stop or restart of the managed rigctld of a rig. It listens on 127.0.0.1 only.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_rigctld]
instrument: hamlib
tier: read_config
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_rigctld

For a rig configured with `model`, this server runs the rigctld. `status` says whether it runs, its port, how many times it has started in the last ten minutes and why it last exited. `start` and `restart` clear the restart limit, so use them after fixing the cause of a failure. `stop` stops it; the next call that needs the rig starts it again. A rig with a transmit attestation is released first (PTT off, read back) before its daemon is stopped, and the stop is refused if that cannot be confirmed: stopping the daemon of a keyed rig would leave it keyed. A rig configured with `rigctld_port` is not started or stopped by this server.

## Arguments

<!-- arguments -->

## Returns

`{"source": "managed rigctld", "running": true, "port": 41234, "pid": ..., "listens_on": "127.0.0.1", "starts_in_last_10_min": 1, "last_exit": null}`.

## Failure modes

* `rigctld exited at start`: the text carries the end of its log. The usual causes are a wrong model number, a wrong baud rate or a busy device; see [troubleshooting](instruments/hamlib/troubleshooting).
* `has been started 4 times in 10 minutes`: fix the cause, then `start`.

## Example

`bench_hamlib_rigctld {"action": "restart"}`.

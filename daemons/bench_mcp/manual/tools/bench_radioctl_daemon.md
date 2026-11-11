---
title: bench_radioctl_daemon
summary: Status, start, stop or restart of the managed radioctld of a rig. It listens on 127.0.0.1 only.
see_also: [instruments/radioctl/overview, instruments/radioctl/operation, instruments/radioctl/limits]
tools: [bench_radioctl_daemon]
instrument: radioctl
tier: read_config
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_daemon

For a rig configured with `rig`, this server runs the radioctld (with a configuration it writes into the managed directory: a virtual rig, or the serial rig with its `device` and `speed`). `status` says whether it runs, its port, how many times it has started in the last ten minutes and why it last exited. `start` and `restart` clear the restart limit, so use them after fixing the cause of a failure. `stop` stops it; the next call that needs the rig starts it again. A rig with a transmit attestation is released first (PTT off, read back) before its daemon is stopped, and the stop is refused if that cannot be confirmed: stopping the daemon of a keyed rig would leave it keyed. A rig configured with `radioctld_port` is not started or stopped by this server.

## Arguments

<!-- arguments -->

## Returns

`{"source": "managed radioctld", "running": true, "port": 41234, "pid": ..., "listens_on": "127.0.0.1", "starts_in_last_10_min": 1, "last_exit": null}`.

## Failure modes

* `radioctld exited at start`: the text carries the end of its log. The usual causes are a rig description that does not load, a wrong baud rate or a busy device; see [troubleshooting](instruments/radioctl/troubleshooting).
* `has been started 4 times in 10 minutes`: fix the cause, then `start`.

## Example

`bench_radioctl_daemon {"action": "restart"}`.

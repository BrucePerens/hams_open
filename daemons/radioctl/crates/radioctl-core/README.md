# radioctl-core

The typed vocabulary and the rig actor of radioctl. The full documentation is the manual in
`docs/manual/` and the workspace `README.md`; this file lists the anchors of the modules added
since the workspace README's list was written.

## Network links (`net`)

`net::open_link` opens a TCP or UDP link that reconnects with backoff and publishes a `LinkStatus`
watch, so the rig actor unkeys the radio on every reconnect. `Framing` is the hook set for radios
with a login or their own framing. Manual: `docs/manual/05-transports.md`.

## Anchors

- [@ANCHOR: radioctl:radioctl_core_net_config_new] (radioctl-core) TCP to `address`, three-second connect, handshake and write limits, no idle limit, the default backoff and one first attempt.
- [@ANCHOR: radioctl:radioctl_core_net_raw_reset] (radioctl-core) `reset` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_raw_greeting] (radioctl-core) `greeting` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_raw_needs_handshake] (radioctl-core) `needs_handshake` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_raw_encode] (radioctl-core) `encode` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_raw_decode] (radioctl-core) `decode` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_raw_keepalive] (radioctl-core) `keepalive` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_length_new] (radioctl-core) A framing with a length field of `width` bytes: 2 or 4 (anything else is made 4).
- [@ANCHOR: radioctl:radioctl_core_net_length_reset] (radioctl-core) `reset` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_length_greeting] (radioctl-core) `greeting` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_length_needs_handshake] (radioctl-core) `needs_handshake` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_length_encode] (radioctl-core) `encode` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_length_decode] (radioctl-core) `decode` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_length_keepalive] (radioctl-core) `keepalive` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_snapshot] (radioctl-core) `snapshot` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_link_status] (radioctl-core) The current link status.
- [@ANCHOR: radioctl:radioctl_core_net_link_watch] (radioctl-core) A receiver that wakes on every status change; pass it as `RigOptions::link_watch`.
- [@ANCHOR: radioctl:radioctl_core_net_link_stats] (radioctl-core) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctl_core_net_link_into_parts] (radioctl-core) Splits the link into the byte stream (for [`crate::rig::spawn`]) and the monitor the owner keeps.
- [@ANCHOR: radioctl:radioctl_core_net_monitor_status] (radioctl-core) The current link status.
- [@ANCHOR: radioctl:radioctl_core_net_monitor_watch] (radioctl-core) A receiver that wakes on every status change.
- [@ANCHOR: radioctl:radioctl_core_net_monitor_stats] (radioctl-core) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctl_core_net_error] (radioctl-core) `net_error` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_wire_open] (radioctl-core) Resolves the address and opens a socket, within `limit`.
- [@ANCHOR: radioctl:radioctl_core_net_wire_recv] (radioctl-core) Reads what is available: a chunk of a TCP stream, or one datagram.
- [@ANCHOR: radioctl:radioctl_core_net_wire_send] (radioctl-core) Writes all of `bytes` (TCP) or sends them as one datagram (UDP).
- [@ANCHOR: radioctl:radioctl_core_net_sleep_until] (radioctl-core) Resolves at `at`, or never when there is no deadline.
- [@ANCHOR: radioctl:radioctl_core_net_supervisor_run] (radioctl-core) The supervisor task: first connection, then sessions and reconnects until the actor's end of the stream is dropped.
- [@ANCHOR: radioctl:radioctl_core_net_sleep_dropping] (radioctl-core) Waits `delay`, dropping (and counting) bytes the actor writes meanwhile.
- [@ANCHOR: radioctl:radioctl_core_net_attempt] (radioctl-core) Opens the socket, sends the greeting and, when the framing needs it, waits for the handshake.
- [@ANCHOR: radioctl:radioctl_core_net_open_and_handshake] (radioctl-core) The cancel-safe part of an attempt: socket, greeting, handshake.
- [@ANCHOR: radioctl:radioctl_core_net_send] (radioctl-core) One bounded write to the socket.
- [@ANCHOR: radioctl:radioctl_core_net_session] (radioctl-core) One ready connection: wire to actor, actor to wire, keep-alive and silence limit.
- [@ANCHOR: radioctl:radioctl_core_net_deliver] (radioctl-core) Writes bytes to the actor's stream, bounded.
- [@ANCHOR: radioctl:radioctl_core_net_proto_name] (radioctl-core) `proto_name` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_read_size] (radioctl-core) `read_size` (see the source).
- [@ANCHOR: radioctl:radioctl_core_net_open_link] (radioctl-core) Opens a network link on `runtime` and returns once the first connection is ready, or with the last attempt's error after `config.first_attempts` ([`ErrorKind::LinkDown`]).

# radioctl-core

The typed vocabulary and the rig actor of radioctl. The full documentation is the manual in
`docs/manual/` and the workspace `README.md`; this file lists the anchors of the modules added
since the workspace README's list was written.

## Network links (`net`)

`net::open_link` opens a TCP or UDP link that reconnects with backoff and publishes a `LinkStatus`
watch, so the rig actor unkeys the radio on every reconnect. `Framing` is the hook set for radios
with a login or their own framing. Manual: `docs/manual/05-transports.md`.

## Anchors


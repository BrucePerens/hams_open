# radioctl-protocols

The sans-IO protocol engines, one per family, and the virtual rigs. The full documentation is the
manual in `docs/manual/` and the workspace `README.md`; this file lists the anchors of modules added
since the workspace README's list was written.

## Kenwood TS-890S LAN session

`kenwood_ts890_lan::Ts890LanFraming` is the TS-890S LAN login (`##CN`, `##ID`) and keep-alive as a
`radioctl_core::net::Framing`. Manual: `docs/manual/05-transports.md`, "The TS-890S over the LAN".

## Anchors


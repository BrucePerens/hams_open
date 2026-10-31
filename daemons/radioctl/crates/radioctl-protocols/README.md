# radioctl-protocols

The sans-IO protocol engines, one per family, and the virtual rigs. The full documentation is the
manual in `docs/manual/` and the workspace `README.md`; this file lists the anchors of modules added
since the workspace README's list was written.

## Kenwood TS-890S LAN session

`kenwood_ts890_lan::Ts890LanFraming` is the TS-890S LAN login (`##CN`, `##ID`) and keep-alive as a
`radioctl_core::net::Framing`. Manual: `docs/manual/05-transports.md`, "The TS-890S over the LAN".

## Anchors

- [@ANCHOR: radioctl:radioctl_ts890lan_credentials_new] (radioctl-protocols) An account (`administrator` or a user account) and its password: 1 to 32 printable ASCII characters without `;` each.
- [@ANCHOR: radioctl:radioctl_ts890lan_credentials_debug] (radioctl-protocols) The account type and name; the password is never shown.
- [@ANCHOR: radioctl:radioctl_ts890lan_new] (radioctl-protocols) A session with the account, keeping the connection alive every [`KEEPALIVE_EVERY`].
- [@ANCHOR: radioctl:radioctl_ts890lan_with_keepalive] (radioctl-protocols) Changes the keep-alive interval (for tests against a fake radio with a short idle limit).
- [@ANCHOR: radioctl:radioctl_ts890lan_handle_frame] (radioctl-protocols) Handles one complete `;`-terminated frame in the current phase.
- [@ANCHOR: radioctl:radioctl_ts890lan_reset] (radioctl-protocols) `reset` (see the source).
- [@ANCHOR: radioctl:radioctl_ts890lan_greeting] (radioctl-protocols) `greeting` (see the source).
- [@ANCHOR: radioctl:radioctl_ts890lan_needs_handshake] (radioctl-protocols) `needs_handshake` (see the source).
- [@ANCHOR: radioctl:radioctl_ts890lan_encode] (radioctl-protocols) `encode` (see the source).
- [@ANCHOR: radioctl:radioctl_ts890lan_decode] (radioctl-protocols) `decode` (see the source).
- [@ANCHOR: radioctl:radioctl_ts890lan_keepalive] (radioctl-protocols) `keepalive` (see the source).
- [@ANCHOR: radioctl:radioctl_ts890lan_keepalive_sent] (radioctl-protocols) `keepalive_sent` (see the source).

<!-- This file is part of hams_open, an open source module. -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Cloudflared FFI

This directory contains a Go-based wrapper (`main.go`) that exposes Foreign Function Interface (FFI) bindings for `cloudflared`.

### Functions
- **FFI Exports**: Provides C-callable functions such as `StartTunnel`, `StopTunnel`, `StartLocalSimulator`, and `StopLocalSimulator`.
- **Local Simulator**: Sets up a local HTTPS reverse proxy simulator that mimics Cloudflare headers (`CF-Connecting-IP`, `X-Forwarded-For`, `CF-Visitor`) for local testing of tunnel-dependent services securely.

### Building

```
cd daemons/cloudflared-ffi
go build -buildmode=c-shared -o libcloudflared.so .
```

This produces `libcloudflared.so` and `libcloudflared.h` in this directory. `cloudflare/utils/cloudflare_daemon.py` loads the compiled `.so` via `ctypes` -- rebuild it here after any change to `main.go` and copy (or symlink) the result to where that loader expects it.

### History

Removed from the tree on 2026-08-25 (commit `51b81cb3`) for lacking a license header; restored on 2026-09-17 with one added, per Bruce: "restore the source, deleting it was a mistake." See `night_shift_todo`'s `cloudflared-ffi-simulator-source-not-in-tree-*.md` (now closed) for the full recovery story.

<!-- This file is part of hams_open, an open source module. -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Cloudflared FFI

This directory contains a Go-based wrapper (`main.go`) that exposes Foreign Function Interface (FFI) bindings for `cloudflared`.

### Functions
- **FFI Exports**: Provides C-callable functions such as `StartTunnel`, `StopTunnel`, `StartLocalSimulator`, and `StopLocalSimulator`.
- **Local Simulator**: Sets up a local HTTPS reverse proxy simulator that mimics Cloudflare headers (`CF-Connecting-IP`, `X-Forwarded-For`, `CF-Visitor`) for local testing of tunnel-dependent services securely.

### Building

`libcloudflared.so` is a per-CPU build artifact and is not in git (an x86-64 copy was once
committed and could not be loaded on arm64 test hosts). It is built from this directory, offline,
with the OS package `golang-1.24-go` plus a C compiler (`build-essential`); this module is
stdlib-only Go, so no module download is involved:

```
sudo apt-get install -y golang-1.24-go build-essential
python3 hams_shared/tools/build_cloudflared_ffi.py
```

`provision.py --test` (and production provisioning) run the same step, and `hams_shared/tools/test.py`
stops with this fix when the `cloudflare` module is tested without a library built for the host's CPU.
The script pins `GOTOOLCHAIN=local`, `GOFLAGS=-mod=readonly` and `GOPROXY=off`. Keep `go.mod`'s `go`
line at 1.24 or lower so the packaged compiler can build it. It produces `libcloudflared.so` and
`libcloudflared.h` in this directory; `cloudflare/utils/cloudflare_daemon.py` loads the `.so` via
`ctypes` from there. Rsync-based test runners must exclude both files so one CPU's build is never
copied over another's.

### History

Removed from the tree on 2026-08-25 (commit `51b81cb3`) for lacking a license header; restored on 2026-09-17 with one added, per Bruce: "restore the source, deleting it was a mistake." See `night_shift_todo`'s `cloudflared-ffi-simulator-source-not-in-tree-*.md` (now closed) for the full recovery story.

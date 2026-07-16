# Cloudflared FFI

This directory contains a Go-based wrapper (`main.go`) that exposes Foreign Function Interface (FFI) bindings for `cloudflared`.

### Functions
- **FFI Exports**: Provides C-callable functions such as `StartTunnel`, `StopTunnel`, `StartLocalSimulator`, and `StopLocalSimulator`.
- **Local Simulator**: Sets up a local HTTPS reverse proxy simulator that mimics Cloudflare headers (`CF-Connecting-IP`, `X-Forwarded-For`, `CF-Visitor`) for local testing of tunnel-dependent services securely.

---
anchor: cloudflare:COMM_systemd_resolve_cloudflared_bin
code_hash: sha256:fe3d4e7a88c3dd8ee855f6819b882a4c0186199821a3b8dfade00a993d4e97d8
hash_algorithm: v2
review_tier: 1
---

# Claim: `_resolve_cloudflared_bin`

`_resolve_cloudflared_bin()` returns the vendored cloudflared if it is executable, else the one on PATH (with a warning), else the vendored path anyway (with an error) so the failed start names the real cause.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

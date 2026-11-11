---
anchor: cloudflare:COMM_systemd_unit_name
code_hash: sha256:30e9aefcc3f50f2c395450de6207c26ca4a074675306eab375542c1f6a34e29a
hash_algorithm: v2
review_tier: 1
---

# Claim: `_unit_name`

`_unit_name(tunnel_key)` returns the systemd unit name `cloudflared@<key>.service` of one tunnel.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

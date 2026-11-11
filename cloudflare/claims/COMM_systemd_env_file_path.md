---
anchor: cloudflare:COMM_systemd_env_file_path
code_hash: sha256:3bdd84b1c865b6341add92367d2316a62cec6c73ccf5b2582aa2075f35481ba4
hash_algorithm: v2
review_tier: 1
---

# Claim: `_env_file_path`

`_env_file_path(tunnel_key)` returns the path of a tunnel's run-token file, `cloudflared-<key>.env` under the keys directory; it only builds the path.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

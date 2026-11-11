---
anchor: cloudflare:COMM_systemd_remove_env_file
code_hash: sha256:a83d9d86401330654c6561ae9aa5ccc1b4dc67004b030c95064e5f7e718409d5
hash_algorithm: v2
review_tier: 1
---

# Claim: `_remove_env_file`

`_remove_env_file(tunnel_key)` deletes a stopped tunnel's run-token file (it holds a live credential); a key resolving outside the keys directory is refused and logged, a missing file is fine, and a failed removal is logged, never raised.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

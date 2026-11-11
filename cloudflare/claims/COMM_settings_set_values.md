---
anchor: cloudflare:COMM_settings_set_values
code_hash: sha256:8107b9e0df2351d0b7eb0fc7e755cb7f2dea3eff0a91cced61019b139eceff50
hash_algorithm: v2
review_tier: 1
---

# Claim: `set_values`

`set_values()` saves the settings, then republishes the merged trusted IP range list to Redis at once so the env-less WSGI hook sees an edit without waiting for the daily cron or its 60-second cache.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

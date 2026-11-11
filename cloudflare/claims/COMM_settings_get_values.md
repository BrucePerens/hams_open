---
anchor: cloudflare:COMM_settings_get_values
code_hash: sha256:c54dbad763e21f833b0635887699d749a4e5edaffa440aa7bf9900a3947efb48
hash_algorithm: v2
review_tier: 1
---

# Claim: `get_values`

`get_values()` (private, model-level) returns the settings defaults with each secret field that the current website has set replaced by the mask `********`, so a settings read never returns the secret.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

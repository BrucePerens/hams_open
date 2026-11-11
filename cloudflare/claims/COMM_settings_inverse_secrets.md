---
anchor: cloudflare:COMM_settings_inverse_secrets
code_hash: sha256:bb534e2c051a9fde9dcaace7148aeefd32e05b83c43581db95033ab9d0fca857
hash_algorithm: v2
review_tier: 1
---

# Claim: `_inverse_cloudflare_secrets`

`_inverse_cloudflare_secrets()` saves the secret settings fields to the selected website: a field still holding the mask is not edited and keeps its stored value, any other value is written, and a blank value clears the secret.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

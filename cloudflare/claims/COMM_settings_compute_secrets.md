---
anchor: cloudflare:COMM_settings_compute_secrets
code_hash: sha256:2a9935f86f8838415861ab552be0655f782118af7cf79fdfd695468b51290b49
hash_algorithm: v2
review_tier: 1
---

# Claim: `_compute_cloudflare_secrets`

`_compute_cloudflare_secrets()` fills each secret settings field (API token, Turnstile secret, ...) with the mask `********` when the selected website has a value and with False when it has none; the secret itself is never put in the field.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

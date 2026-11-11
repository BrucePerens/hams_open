---
anchor: cloudflare:COMM_tunnel_check_catch_all_service
code_hash: sha256:bf29a8e527b6a105a5622587d0b24c8443ce80eb3a32e11bf0b8100554f02ee1
hash_algorithm: v2
review_tier: 1
---

# Claim: `_check_catch_all_service`

`_check_catch_all_service()` is the constraint on a tunnel's `catch_all_service`: the whole value must be an http:// or https:// URL without spaces or `http_status:<3 digits>`, else `ValidationError`, so a malformed final ingress rule is refused when saved, never pushed.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

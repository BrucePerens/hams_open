---
anchor: cloudflare:COMM_dns_record_onchange_type
code_hash: sha256:cf6664acb7083cb71d076c93c2d981b9f2cf0b08e826edc93f631f6009b64684
hash_algorithm: v2
review_tier: 1
---

# Claim: `_onchange_type_clears_proxied`

`_onchange_type_clears_proxied()` is the form onchange on `type`: choosing a type that cannot be proxied (anything but A, AAAA and CNAME) unticks Proxied, so the administrator is not left with a row the constraint refuses to save.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

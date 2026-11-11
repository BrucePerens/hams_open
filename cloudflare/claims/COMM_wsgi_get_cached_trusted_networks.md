---
anchor: cloudflare:COMM_wsgi_get_cached_trusted_networks
code_hash: sha256:f174d078139b82d0e96b1acb5eb717d0ab3e4e11ee0022f1f58eac90240b3aba
hash_algorithm: v2
review_tier: 1
---

# Claim: `_get_cached_trusted_networks`

`_get_cached_trusted_networks()` returns the tuple of trusted networks the env-less WSGI hook uses, read from Redis at most once per cache TTL per process; when Redis fails or holds malformed data it keeps the last good list (empty at cold start, so loopback only), never widening trust.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

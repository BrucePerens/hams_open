---
anchor: cloudflare:COMM_domain_unlink
code_hash: sha256:80451eaa376ac50d8ebe98d58d2cf2a53e624ef5defa46f3561a4e01513a624d
hash_algorithm: v2
review_tier: 1
---

# Claim: `unlink`

`unlink()` on a routing domain first deletes the Cloudflare custom hostnames of the records being deleted (`_delete_cloudflare_custom_hostname_batch`; a failed Cloudflare delete is logged, not raised), then deletes the rows.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

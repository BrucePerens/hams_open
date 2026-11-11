---
anchor: cloudflare:COMM_cache_rule_push_to_cloudflare
code_hash: sha256:eaec9c3c7399ab1bd258faf3dc08efaedaaf35fd07399161ba0f7aa54f577b35
hash_algorithm: v2
review_tier: 1
---

# Claim: `CloudflareCacheRule`

`CloudflareCacheRule.action_push_to_cloudflare()` (the form's Push button, one record) runs `action_push_cache_rules` for the rule's website (else the current website) as the `user_cloudflare_waf` service user and returns a `display_notification` client action: `success` with the result message, or a sticky `danger` titled "Cloudflare push failed".

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

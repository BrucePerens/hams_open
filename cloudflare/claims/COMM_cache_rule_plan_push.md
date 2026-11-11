---
anchor: cloudflare:COMM_cache_rule_plan_push
code_hash: sha256:945677557fbd445f07a899d27119887fbed389a7eca65fdc898334990aab0c49
hash_algorithm: v2
review_tier: 1
---

# Claim: `CloudflareCacheRule`

`CloudflareCacheRule.action_plan_cache_push()` (the form's Plan button, one record) runs `plan_cache_rules` for the rule's website (else the current website) as the `user_cloudflare_waf` service user and returns a sticky `display_notification` client action: the plan text as `info`, or the refusal message as `danger` titled "Cache rule plan refused". It only reads Cloudflare and writes nothing.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

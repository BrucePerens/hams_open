---
anchor: cloudflare:COMM_cache_plan_cache_rules
code_hash: sha256:3a822bed698736561c1e62390cb18cdd63b008ddb19a3deaddcd6dedb33a971b
hash_algorithm: v2
review_tier: 1
---

# Claim: `plan_cache_rules`

`plan_cache_rules(website_id=None)` reads the zone's cache-settings ruleset (GET only) and returns a dict with `ok`, `message`, `text`, `hash`, `writes`, `rules`, `ruleset_id` and `ruleset_name`. It checks the caller is authorized to manage the WAF, and returns `ok` False with nothing planned when credentials are missing, Cloudflare could not be read (an unreadable ruleset is never treated as empty), or the merge reports a problem.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

---
anchor: cloudflare:COMM_cache_action_push_cache_rules
code_hash: sha256:be285dd68021182c940cf363c65d05fb8f9f28b4edbab177ffc42b536cc294a7
hash_algorithm: v2
review_tier: 1
---

# Claim: `action_push_cache_rules`

`action_push_cache_rules(website_id=None, plan_hash=None)` pushes the website's cache rules to the zone's `http_request_cache_settings` ruleset and returns `(ok, message)`. It plans first and refuses when the plan is refused, when `plan_hash` is given and no longer matches the fresh plan, and does nothing (`True`, "Nothing to do") when the plan has no writes; otherwise it updates or creates the ruleset and reads it back, returning False when the rules Odoo does not manage differ from the plan. Rules Odoo does not manage are carried through untouched.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

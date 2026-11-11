---
anchor: cloudflare:COMM_dns_zone_of
code_hash: sha256:21ae569549e552f2d7622bfbc89dfc8b71aef11096ba79f0ddbdc0cbbed5c74d
hash_algorithm: v2
review_tier: 1
---

# Claim: `_dns_zone_of`

`_dns_zone_of(name, website, token, cache)` finds the zone that holds `name`: the longest suffix of two or more labels that the token can see, asking Cloudflare once per (website, suffix) through `cache`. It returns `(zone_id, None)`, or `(None, problem)` when Cloudflare could not be asked or no visible zone matches.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

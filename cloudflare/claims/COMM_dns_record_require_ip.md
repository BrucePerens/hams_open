---
anchor: cloudflare:COMM_dns_record_require_ip
code_hash: sha256:773b82fa0567ee7e10151fa7be0ee25e3eae4cd223a50e390a511493a6437456
hash_algorithm: v2
review_tier: 1
---

# Claim: `_require_ip`

`_require_ip(rec, factory)` (static) calls `factory` (`ipaddress.IPv4Address` or `IPv6Address`) on the row's content and raises `ValidationError` when it is not an address of that kind; used by the content constraint for A and AAAA rows.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

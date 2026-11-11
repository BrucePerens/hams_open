---
anchor: cloudflare:COMM_dns_api_json_result_kind
code_hash: sha256:d5421e549bb5a012f1093546ef6d9b1f89835247a6f6ae0e20a0448d8a0a6416
hash_algorithm: v2
review_tier: 1
---

# Claim: `_json_result`

`_json_result(resp, kind=dict)` returns the `result` of a Cloudflare JSON answer when it is a `kind` (an object by default, a list or string for calls that return those) and None when the body is not JSON, is not an object, or its result is another type; callers treat None as an unreadable answer.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

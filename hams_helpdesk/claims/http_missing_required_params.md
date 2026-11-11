---
anchor: hams_helpdesk:http_missing_required_params
code_hash: sha256:b2cac8b8ee325aaed69785062be7b7e6c8c4b02c101b497b4d8d300dfb1438f0
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ir_http_exception._missing_required_params`

THE function SHALL return `[]` when the route type is not http, json or jsonrpc, when the endpoint has no `original_endpoint`, or when its signature cannot be read. Otherwise it returns, in signature order, the names of the handler's parameters after the first that have no default, are positional-or-keyword or keyword-only, and are not keys of `request.params`. HOLDS. Read 2026-11-11 against the code.

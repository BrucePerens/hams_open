---
anchor: cloudflare:COMM_trusted_ip_parse_ranges
code_hash: sha256:772a0bde98df9a954a8cda6dca990a09a2d77e6b64ed6c3f58a9fce17313f4bd
hash_algorithm: v2
review_tier: 1
---

# Claim: `_parse_ranges`

`_parse_ranges(text)` splits `text` on commas and newlines and returns the tokens that parse as an IPv4 or IPv6 network, each in canonical form (`198.51.100.77/24` becomes `198.51.100.0/24`, a bare address gets `/32` or `/128`). A malformed token is logged and skipped, never aborting the list; None or empty text gives `[]`.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

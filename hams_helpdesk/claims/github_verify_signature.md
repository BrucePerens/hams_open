---
anchor: hams_helpdesk:github_verify_signature
code_hash: sha256:f6e3c1706a55b6b3964049ca5d96336f2b8f23592868be4d2318c8e17550cd63
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `github_webhook_api._verify_github_signature`

THE function SHALL return False when `secret` or `signature_header` is falsy or the header does not start with `sha256=`. Otherwise it SHALL return True exactly when the hex after the prefix equals the HMAC-SHA256 of `raw_body` keyed by `secret.encode()` (compared with `hmac.compare_digest` on bytes, so a non-ASCII header is False, not an error). A `TypeError` or `ValueError` while computing the HMAC (for example `raw_body` that is not bytes) gives False and a warning log. It never raises for these inputs. HOLDS. Read 2026-11-11 against the code.

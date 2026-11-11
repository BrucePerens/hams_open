---
anchor: cloudflare:COMM_dns_credential_website
code_hash: sha256:ca6078032d13f4593449fb432646241f090fa623db9be06e520060d04d77f7ba
hash_algorithm: v2
review_tier: 1
---

# Claim: `_dns_credential_website`

`_dns_credential_website(row, websites)` picks the website whose token pushes one DNS row: the row's own website, else the first of `websites` that has a usable token. It returns `(website, token, problem)`; the token is None and `problem` is a sentence when the row's own website has no usable token or no website has one.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

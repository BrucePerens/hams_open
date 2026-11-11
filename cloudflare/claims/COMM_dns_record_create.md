---
anchor: cloudflare:COMM_dns_record_create
code_hash: sha256:c82c641f08a3ff95345af2bda09a0d38be7080a2b0ce5df692138ac2a12cc781
hash_algorithm: v2
review_tier: 1
---

# Claim: `create`

`create(vals_list)` creates DNS rows; a row whose type can never be proxied (NS, TXT, ...) and whose values do not say `proxied` is created DNS-only, because the field's default is on and the new row would fail its own constraint. An explicit `proxied=True` for such a type is still refused by the constraint.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

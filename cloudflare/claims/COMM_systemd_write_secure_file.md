---
anchor: cloudflare:COMM_systemd_write_secure_file
code_hash: sha256:a641d09c11db48b4c7e5e6148e79e732d5ef278c3c5f2d2d09717c23272bfd46
hash_algorithm: v2
review_tier: 1
---

# Claim: `_write_secure_file`

`_write_secure_file(path, content, mandatory_prefix)` writes `content` atomically at mode 0600 (a new temp file in the same directory, then rename); it raises `ValueError`, writing nothing, when the resolved path is not strictly inside `mandatory_prefix` as a directory.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

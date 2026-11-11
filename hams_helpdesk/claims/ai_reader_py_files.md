---
anchor: hams_helpdesk:ai_reader_py_files
code_hash: sha256:c256f4d96818151086103f445d4e93412a4575cb3d9c69567282e4cb84668023
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ai_reader_rule._py_files`

THE generator SHALL yield each path from `_candidate_files(root)` whose file name ends in `.py` and does not start with `test_`. IF `daemon_only` is true, it SHALL yield only those whose path relative to `root` has a directory component named `daemon` or `daemons`. It reads no file. HOLDS. Read 2026-11-11 against the code.

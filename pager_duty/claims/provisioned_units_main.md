---
anchor: pager_duty:provisioned_units_main
code_hash: sha256:947e662638338d799fa7aae696104c3f0da24414566f5f2d41bcc626b14fc6fa
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: check_provisioned_units.py `main` (`pager_duty/daemon/check_provisioned_units.py`)

Read 2026-11-11, whole function. Tier 2: Exit code and text for the pager monitor; reads the MANIFEST and asks systemd, changes nothing.

1. THE function SHALL ignore its `argv` and read `HAMS_UNITS_EXPECTED_OFF` as a comma-separated list of unit names, stripped, blanks dropped, passed to `units_not_in_operation` as the units that may be off.
2. WHEN `expected_units` raises `OSError` or `KeyError` THE function SHALL print `SEVERITY:high` and a line saying the MANIFEST did not load to stderr and return 1.
3. WHEN no unit is reported THE function SHALL print `all N provisioned units are in operation` to stdout and return 0.
4. WHEN units are reported THE function SHALL print `SEVERITY:high` and a first line giving the count and the first four unit names (then ` +N` for the rest) to stderr, then one line of every `unit (reason)` joined by `; `, and return 1. HOLDS.

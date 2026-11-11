---
anchor: pager_duty:provisioned_units_systemctl_show
code_hash: sha256:1a3ede640a9ab7981a287e9c684b80be69dc71c4a0184007ca749a96378237db
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: check_provisioned_units.py `systemctl_show` (`pager_duty/daemon/check_provisioned_units.py`)

Read 2026-11-11, whole function. Tier 2: Runs a read-only systemctl query.

1. THE function SHALL run `systemctl show <unit> -p <SHOW_PROPERTIES>` capturing text output, without raising on a non-zero exit.
2. THE function SHALL return a dict with one entry per output line that contains `=`, split at the first `=` only, so a value containing `=` is kept whole; lines without `=` are ignored.
3. WHEN systemctl prints nothing usable (an unknown unit, a failed call) THE function SHALL return an empty dict; it never inspects the exit status. A missing `systemctl` binary raises `OSError` to the caller, and there is no timeout. HOLDS.

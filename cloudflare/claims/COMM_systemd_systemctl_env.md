---
anchor: cloudflare:COMM_systemd_systemctl_env
code_hash: sha256:8b874c8e42a1736349b875e1e3074caacf79bd6af285c315d0597d7c7e000942
hash_algorithm: v2
review_tier: 1
---

# Claim: `_systemctl_env`

`_systemctl_env()` returns a copy of the process environment with `XDG_RUNTIME_DIR` set to `/run/user/<uid>`, which a system-unit Odoo needs for `systemctl --user` to reach the user manager.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

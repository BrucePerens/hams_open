---
anchor: cloudflare:COMM_systemd_run_systemctl
code_hash: sha256:3df3314ed9e7297115c6ffea9e208f27ec75c165f820962f3192dee5232123d3
hash_algorithm: v2
review_tier: 1
---

# Claim: `_run_systemctl`

`_run_systemctl(*args)` runs `systemctl --user <args>` without a shell, with a timeout and the `_systemctl_env` environment, and returns `(True, stdout)` or `(False, stderr or stdout)`; a missing systemctl or a timeout is logged and returned as `(False, message)`.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

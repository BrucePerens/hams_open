---
anchor: cloudflare:COMM_tunnel_wizard_compute_command
code_hash: sha256:65ee4b4e53cbd6a3d8d695c9dcb3ee2fe052c4cebf4c1ae2496ee719649cb085
hash_algorithm: v2
review_tier: 1
---

# Claim: `_compute_command`

`_compute_command()` sets the wizard's `command` field to the `install_command` of the action context, or empty when there is none; it is computed, never stored, so the one-time install token is not written to the database.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.

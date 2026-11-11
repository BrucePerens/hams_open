---
anchor: hams_helpdesk:triage_spool_dir
code_hash: sha256:8dc2441fada382ad492e7e5aa9e8e02b7023bdf3c464834f2ddabe884cea19b1
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `triage_wakeup.spool_dir`

THE function SHALL return the value of the environment variable `HAMS_TRIAGE_SPOOL_DIR` when it is set and non-empty, else `/opt/hams/spool/ticket_triage`. It evaluates the environment at each call and touches no file. HOLDS. Read 2026-11-11 against the code.

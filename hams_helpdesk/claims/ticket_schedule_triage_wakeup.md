---
anchor: hams_helpdesk:ticket_schedule_triage_wakeup
code_hash: sha256:977b4294a944106f5bf80d66def2df3ec1a2ff86d06f316d709bf6260d8c6017
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HelpdeskTicket._schedule_triage_wakeup`

THE method SHALL do nothing for an empty or falsy `ticket_ids`. Otherwise it adds one callback to `cr.postcommit` that calls `write_triage_wakeups` with a tuple of the ids, and so writes nothing before the commit. An exception while adding the callback is logged and swallowed; the method never raises. HOLDS. Read 2026-11-11 against the code.

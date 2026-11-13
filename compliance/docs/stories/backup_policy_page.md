# Story: Backup and Data Resilience Policy Page

## User Persona
A prospective or existing member who wants to verify, for themselves, that hams.com is not exposed
to the kind of incident that struck the ARRL (American Radio Relay League) in May 2024 (a ransomware attack that encrypted its
systems, ending in a reported $1 million ransom payment).

## Scenario
1. A visitor navigates to the published "Backup and Data Resilience Policy" page, which is not
   published because any law requires it, but so every member can check hams.com's real backup and
   recovery posture for themselves -- a concrete, technical page, not a marketing claim
   *(Reference: [@ANCHOR: compliance:backup_policy_page])*. The page is the QWeb template `backup_policy_template` in `data/backup_policy_data.xml`, published at `/backup-policy` through a `website.page` record, with a `compliance.document` row so `/compliance` lists it. Its sections are immutable off-site undeletable backups, rebuilding from source and not only restoring a database, restore verification as a routine part of the system, and a status section. It is public static text, inside a `noupdate` block, with no controller or arguments. Test: `test_backup_policy_page_reachable_and_registered`.
2. The page is injected via `post_init_hook`, matching this module's own established convention for
   every other trust/legal page, so it exists on a fresh install with no separate manual step.

**Status:** Verified by `test_backup_policy_page_reachable_and_registered` (tests/test_pages.py).

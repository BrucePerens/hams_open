# Story: Backup and Data Resilience Policy Page

## User Persona
A prospective or existing member who wants to verify, for themselves, that hams.com is not exposed
to the kind of incident that struck the ARRL in May 2024 (a ransomware attack that encrypted its
systems, ending in a reported $1 million ransom payment).

## Scenario
1. A visitor navigates to the published "Backup and Data Resilience Policy" page, which is not
   published because any law requires it, but so every member can check hams.com's real backup and
   recovery posture for themselves -- a concrete, technical page, not a marketing claim
   *(Reference: [@ANCHOR: compliance:backup_policy_page])*.
2. The page is injected via `post_init_hook`, matching this module's own established convention for
   every other trust/legal page, so it exists on a fresh install with no separate manual step.

**Status:** Verified by `test_backup_policy_page_reachable_and_registered` (tests/test_pages.py).

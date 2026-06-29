---
name: "todo-list"
description: "Project management to-do list and priorities for ongoing work. Links to detailed proposals."
---

# Project To-Do List

This skill tracks ongoing to-dos, priorities, and coupling to proposal documents.
When adding a new to-do, use `write_to_file` (with Overwrite=True) to rewrite this SKILL.md with the updated list. Check off completed items.

## Current To-Dos

- [x] Moderate and suspend user websites. (Priority: High)
  - Details: Make it possible for group websites to have violation reports, moderation, and suspension, and anything else we do to manage user websites.
  - Wait Condition: DO NOT start this until all test failures are resolved.
  - Linked Proposal: docs/proposals/website_moderation.md (Example)

- [ ] Implement full remote operation for ham_shack and ham_relay_bridge. (Priority: Medium)
  - Details: Provide full remote operation of the user's radio and facilities based on the approved implementation plan.
  - Linked Proposal: docs/proposals/HAM_SHACK_REMOTE_OPERATION.md

- [x] Refactor Moderation Dashboard into `ham_moderation` module. (Priority: Medium)
  - Details: Move `moderation_dashboard.py` out of `ham_base` and into a dedicated `ham_moderation` module that cleanly depends on `ham_base`, `user_websites`, `ham_events`, and `ham_onboarding`. This will eliminate all soft-dependency hacks in the codebase by explicitly declaring module dependencies.
  - Linked Proposal: [docs/proposals/soft_dependency_refactoring.md](../../../../../hams_com/docs/proposals/soft_dependency_refactoring.md)

## Instructions for the AI

1. Whenever the user requests adding something to the to-do list, append it here by rewriting this SKILL.md file.
2. Mark items as `[x]` when completed.
3. Keep track of priorities and dependencies.

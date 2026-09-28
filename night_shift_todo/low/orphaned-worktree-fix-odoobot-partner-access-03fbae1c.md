---
status: open
claimed_by:
repo: hams_open
category: cleanup
created_at: 2026-09-28
---

# Orphaned worktree with real content: `/home/bruce/workspace/hams_open/.claude/worktrees/fix-odoobot-partner-access`

## What

`sweep_orphan_worktrees.py` (ADR-0102) found this worktree idle for over 6
hours with uncommitted changes and/or commits not yet pushed to its upstream (branch
`fix-odoobot-post-init-hook`). Never force-deleted -- flagged here instead so a person or a session can look at
what it holds before anything is lost.

## Done when

Someone has looked at `/home/bruce/workspace/hams_open/.claude/worktrees/fix-odoobot-partner-access`, decided whether its content is still wanted, and
either pushed/committed what should be kept then removed the worktree (`git worktree remove
/home/bruce/workspace/hams_open/.claude/worktrees/fix-odoobot-partner-access`), or removed it outright if it was genuinely abandoned.

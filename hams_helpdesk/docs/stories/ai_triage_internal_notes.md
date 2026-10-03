# Story: External AI Triage Can Post an Internal Note [@ANCHOR: hams_helpdesk:COMM_story_ai_triage_internal_notes]

As the **external ticket-triage MCP server** (`daemons/hams_ticket_triage_mcp`, hams_com),
I want to post an admin-only internal note onto a ticket I'm scoped to read,
so that my analysis is visible to staff without ever being customer-visible, and without needing
write access to the ticket model at all.

## The Process

1. The MCP server's `post_internal_note` tool calls this method, which posts the note to the
   ticket's own chatter as an internal (`mail.mt_note`, never customer-visible) note. `mail.thread`'s
   default `_mail_post_access = 'write'` would otherwise require write access to the model, so this
   method elevates internally to `hams_helpdesk.user_helpdesk_service`, the same account
   `_automated_routing_and_notification()` and `action_portal_close()` already elevate to for their
   own `message_post()` calls `[@ANCHOR: hams_helpdesk:COMM_mcp_post_internal_note]`.
2. Which tickets the caller may touch at all is decided exactly once, before any elevation: a
   `self.read(["ticket_type"])` runs first, under the CALLING identity's own env, forcing the same
   `ir.rule` (`rule_helpdesk_ticket_ai_triage_external_allowlist`) and `ir.model.access` check that
   already govern which tickets `group_ai_triage_external_service` can list/read. If the ticket's
   `ticket_type` isn't on that rule's allow-list, this raises `AccessError` and nothing is posted --
   "which tickets external AI may touch" is defined in exactly one place (the `ir.rule` domain), not
   duplicated here `[@ANCHOR: hams_helpdesk:mcp_post_internal_note]`.

**Status:** Verified by
`test_05_mcp_post_internal_note_posts_as_internal_note_on_allowlisted_ticket` and
`test_06_mcp_post_internal_note_enforces_allowlist_before_elevating`
(tests/test_helpdesk_ai_triage_external.py).

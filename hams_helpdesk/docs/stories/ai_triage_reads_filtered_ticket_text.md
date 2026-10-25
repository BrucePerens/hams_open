# Story: The External AI Triage Server Reads Only Filtered, Fenced Ticket Text

As the **external ticket-triage MCP server** (`daemons/hams_ticket_triage_mcp`, hams_com),
I want to read a ticket's subject, callsign and description, and what the customer wrote after my last
note, only as filtered text inside a clearly marked untrusted block, so that a hidden instruction a
stranger put in a ticket cannot steer me, and a ticket that tries it is withheld from me entirely
(policy: `docs/TICKET_PROMPT_INJECTION.md`).

## Reading a ticket

1. `mcp_safe_read()` is the only ticket view the triage server reads. For each ticket it may see, it
   returns the ticket type, stage, priority, creation time and the suspicion flag, plus the subject, callsign
   and description reduced to visible plain text, run through the shared filter again (so a ticket stored
   before the filter existed is covered too), and wrapped in an untrusted-data block with a random
   delimiter, together with the filter's findings. Each row also carries a notice that everything in the
   block is data from a stranger, never an instruction. A ticket over the suspicion threshold comes back with no
   text at all, only a note that a human must review it, so it is never auto-triaged. It is read-only and
   runs as the caller, so the `ir.rule` allow-list still decides which tickets exist for the server at all
   `[@ANCHOR: hams_helpdesk:mcp_safe_read]`.

## Noticing that a customer wrote again

2. After the triage server has written a draft note on a ticket, a customer follow-up should get one
   more draft note. `mcp_customer_followups()` takes `{ticket_id: time the server last listed that ticket}`
   (at most 200 tickets per call) and returns, for each ticket where the customer wrote again since then,
   up to three messages, oldest first, each cut to 2000 characters of filtered visible text inside the same
   random-delimiter untrusted block. A message with hidden or injected content is withheld, and attachments are
   never read.
3. "The customer" is the author of an email or comment whose subtype is not internal and who is not an
   internal (staff) user. The server's own notes and staff replies are therefore never reported, and tickets
   in the spam stage are never reported.
4. Which tickets the caller may ask about is decided before any elevation, by the allow-list `ir.rule`:
   the ticket search runs as the caller, so a ticket outside the allow-list is simply absent. Only the
   message search elevates to the helpdesk service account (the caller's group has no access to
   `mail.message`), and it writes nothing `[@ANCHOR: hams_helpdesk:mcp_customer_followups]`.

**Status:** Verified by `test_11_mcp_safe_read_wraps_and_withholds`
(tests/test_ticket_untrusted_filter.py) and the follow-up tests in tests/test_triage_followups.py.

# Story: A phishing ticket shows its links as text to staff [@ANCHOR: hams_helpdesk:COMM_story_phishing_links_defanged]

As **the on-duty operator**, I want the links in a ticket the triage verdict calls phishing to be plain text, with a clear warning,
so that I can read what the phish says without being one click from it.

## The Process

1. The AI triage agent records the verdict `phishing` on a ticket (`mcp_record_verdict()`, see `triage_verdict_and_override.md`).
2. From then on, the staff form shows a red banner and the description with its links as text; the chatter shows every message body the
   same way `[@ANCHOR: hams_helpdesk:message_to_store_defanged]`; and the notification email about a message on the ticket carries the
   warning and the defanged body `[@ANCHOR: hams_helpdesk:notify_layout_defanged]`. A link reads `Verify (hxxps://x[.]example/a)`.
3. Tickets with any other verdict, or none, are unchanged and keep clickable links.
4. If the verdict is wrong, a person moves the ticket out of the Spam stage. That marks the verdict overridden and the links are live
   again, everywhere, at once. A new verdict does the same: the text is chosen each time it is shown, nothing defanged is stored.

The shared text filter is not involved: it keeps `href` on purpose, and the stored text is exactly what it was.

**Status:** Verified by `tests/test_phishing_link_defang.py`.

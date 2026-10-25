# Story: A Suspicion Record Is Named by Where It Came From, Never by What Was Removed

As **support staff** looking at the suspicion log of a ticket, I want each record's title to say only which part of the
ticket the text came from and which ticket it belongs to, so that the title of a record, which shows up in lists,
breadcrumbs and the chatter, can never carry the hostile text that was removed.

The name of a suspicion record is computed from its source and its ticket and from nothing else: "<source> (ticket <id>)",
with "text" standing in when no source was recorded. It never includes the removed text, the original text or any finding
excerpt; those stay in the staff-only fields of the record. [@ANCHOR: hams_helpdesk:suspicion_name]

Tests: `tests/test_ticket_untrusted_filter.py`, the test that a record's name is the source and ticket and never the text.

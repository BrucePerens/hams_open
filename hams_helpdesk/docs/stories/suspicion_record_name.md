# Story: A Suspicion Record Is Named by Where It Came From, Never by What Was Removed

As **support staff** looking at the suspicion log of a ticket, I want each record's title to say only which part of the
ticket the text came from and which ticket it belongs to, so that the title of a record, which shows up in lists,
breadcrumbs and the chatter, can never carry the hostile text that was removed.

[@ANCHOR: hams_helpdesk:suspicion_name] The name of a suspicion record is computed from its source and its ticket and from nothing else: "<source> (ticket <id>)",
with "text" standing in when no source was recorded. It never includes the removed text, the original text or any finding
excerpt; those stay in the staff-only fields of the record.

**Interface:** `_compute_name(self)` (`models/untrusted_mixin.py`) is the compute method of the `name` Char field of `hams_helpdesk.ticket.suspicion`. It is declared `@api.depends("ticket_id", "source")`, takes no arguments besides the recordset and returns nothing; for each record it sets `name = "<source or 'text'> (ticket <ticket id>)"`. The field is not stored. The only inputs are the record's `source` and the id of its ticket, so the title can never carry hostile text; the removed text, the original (`raw_original`) and the escaped excerpts in `findings_json` live in other staff-only fields. No errors, no locking, no side effects. Tests: `tests/test_ticket_untrusted_filter.py`, `test_01a_suspicion_record_name_is_the_source_and_ticket_never_the_text`.

# Story: Staff see the problem text first, in the form and in the email [@ANCHOR: hams_helpdesk:COMM_story_problem_text_first]

As **the on-duty operator**, I want the reporter's own words at the top of the ticket and at the top of the new-ticket email,
so that I do not have to dig for what is wrong.

## The Process

1. A ticket is created (portal, backend or inbound email). Its description holds the reporter's text, already through the shared text
   filter and sanitized as HTML.
2. The assignee's email starts with that text, then the one-line "assigned to you" notice
   `[@ANCHOR: hams_helpdesk:assignment_notice_body]`. A ticket with no description gets the notice alone. A phishing ticket's links are
   still turned into text by the email layout (`phishing_links_defanged.md`).
3. The staff form shows the title, then the problem text in large type, then the secondary fields and the tabs
   `[@ANCHOR: hams_helpdesk:view_problem_text]`. Nothing is hidden or shortened; the Description tab is gone because its content moved up.

**Status:** Verified by `tests/test_helpdesk_core.py` (`test_04e`, `test_04f`, `test_04g`).

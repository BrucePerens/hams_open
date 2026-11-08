# Story: Every ticket carries a triage verdict, and a person's correction is recorded [@ANCHOR: hams_helpdesk:COMM_story_triage_verdict]

As **the on-duty operator**, I want each ticket to say whether it was judged spam, phishing, a real request or unsure, by
whom and why, so that I can trust the Spam stage, correct it, and have my correction teach the filter.

## The Process

1. Two judges write a verdict: the inbound mail rules at intake (`message_new()`, source `rules`, the reasons are the
   rule hits) and the AI triage agent after it (`mcp_record_verdict()`, source `agent`, the reasons are the agent's words,
   filtered like every note it writes) `[@ANCHOR: hams_helpdesk:mcp_record_verdict]`. Both go through one writer that runs as
   the helpdesk service account, validates the verdict and cuts the reasons to 2000 characters
   `[@ANCHOR: hams_helpdesk:triage_record_verdict]`. No caller can set the fields through `create()` or `write()`.
2. Which verdicts move a ticket to the Spam stage is a setting, `hams_helpdesk.triage_quarantine_verdicts` (comma separated,
   default `spam,phishing`) `[@ANCHOR: hams_helpdesk:triage_quarantine_verdicts]`. A verdict outside the set is recorded and
   the ticket stays where it is. Nothing is ever deleted and the sender is never mailed.
3. A person who moves a ticket out of the Spam stage after a spam or phishing verdict marks it overridden, with the time
   `[@ANCHOR: hams_helpdesk:triage_record_override]`. The verdict, its source and the flag are the labelled example (a false
   positive of the rules or of the agent); no ticket text is copied. The agent never judges that ticket again
   (`mcp_record_verdict()` and `mcp_mark_spam()` refuse it), so the person's decision wins.

**Status:** Verified by `tests/test_triage_verdict.py`.

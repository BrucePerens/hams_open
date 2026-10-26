# Story: Helpdesk Ticket Creation

**[@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_creation]**

Ticket creation is an intelligent process that automatically routes incidents to the appropriate on-duty personnel.

1.  **Incoming Trigger**: A ticket is created via UI, API, or automated integration -- including inbound support email, which the mail-ingest daemon hands off to `ingest_inbound_email()` over the JSON-2 API as a base64-encoded raw MIME message, restricted to the dedicated `mail_ingest_service_internal` account **[@ANCHOR: COMM_helpdesk_mail_ingest]**. An inbound email that `message_process()` resolves to an existing customer partner is linked to that customer's `partner_id` on the new ticket via `message_new()` **[@ANCHOR: COMM_helpdesk_message_new]**, the same way a portal- or backend-created ticket already is -- without it, an emailed-in ticket would never appear under that customer's own `/my/tickets`. A third trigger is GitHub itself: `/webhook/github` **[@ANCHOR: COMM_github_webhook_receive]** receives GitHub's own signed `dependabot_alert` and `workflow_run` webhook POSTs (HMAC-verified via `X-Hub-Signature-256`, matching `ses_webhook`'s own signature-gate-before-dispatch shape), and hands off to `_handle_dependabot_alert()` **[@ANCHOR: COMM_github_webhook_handle_dependabot_alert]** or `_handle_workflow_run()` **[@ANCHOR: COMM_github_webhook_handle_workflow_run]** depending on the event type -- a new, unactioned security alert or a CI failure becomes a ticket the same way any other trigger's ticket does, deduplicated by the alert's/run's own `html_url` via `_github_webhook_ticket_for()` **[@ANCHOR: COMM_github_webhook_ticket_for]** so a GitHub retry never opens a second ticket for the same event.
2.  **On-Duty Lookup**: The system queries the `pager_duty` adaptors to find the current "On-Duty" admin. ("On-Duty" is defined in the Architecture section of [the module README](../../README.md): without the `pager_duty` module, or when no shift is in force, nobody is on duty and the ticket stays unassigned.)
3.  **Automatic Assignment**: The `user_id` is set to the on-duty admin if unassigned.
4.  **Notifications**: Toast notifications and emails are sent to the assignee.
5.  **Pre-Shift Awareness**: Upcoming shift operators are CC'd on tickets created shortly before their shift. (The module README, under "Ticket Creation", notes that this step currently does nothing, because nothing in this repository supplies upcoming shifts.)
6.  **AI Triage Wake-Up**: after the transaction commits, a ticket created in the `new` stage drops a tiny spool file (`ticket-<id>.json`, the integer id and nothing else) for the AI triage daemon **[@ANCHOR: hams_helpdesk:COMM_triage_wakeup_on_create]**, written by `write_triage_wakeups()` **[@ANCHOR: hams_helpdesk:triage_wakeup_write]**. It is fire-and-forget: a failure is logged and never reaches the create. A ticket in the spam quarantine stage writes nothing. The daemon's own debounce, daily caps and kill switch decide what happens next (`daemons/ticket_triage_agent/README.md` in hams_com).

7.  **The Submitter's Callsign**: a ticket takes its callsign from its partner, so the partner's own callsign has to be right. The partner's callsign follows the callsign of the partner's login user and is recalculated whenever that changes, which is what lets a member's real, on-file callsign reach every ticket they submit through `/my/tickets/new`. A partner whose user has no callsign, or who has no login at all (a phone-in contact a support agent entered), keeps the value typed in by hand rather than having it blanked. The user is read through an internal facility service account, because this calculation can be triggered by a write to any partner by any caller, including narrowly scoped service accounts that cannot read users themselves **[@ANCHOR: hams_helpdesk:res_partner_compute_callsign]**.

*Verified by [@ANCHOR: hams_helpdesk:COMM_test_01_ticket_creation_and_routing]*

## Detailed Feature Index
The ticket creation process utilizes the standard ticket form.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_form]**

The operator view lists all created tickets.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_list]**

The portal user can list their tickets.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_list]**

The portal provides detailed views for individual tickets.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_detail]**

Customers can submit a new ticket directly from the portal.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_new]**

## Amazon SES scan verdicts count as spam signals

Mail to the support addresses passes through Amazon SES, whose receipt rule has scanning on, so each stored message carries
`X-SES-Spam-Verdict` and `X-SES-Virus-Verdict`. `message_parse` keeps them and `message_new` treats a `FAIL` of either like
any other spam signal: the ticket starts in the Spam / Phishing stage with the reason in its first note, the same reversible
quarantine as the filter's own signals. `PASS`, `GRAY` and `PROCESSING_FAILED` are ignored, and the DKIM, SPF and DMARC
verdicts are not used because mailing lists and forwarders legitimately fail them. [@ANCHOR: hams_helpdesk:ses_verdicts]

**Status:** Verified by `test_17_ses_verdicts_flag_only_a_fail`, `test_18_a_message_without_ses_headers_has_no_verdicts` and
`test_19_a_hostile_verdict_value_is_bounded` (tests/test_inbound_spam_filter.py).

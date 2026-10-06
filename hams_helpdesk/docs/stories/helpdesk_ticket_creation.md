# Story: Helpdesk Ticket Creation

**[@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_creation]**

Ticket creation is an intelligent process that automatically routes incidents to the appropriate on-duty personnel.

1.  **Incoming Trigger**: A ticket is created via UI, API, or automated integration -- including inbound support email, which the mail-ingest daemon hands off to `ingest_inbound_email()` over the JSON-2 API as a base64-encoded raw MIME message, restricted to the dedicated `mail_ingest_service_internal` account **[@ANCHOR: COMM_helpdesk_mail_ingest]**. An inbound email that `message_process()` resolves to an existing customer partner is linked to that customer's `partner_id` on the new ticket via `message_new()` **[@ANCHOR: COMM_helpdesk_message_new]**, the same way a portal- or backend-created ticket already is -- without it, an emailed-in ticket would never appear under that customer's own `/my/tickets`. A third trigger is GitHub itself: `/webhook/github` **[@ANCHOR: COMM_github_webhook_receive]** receives GitHub's own signed `dependabot_alert` and `workflow_run` webhook POSTs (HMAC-verified via `X-Hub-Signature-256`, matching `ses_webhook`'s own signature-gate-before-dispatch shape), and hands off to `_handle_dependabot_alert()` **[@ANCHOR: COMM_github_webhook_handle_dependabot_alert]** or `_handle_workflow_run()` **[@ANCHOR: COMM_github_webhook_handle_workflow_run]** depending on the event type -- a new, unactioned security alert or a CI failure becomes a ticket the same way any other trigger's ticket does, deduplicated by the alert's/run's own `html_url` via `_github_webhook_ticket_for()` **[@ANCHOR: COMM_github_webhook_ticket_for]** so a GitHub retry never opens a second ticket for the same event.
2.  **On-Duty Lookup**: The system queries the `pager_duty` adaptors to find the current "On-Duty" admin. ("On-Duty" is defined in the Architecture section of [the module README](../../README.md): without the `pager_duty` module, or when no shift is in force, nobody is on duty and the ticket stays unassigned.)
3.  **Automatic Assignment**: The `user_id` is set to the on-duty admin if unassigned.
4.  **Notifications**: Toast notifications and emails are sent to the assignee.
5.  **Pre-Shift Awareness**: Upcoming shift operators are CC'd on tickets created shortly before their shift. (The module README, under "Ticket Creation", notes that this step currently does nothing, because nothing in this repository supplies upcoming shifts.)
6.  **AI Triage Wake-Up**: after the transaction commits, a ticket created in the `new` stage drops a tiny spool file (`ticket-<id>.json`, the integer id and nothing else) for the AI triage daemon **[@ANCHOR: hams_helpdesk:COMM_triage_wakeup_on_create]**, written by `write_triage_wakeups()` **[@ANCHOR: hams_helpdesk:triage_wakeup_write]**. It is fire-and-forget: a failure is logged and never reaches the create. A ticket in the spam quarantine stage writes nothing. The daemon's own debounce, daily caps and kill switch decide what happens next (`daemons/ticket_triage_agent/README.md` in hams_com).

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

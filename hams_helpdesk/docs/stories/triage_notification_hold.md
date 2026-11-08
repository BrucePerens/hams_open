# Story: A spam ticket mails nobody, and a new ticket's notice waits for the triage review [@ANCHOR: hams_helpdesk:COMM_story_triage_notification_hold]

As **the on-duty operator**, I want no email, toast or assignment notice about a ticket that is spam, and I want a new
ticket's notice held for a short review window so the AI triage agent can stop a spam ticket before it reaches me, without
a real request ever being lost because the agent was slow or down.

## The Process

1. Each ticket has a notification state: `sent` (the ordinary case), `held` (waiting for the review) or `suppressed` (a
   ticket in the Spam stage) `[@ANCHOR: hams_helpdesk:COMM_triage_notification_state_on_create]`. A ticket created in the
   Spam stage (the inbound rules, a form, the API) is `suppressed`. With a hold window configured, a new ticket of a type
   the agent can judge (`general`, `hams_local_relay`) is `held`; every other type, and every ticket when no window is
   configured, notifies exactly as before. No caller can set the state.
2. The window is the system parameter `hams_helpdesk.triage_notify_hold_seconds` `[@ANCHOR: hams_helpdesk:triage_hold_seconds]`.
   It is 0 until Bruce picks a value, so nothing changes today; it is cut to 300. Which window is right is his decision (the
   to-do suggests 10 to 30 seconds). The triage daemon's debounce (`HAMS_TRIAGE_DEBOUNCE_QUIET_SECONDS`, 30 s by default)
   must be shorter than the window for the agent to judge a ticket inside it.
3. A held or suppressed ticket has no followers, so nothing posted on it is mailed, and the assignment notice, toast and
   shift CC are skipped.
4. The notice is sent (`[@ANCHOR: hams_helpdesk:triage_release_notification]`: the assignee follows, then the ordinary routing
   and notification run) when the agent gives any verdict that is not a quarantine verdict
   `[@ANCHOR: hams_helpdesk:mcp_record_verdict]`, or when the window ends: the helpdesk cron runs every minute and releases
   every held ticket past its time whatever the agent did, so a down or slow agent costs a delay and never a lost request
   `[@ANCHOR: hams_helpdesk:cron_release_held_notifications]`. The cron works in whole minutes, so the real wait is the window
   plus up to a minute.
5. A quarantine verdict (spam or phishing by default) suppresses the ticket and removes its followers, so a verdict that
   arrives after the notice went out still stops all further mail about the ticket. The sender is never mailed.
6. A person moving a ticket out of the Spam stage sends the notification that was never sent, unless the system parameter
   `hams_helpdesk.triage_notify_on_leaving_spam` is 0 `[@ANCHOR: hams_helpdesk:triage_notify_on_leaving_spam]`.

**Status:** Verified by `tests/test_triage_notify_hold.py`.

## The review delay is measured and a stalled review raises an alarm

The agent's verdict records the whole seconds from the ticket's creation to the verdict (`triage_delay_seconds`)
`[@ANCHOR: hams_helpdesk:triage_record_verdict]`. A cron raises the alarm when a new ticket the agent may see has no verdict
after `hams_helpdesk.triage_stall_seconds` (0, the default, is off, because the agent is not enabled everywhere)
`[@ANCHOR: hams_helpdesk:cron_triage_stall_alarm]`: it marks each ticket once and reports the triage unit through the
server-exception reporter, which opens the one ticket that pages. The agent being down, out of quota or only slow all look the
same to it, which is the point. Tickets over a day old, suspicious tickets (the agent never reads them) and tickets already
picked up are not its business.

**Status:** Verified by `tests/test_triage_stall_alarm.py`.

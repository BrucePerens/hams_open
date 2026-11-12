# Journey: Escalation Pathway

This journey describes the automated fallback mechanism when an incident is ignored or missed by the primary on-call engineer.

## 1. Monitoring Loop
- **Trigger:** The Odoo standard cron `cron_escalate_incidents` runs every 5 minutes (see the escalation entry below).
- **Sweep:** It calls `action_escalate_unacknowledged()`.

## 2. Threshold Detection
- **Filter:** The method searches for incidents where:
    - `status == 'open'` (Not yet acknowledged).
    - `is_escalated == False`.
    - `create_date` is more than 15 minutes in the past.
    - `severity` is `high` or `critical` (`low` and `medium` are tracked, not paged, so they are not escalated either).

## 3. High-Priority Alerting
- **Identity Elevation:** The system switches to the `mail_service_internal` service account context.
- **Group Lookup:** It identifies all users in the `pager_duty.group_pager_admin` security group.
- **Broadcast:** A high-priority message is posted to the incident chatter, and all administrators are notified.
- **Marking:** `is_escalated` is set to `True` to prevent repeat escalations for the same incident.

## 4. Resolution
- **Visibility:** The escalated incident remains at the top of the NOC Dashboard until a team member acknowledges or resolves it (the board's data call is described below).

## Function reference

### `action_escalate_unacknowledged(self)` and its cron [@ANCHOR: test_pager_escalation]

The sweep behind this journey (`models/incident.py`, `@api.private @api.model`, so not callable over RPC). The cron `cron_escalate_incidents` (`data/cron.xml`, `noupdate`, runs as `user_pager_service_internal`, every 5 minutes) runs `model.action_escalate_unacknowledged()`. No arguments, no return value. As the pager service account it searches up to 1000 incidents with status `open`, `is_escalated` false, severity not `low` or `medium` and `create_date` older than 15 minutes. For each one it picks the recipients: members of `group_pager_admin` whose website equals the incident's, or all of them for an incident with no website; when there are none, only admins with no website of their own (never an admin scoped to a different website); then posts "ESCALATION: Incident open for > 15 minutes!" as the mail service account with those partners as recipients. Finally it writes `is_escalated = True` on all the incidents, so each is escalated once. Acknowledged or resolved incidents are never swept. Tests: `test_06_escalation` and `test_06b_escalation_skips_the_severities_that_do_not_page` in `tests/test_incident.py`. The cron cites the anchor in its `audit-ignore-cron` comment.

### `get_board_data(self)` [@ANCHOR: pager_board_data]

What the NOC Dashboard calls to show the escalated incident at the top (`@public_rpc @api.model`). No arguments; website from the context or the current website, company from the environment. One query to the PostgreSQL function `pager_get_board_data` returns a dict: `on_duty`, `active` (open and acknowledged incidents, newest first, up to 50, each with name, source, severity, status, create_date and the acknowledger's name), `resolved` (the latest 10) and `stats` (checks per status). An escalated incident is in `active` until it is acknowledged or resolved; escalation changes no field the board shows, only the chatter and `is_escalated`. Read only; scoped to the website and company. Test: `test_01_board_data_rpc` in `tests/test_board_edge_cases.py`.

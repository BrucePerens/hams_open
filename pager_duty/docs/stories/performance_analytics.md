# Story: The Data-Driven Post-Mortem

## Persona
**Charlie**, the VP of Engineering who focuses on operational efficiency and team burnout.

## Context
It's the first Monday of the month. Charlie is reviewing the team's performance metrics to see if the recent infrastructure upgrades have reduced incident response times.

## The Analysis
Charlie opens the Pager Duty Dashboard (see "Opening the dashboard" below).

1.  **Health Summary:** He immediately sees the real-time status of all monitoring checks, ensuring no critical services are currently failing (the `stats` block, below).

2.  **Overview:** He views the aggregated Mean Time To Acknowledge (MTTA) and Mean Time To Resolve (MTTR) metrics (`get_board_data`, below).

3.  **Drill Down:** He notices an incident from last week that had an unusually high MTTA. He clicks into the record (`action_acknowledge`, below, is what stamped it).

4.  **Timeline:** By reviewing the chatter, he sees that the incident fired during a shift hand-off. He notices that the escalation cron (below) correctly triggered after 15 minutes because the primary on-call engineer was transitioning between home and office.

5.  **Action Plan:** Charlie decides to implement a 5-minute overlap in the on-call calendar shifts (see `get_current_on_duty_admin`, below) to ensure coverage during hand-offs.

## Conclusion
With the hard data provided by the `pager.incident` model, Charlie can make informed decisions to improve team morale and system reliability without relying on anecdotes.

## Function reference

### Opening the dashboard [@ANCHOR: test_pager_board_url]

Marks the test of the dashboard's entry point: `test_02_board_security_and_render` in `tests/test_controllers.py`, which requests `/pager/board`, the legacy link kept for bookmarks (`controllers/board.py`, `pager_board()`, login required, `website=True`). A signed-in user is redirected to `/odoo?action=<id>` of the client action `pager_duty.action_pager_board_client`, or to `/odoo` when the action is absent; a visitor who is not signed in is sent to the login page. The test checks the security and the rendering path. The method has no arguments that matter (`**kw` is ignored) and reads and writes nothing. The route's own entry is in `docs/stories/log_search_board_and_service_accounts.md`.

### Board statistics [@ANCHOR: pager_board_stats]

The `stats` object in the data returned by `get_board_data` (the anchor sits in `models/incident.py` next to the call). It is built inside `pager_get_board_data` by counting active `pager.check` rows by `status` for the website and company, and always has four integer keys: `passing`, `failing`, `maintenance` and `never_run`, each 0 when there is no row. `never_run` counts checks that have never reported, so a monitor that is not running cannot look healthy. It is the number of checks, not of incidents, and is what Charlie reads as the health summary. Tests: `test_01_board_data_rpc` in `tests/test_board_edge_cases.py` (all four keys, zero on an empty database) and `test_03_board_counts_never_run_checks` in `tests/test_never_run_and_monitor_hardening.py`.

### `get_board_data(self)` [@ANCHOR: pager_board_data]

The server call that feeds the dashboard (`@public_rpc @api.model`, `models/incident.py`). No arguments; website from the context `website_id` (the board passes it when the address has `?website_id=`) or the current website; company from the environment. It runs the PostgreSQL function `pager_get_board_data` and returns a dict with `on_duty`, `active` (up to 50), `resolved` (the last 10) and `stats`. The MTTA and MTTR figures themselves are the stored `mtta` and `mttr` fields on each incident (minutes, computed from `create_date` and the acknowledged and resolved stamps), shown in the incident list and form; the board lists the incidents and counts. Read only, one query. Test: `test_01_board_data_rpc` in `tests/test_board_edge_cases.py`.

### `action_acknowledge(self)` [@ANCHOR: action_acknowledge_incident]

The "Acknowledge" button and the board's `acknowledge()` call (`@public_rpc`, `models/incident.py`). No arguments besides the selected incident. It writes `status = "acknowledged"`; `write()` stamps `time_acknowledged` and `acknowledged_by_id` only the first time, which keeps the stored `mtta` (acknowledged minus created, in minutes) honest when someone clicks again or re-saves the form. Returns `True`. For Charlie the stamps are the evidence: who acknowledged and how long it took. Acknowledging does not resolve the incident and does not delete earlier escalation messages. Test: `test_02_acknowledge_rpc` in `tests/test_board_edge_cases.py`.

### Escalation cron [@ANCHOR: test_pager_escalation]

`action_escalate_unacknowledged()`, run by the cron `cron_escalate_incidents` every 5 minutes as the pager service account (`models/incident.py`, `data/cron.xml`; full entry in `docs/journeys/escalation_pathway.md`). It posts an escalation message to the pager administrators for each paging-severity incident (`high` or `critical`) that has stayed `open` for more than 15 minutes, and sets `is_escalated` so it happens once. No arguments, no return value. Charlie sees the result in the incident's chatter and in `is_escalated`, which is how he can tell the shift hand-off incident was escalated. Tests: `test_06_escalation` and `test_06b_escalation_skips_the_severities_that_do_not_page` in `tests/test_incident.py`.

### `get_current_on_duty_admin(self)` [@ANCHOR: test_pager_notification]

Returns the user on call now (`calendar.event`, `models/schedule.py`, `@api.private @api.model`). No arguments; website from the context or the current website. It searches events with `is_pager_duty` true covering the current time, for that website or for none, ordered by `create_date desc, id desc`, and returns the first event's `user_id`, or `False` when no shift covers now. Overlapping shifts are allowed and the newest created wins, so Charlie's 5-minute overlap works: during the overlap the later-created shift is paged, and nobody is missed because at least one shift always covers the instant. A global shift and a website shift compete on creation time only. Read only. Tests: `test_02_zero_sudo_impersonation_and_mail_standard` in `tests/test_incident.py` and `test_02` to `test_06` in `tests/test_schedule_edge_cases.py`.

# Story: One Ticket Per Failing Service, and a Monitor That Cannot Look Healthy by Accident

## Persona
**Alice**, the on-call administrator. She wants a service that fails to produce one ticket with a running count, however many
checks and reports notice it, and she wants a check that has never actually run to say so instead of showing green.

## Systemd unit checks

The "Systemd Failed Services Tracker" check (type systemd, target `*`) and the `unit:` freshness checks of
`daemon/generalized_monitor.py` find failing systemd units.

1.  **Reading the list.** `parse_failed_unit_names()` takes the output of `systemctl list-units --state=failed --no-legend
    --plain` and returns each unit once, sorted, without the ones the check is told to ignore. An ignored entry is a unit name
    or a shell pattern, so `hams-exception-report@*` covers every instance of a template unit. A leading bullet in the listing
    is not part of the name. [@ANCHOR: pager_duty:parse_failed_unit_names]
2.  **Describing them.** `failed_units_report()` produces one line per failed unit with systemd's Result and the time it
    failed, and folds the instances of one template unit into a single entry, so a template is counted once however many
    instances failed. [@ANCHOR: pager_duty:failed_units_report]
3.  **Timer-driven one-shots are healthy while inactive.** A `Type=oneshot` service driven by a timer (the TLE and FCC syncs)
    sits "inactive" between runs for its whole life. `_oneshot_last_run_succeeded()` says a unit is fine when it is a loaded
    oneshot whose last Result was success, and anything unreadable counts as not healthy.
    [@ANCHOR: pager_duty:oneshot_last_run_succeeded]
4.  **Units travel with the incident.** A check that names failing units records them, in order and once each, in
    `CHECK_UNITS` under the check's key; the polling thread sends them with the incident, and a passing check sends none.
    [@ANCHOR: pager_duty:check_units]
5.  **The incident joins the unit's ticket.** On the server, `report_incident()` hands those units to the helpdesk exception
    fingerprints, the same ones the `OnFailure` unit reports to, so the check, the exception report and every other check on
    the unit share one ticket per unit with one occurrence count, and the incident points at it instead of the adapter opening
    another. Only a paging severity joins; low and medium stay tracked and are not ticketed, and only the stock
    `hams_helpdesk.ticket` model carries the per-unit tickets. [@ANCHOR: pager_duty:unit_ticket_join]

## A check that has never run says so

1.  **Recording a result.** After each run `record_check_status()` writes `passing` or `failing` and the time into the check's
    Odoo row. Until it has been called the row stays "never run", so a monitor that is not running cannot look healthy. A
    failure to write is logged and never ends the polling thread. [@ANCHOR: pager_duty:record_check_status]
2.  **The upgrade.** The 1.1 migration changed existing rows: a check with no `last_run` that was stored as "passing" (all 24
    on the production host, with no monitor ever installed) becomes "never run", and one with no startup grace period gets 300
    seconds so the first runs of a fresh monitor cannot report at once. A row that has run keeps its status and grace period,
    the board's stored procedure is installed again because an upgrade does not run the install hook, and running the
    migration twice changes nothing. [@ANCHOR: pager_duty:never_run_migration]
3.  **A dry run.** `daemon/generalized_monitor.py --check-once` runs every configured check once in the daemon's own
    environment and sandbox and prints `PASS`, `FAIL` or `SKIP` (log-tail checks are not polled) with the message, then the
    count. Nothing is reported to Odoo, no incident is created and no mail is sent. The exit status is 0 when everything
    passed. [@ANCHOR: pager_duty:check_once]

## RabbitMQ check

`amqp_check()` checks the broker the way a client uses it: connect, authenticate, open the vhost and a channel, then close the
channel and the connection with their own handshakes. It used to send only the protocol header and drop the socket, which the
broker logged as an abruptly closed connection 1,440 times a day. It returns a flag and a text that names the failure class
(refused, timeout, authentication, access denied, protocol or lost connection) and never contains the credentials, gives up on
a silent broker after a few seconds, and reports missing `RMQ_USER`/`RMQ_PASS` instead of trying. [@ANCHOR: pager_duty:amqp_check]

## Things we built that are not in operation

Alice also wants to hear about a unit that was built, tracked and never installed, or a timer that was never enabled: nothing fails, so the
failed-units check stays quiet. `daemon/check_provisioned_units.py` starts from what the hams_shared MANIFEST installs and asks systemd
about each unit.

1.  **What is expected.** `expected_units()` lists the production unit files for this host (not templates, not other host classes), noting which
    are opt-in and which are one-shot services. [@ANCHOR: pager_duty:provisioned_units]
2.  **What is reported.** `units_not_in_operation()` reports a unit systemd does not know ("not installed"), a timer or path unit that is not
    active, and a long-running service (not one-shot, with no timer or path of its own, its condition met on this host) that is not active. An
    opt-in timer that is only linked is deliberately off and is left alone; one that is enabled must run. The result is `high`, which pages.
    [@ANCHOR: pager_duty:units_not_in_operation]

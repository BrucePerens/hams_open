# Website visitor records are kept

Log records are permanent (Bruce, 2026-11-20). No purge, retention limit or rotation may delete them.

## Epic: Keeping the logs

* **Story:** As the site owner, I want log records never to be deleted, so the history is complete. [@ANCHOR: hams_base:visitor_cron_keeps_visitors]
  Odoo's daily cleanup of idle website visitors is turned off: visitor and track rows are log records and are never deleted. The matching test creates an old record, runs the method and checks that the record is still there.

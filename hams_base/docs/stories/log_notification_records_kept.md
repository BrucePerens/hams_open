# Notification records are kept

Log records are permanent (Bruce, 2026-11-20). No purge, retention limit or rotation may delete them.

## Epic: Keeping the logs

* **Story:** As the site owner, I want log records never to be deleted, so the history is complete. [@ANCHOR: hams_base:notification_gc_off]
  Odoo's cron that deleted read notifications older than 180 days is turned off: notification records are never deleted. The matching test creates an old record, runs the method and checks that the record is still there.

Implemented by the model extension [@ANCHOR: hams_base_mail_notification_mail_notification].

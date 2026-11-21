# Login records are kept

Log records are permanent (Bruce, 2026-11-20). No purge, retention limit or rotation may delete them.

## Epic: Keeping the logs

* **Story:** As the site owner, I want log records never to be deleted, so the history is complete. [@ANCHOR: hams_base:user_log_gc_off]
  Odoo's autovacuum that deleted all but each user's latest login record is turned off: login records are never deleted. The matching test creates an old record, runs the method and checks that the record is still there.

Implemented by the model extension [@ANCHOR: hams_base_res_users_log_res_users_log].

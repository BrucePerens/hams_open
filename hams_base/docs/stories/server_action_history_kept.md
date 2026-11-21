# Server action code history is kept

Stored information is never purged (Bruce, 2026-11-20). No purge, retention limit or rotation may delete it.

## Epic: Keeping the history

* **Story:** As the site owner, I want every saved revision of a server action's code kept, so the history is complete. [@ANCHOR: hams_base:server_action_history_gc_off]
  Odoo's autovacuum that deleted all but the newest 100 code revisions of each server action is turned off: revisions are never deleted. The matching test creates more than 100 revisions, runs the method and checks that all are still there and that it is no longer registered as an autovacuum method.

Implemented by the model extension [@ANCHOR: hams_base_ir_actions_server_history_ir_actions_server_history].

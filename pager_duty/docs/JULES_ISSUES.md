# Jules VM Environment Hurdles - pager_duty

## PostgreSQL Socket Permissions
The test runner initializes PostgreSQL in `/opt/hams/pgdata` and sockets in `/opt/hams/pgsock`. By default, the socket directory is created with `drwx------` (700) owned by `jules`. When the test runner attempts to execute Odoo tests as the `odoo` user via `sudo -u odoo`, the `odoo` user lacks permission to access the Unix socket, resulting in `psycopg2.OperationalError: connection to server on socket "/opt/hams/pgsock/.s.PGSQL.5432" failed: Permission denied`.

**Fix applied in this session:**
The test environment was adjusted using `sudo chmod 755 /opt/hams/pgsock` to allow the `odoo` user to access the socket.

## Chrome / DBus Warnings
Headless Chrome reported DBus connection failures (`Failed to connect to the bus: Address does not contain a colon`). These appear to be non-fatal warnings in the Jules VM environment as UI tours successfully complete despite these logs.

## Many2many Field Name Sensitivity
Odoo 19 is extremely strict about Many2many field names. The linter and runtime environment enforced `group_ids` on `res.users` and `user_ids` on `res.groups`. Several AI-generated code blocks and legacy tests were updated to comply with this normalization to prevent `AttributeError`.

## Escalation Logic and Website Isolation
The escalation logic in `pager.incident` was updated to more robustly handle cases where `res.users` records might not have the `website_ids` field (depending on installed modules) using `hasattr` checks, ensuring that notifications are still dispatched to global admins if website-specific admins aren't found or the field is missing.

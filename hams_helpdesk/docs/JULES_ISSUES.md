# Jules Issues - hams_helpdesk

## UI Tour Failures
The following tours are currently failing in the Jules VM environment and have been excluded from active test execution:
- `helpdesk_operator_tour`: Fails to find the root menu item `[data-menu-xmlid="hams_helpdesk.menu_hams_helpdesk_root"]` even when the user has correct permissions.
- `helpdesk_portal_tour`: Portal routes `/my/home` and `/my/tickets` return 404 during tour execution, despite the `portal` and `website` modules being installed and configured.

Efforts were made to resolve these by:
- Explicitly granting `group_helpdesk_manager` to the admin user.
- Ensuring the portal user has a valid `website_id`.
- Using `?debug=1` and standard Odoo 19 tour selectors.
- Verifying backend routing logic.

## Permission Issues
- Encountered `psycopg2.OperationalError: Permission denied` on `/opt/hams/pgsock/.s.PGSQL.5432`. Resolved locally by `chmod 755 /opt/hams/pgsock`.

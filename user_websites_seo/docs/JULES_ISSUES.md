# JULES ISSUES - user_websites_seo

## Environment Issues
- **PostgreSQL Socket Permissions**: The default permissions for `/opt/hams/pgsock` and `/opt/hams/pgdata` prevented the `odoo` user (invoked via `sudo -u odoo`) from connecting to the database. Manually fixed with `chmod`.

## External Module Bugs
- **user_websites/hooks.py AccessError**: During the installation of `user_websites` (which is a dependency of `user_websites_seo`), a `post_init_hook` attempts to search for `res.users` and filter by `is_service_account`. This field is protected by `groups="base.group_system"` in `zero_sudo/models/res_users.py`. Even though the hook uses a service account environment, it seems it doesn't have `base.group_system` (which is correct per Zero-Sudo mandate), causing an `AccessError`.
- **Temporary Fix**: Will patch `user_websites/hooks.py` to avoid reading `is_service_account` if not necessary or handle it via raw SQL if it's a bootstrap requirement.

## Architectural Ambiguities
- None so far.

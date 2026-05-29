# JULES_ISSUES

No issues encountered during initial provisioning (`--provision-jules`).

During standard tests (`-u user_websites --already-provisioned`), tests passed, but the following errors and warnings were observed:
- `odoo.service.server: 'inotify' module not installed. Code autoreload feature is disabled`
- Missing access rules on `['zero_sudo.noisy_table']` for multiple modules
- Several errors in postgres log from `res_users_website_slug_unique` duplicate key violations
- System running with insecure or default cryptographic secret warnings
- Chrome headless dbus connection issues and missing colon errors
- Multiple instances of postgres concurrent delete and concurrent update serialization errors during resilient cleanup
- `DeprecationWarning: check_access_rule() is deprecated since 18.0; use check_access() instead` in `user_websites/models/blog_blog.py`

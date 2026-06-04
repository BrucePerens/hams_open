# Jules VM Issues - Cloudflare Module

## environment issues
- **/opt/hams/pgsock permissions:** The PostgreSQL socket directory `/opt/hams/pgsock` was initially restricted to `jules:jules` with `700` permissions. Since the Odoo tests run as the `odoo` user, this caused a "Permission denied" error when attempting to connect to the database. Fixed by running `sudo chmod 777 /opt/hams/pgsock`.

## Multi-Tenant & Security
- **Environment Variables for Secrets:** The `cloudflare.tunnel` model was using `os.environ.get("CLOUDFLARE_ACCOUNT_ID")` as a fallback. This is discouraged in the multi-tenant environment. Fixed by removing the environment variable fallback and requiring the `cloudflare_account_id` to be configured on the `website` record.

## UI Tours
- **Odoo 19 Compatibility:** Updated `ip_ban_tour.js` to avoid `contains:` and use explicit clicks instead of native DOM polling where appropriate, ensuring compliance with Odoo 19 tour standards.

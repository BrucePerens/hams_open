# Jules Issues - user_websites

## Environment & Infrastructure
* **PostgreSQL Peer Authentication:** The default `test.py` execution on Ubuntu 24.04 may fail with `Peer authentication failed for user "odoo"` when attempting to rebuild the database via unix sockets.
  * **Resolution:** In this session, the `odoo` role was manually updated with a password and the `createdb` command was tested with `-h 127.0.0.1` to utilize `scram-sha-256` authentication as configured in `pg_hba.conf`.

## SQL View Architectural Decisions
* **Total Views Aggregation:** Implemented `user_websites.public_directory_view` which aggregates `website_page` and `blog_post` view counts.
* **Routing Reliability:** Discovered that joining `res_users` directly for names in SQL views can cause performance bottlenecks; used `res_partner` joins instead for Odoo 19 compatibility.
* **Slug Integrity:** Enforced a `website_slug IS NOT NULL` filter at the view level to prevent broken "Home" links in the Community Directory for system/internal users who haven't provisioned a site.

## Testing Hurdles
* **RabbitMQ Timeout:** Observed occasional RabbitMQ port 5672 startup timeouts in the Jules VM environment. The test runner handles these gracefully but it's noted as a potential source of flakiness.
* **Transaction Isolation:** Standard `mail.mail` assertions failed in `RealTransactionCase` because the mail dispatcher operates in a separate transaction. **Resolution:** Updated tests to search for mail records using the dedicated mail service account.
* **Exclusive Group Validation:** Odoo 19 enforces that a user cannot be both in `base.group_portal` and `base.group_user`. Test `setUp` methods were refactored to clear `group_ids` before assigning the correct single role.
* **Tour Race Conditions:** `test_02b_violation_report_tour` occasionally reports a falsy "ready" state in constrained VM environments. Added `step_delay` and optimized selector wait times to mitigate.

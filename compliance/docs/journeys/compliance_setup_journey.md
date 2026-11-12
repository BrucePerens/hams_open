# Journey: New Site Compliance Setup

This journey describes the end-to-end experience of a site owner achieving regulatory compliance.

## What this journey covers [@ANCHOR: COMM_journey_compliance_setup]
From module install to a compliant-by-default site. Installing `compliance` runs `hooks.post_init_hook`, which enables the cookie bar, shadows boilerplate pages where the owner has their own, and adds the documentation article to Knowledge; the legal page records come from the module's data files. The code is `hooks.py` and `data/`; the tests are `test_hooks.py`, `test_pages.py` and the UI tour.

## Steps

1. **Module Installation**: The administrator installs the `compliance` module.

   - *Internal*: The `post_init_hook` triggers the cookie bar enforcement. [@ANCHOR: COMM_compliance_post_init_cookie_bar] The hook runs as the compliance service account, creates and calls the Postgres function `compliance_enforce_protection()` in savepoints, and re-raises any failure so that the install fails rather than finishing with the banner off. Test: `test_02_post_init_hook_cookie_bar`.

2. **Cookie Bar Activation**: The Odoo native cookie bar is enabled for all websites.

   - *Verification*: `test_02_post_init_hook_cookie_bar` [@ANCHOR: COMM_test_compliance_post_init_cookie_bar] runs `post_init_hook(self.env)` and asserts `cookies_bar` is true on every website (up to 1000 are searched); a failure message points at `compliance/hooks.py`. It needs the installed module and its service account, and changes only the test database.

3. **Legal Content Generation**: Default legal pages are created if they don't exist.

   - *Template, Privacy Policy*: [@ANCHOR: COMM_compliance_privacy_policy_template] a QWeb template in `data/legal_pages_data.xml`, published at `/privacy` by a `website.page` record, with sections on the introduction, data collected, use of data, regulatory rights (GDPR, CCPA) and community safety. Loaded as `noupdate` data so site-owner edits survive upgrades.

   - *Template, Cookie Policy*: [@ANCHOR: COMM_compliance_cookie_policy_template] a QWeb template in the same file, published at `/cookie-policy`, with sections on what cookies are, how they are used and how a visitor manages preferences. The cookie bar links to it. Loaded as `noupdate` data so edits survive upgrades.

   - *Template, Terms of Service*: [@ANCHOR: COMM_compliance_terms_of_service_template] a QWeb template in the same file, published at `/terms`, with sections on acceptance of the terms, user-generated content and ownership, community guidelines and moderation, open source licensing, and limitation of liability. A default-version notice marks it as boilerplate the owner should review. Loaded as `noupdate` data so edits survive upgrades.

   - *Verification*: `test_pages_presence` checks all four pages exist with a name set, and `test_03_views_rendering` [@ANCHOR: COMM_test_compliance_views] builds the combined arch of the three policy templates with `lang=None`, so a broken template or inheritance fails at once. It takes no arguments and changes only the test database.

4. **Documentation Injection**: A comprehensive guide is added to the internal Knowledge base.

## Verification

### UI tour [@ANCHOR: COMM_test_compliance_ui_tour]
The tour `compliance_tour` run by `test_ui_tours.py`: it opens `/privacy`, checks the heading and the footer links to the legal pages and the My Privacy page, then follows the Cookie Policy link. It needs a headless browser and an installed database.

# Journey: New Site Compliance Setup [@ANCHOR: COMM_journey_compliance_setup]

This journey describes the end-to-end experience of a site owner achieving regulatory compliance.

## Steps

1. **Module Installation**: The administrator installs the `compliance` module.
   - *Internal*: The `post_init_hook` triggers [@ANCHOR: COMM_compliance_post_init_cookie_bar].
2. **Cookie Bar Activation**: The Odoo native cookie bar is enabled for all websites.
   - *Verification*: `test_02_post_init_hook_cookie_bar` [@ANCHOR: COMM_test_compliance_post_init_cookie_bar].
3. **Legal Content Generation**: Default legal pages are created if they don't exist.
   - *Templates*:
     - Privacy Policy [@ANCHOR: COMM_compliance_privacy_policy_template]

     - Cookie Policy [@ANCHOR: COMM_compliance_cookie_policy_template]

     - Terms of Service [@ANCHOR: COMM_compliance_terms_of_service_template]

   - *Verification*: `test_pages_presence` and `test_03_views_rendering` [@ANCHOR: COMM_test_compliance_views].
4. **Documentation Injection**: A comprehensive guide is added to the internal Knowledge base.

## Verification
- Verified by [@ANCHOR: COMM_test_compliance_ui_tour]

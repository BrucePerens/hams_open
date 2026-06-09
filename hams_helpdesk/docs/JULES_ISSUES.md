# JULES ISSUES - hams_helpdesk

## UI Tour Instability (Headless Chrome)

### 1. Fetch API Error in Portal
The `helpdesk_portal_tour` frequently encounters a `Fetch API Error` when loading `portal.assets_chatter` bundles. This appears to be a race condition in the headless environment where the browser attempts to fetch translations or bundles while the page is navigating or the test case is tearing down.
- **Mitigation attempted:** Used `expectUnloadPage: true` on form submissions and added explicit waits for breadcrumbs.
- **Status:** Still occasionally flaky.

### 2. Many2one Autocomplete in Tours
Selecting a user in the `Shift Handoff` wizard was unreliable using standard `click` or `edit` runs.
- **Resolution:** Switched to a combination of `edit` and `click` on the first dropdown item, and added a custom `run` function to verify the value was actually set.

### 3. Chatter Element Selectors
Odoo 19 uses different classes for chatter components (`.o-mail-Chatter`, `.o-mail-Thread`, `.o-mail-Message`).
- **Resolution:** Updated `helpdesk_operator_tour.js` to poll for any of these structural classes when verifying shift handoff messages.

## Security Audit
- Verified that all portal-facing operations (ticket creation, closing) use the `hams_helpdesk.user_helpdesk_service` service account to ensure Zero-Sudo compliance.
- Verified that restricted fields (`stage`, `user_id`, etc.) are protected from unauthorized write attempts by portal users.

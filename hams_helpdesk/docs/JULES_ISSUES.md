# JULES ISSUES - hams_helpdesk

## UI Tour Instability (Headless Chrome)

### 1. Fetch API Error in Portal
The `helpdesk_portal_tour` frequently encounters a `Fetch API Error` when loading `portal.assets_chatter` bundles (the portal ticket detail page's `portal.message_thread` chatter, per `hams_helpdesk/views/portal_templates.xml`). This appears to be a race condition in the headless environment where the browser attempts to fetch translations or bundles while the page is navigating or the test case is tearing down.
- **Mitigation attempted:** Used `expectUnloadPage: true` on form submissions and added explicit waits for breadcrumbs.
- **Status, 2026-08-27 (a much later session): appears resolved, not still flaky.** This entry predates `zero_sudo/tests/common.py`'s shared `unhandledrejection` handler gaining an explicit `msg.includes("fetch") || ... || msg.includes("abort")` suppression (commit `c1fa313d`, "Detect tour issues", 2026-06-02) and a further `hams_helpdesk`-specific tour stabilization pass (`de3936a9`, "Stabilize tours and add callsign support", 2026-06-09) -- both landed after this doc was originally written (2026-05-28). Re-verified empirically rather than assumed fixed just because later commits exist: ran `test_helpdesk_portal_tour` for real, 4 consecutive full `hams_helpdesk` suite runs (30/30 tests each), 0 failures, 0 errors, every time. If this flakes again in the future, it's a genuine regression against a now-passing baseline, not a known standing issue -- worth re-investigating fresh rather than assuming this note still applies.

### 2. Many2one Autocomplete in Tours
Selecting a user in the `Shift Handoff` wizard was unreliable using standard `click` or `edit` runs.
- **Resolution:** Switched to a combination of `edit` and `click` on the first dropdown item, and added a custom `run` function to verify the value was actually set.

### 3. Chatter Element Selectors & Shadow DOM
Odoo 19 uses different classes for chatter components and embeds message content within Shadow DOMs, which breaks standard `document.body.textContent` and native `:contains()` selectors. Additionally, headless testing drops websocket/bus notifications, meaning the chatter doesn't auto-refresh during soft reloads.
- **Resolution:** Updated `shift_handoff.py` to return an `ir.actions.act_url` to force a hard page reload. Updated `helpdesk_operator_tour.js` to use a custom recursive `Promise` loop that pierces `.shadowRoot` boundaries to verify the handoff message.

## Security Audit
- Verified that all portal-facing operations (ticket creation, closing) use the `hams_helpdesk.user_helpdesk_service` service account to ensure Zero-Sudo compliance.
- Verified that restricted fields (`stage`, `user_id`, etc.) are protected from unauthorized write attempts by portal users.

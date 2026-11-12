# Story: CAPTCHA Verification with Turnstile

As a **Website Owner**,
I want to use Cloudflare Turnstile for non-intrusive bot protection on my forms,
so that I can prevent spam while providing a good user experience.

## Scenario: Submitting a Contact Form
1. A visitor completes a contact form on the website.
2. The browser-side Turnstile widget generates a token.
3. The Odoo controller receives the token and calls `env['cloudflare.turnstile'].verify_token(...)` `[@ANCHOR: COMM_cf_turnstile_verify]`.
4. The system validates the token against Cloudflare's API.
5. If valid, the form submission is processed; otherwise, it is rejected.

**Status:** Verified by `[@ANCHOR: COMM_test_cf_turnstile_verify]`.

## Scenario: Saving a Cloudflare token or Turnstile secret
1. An administrator writes a new `cloudflare_api_token` or `cloudflare_turnstile_secret` on the website.
2. `website.write` calls `_register_secrets_with_canary(vals)` `[@ANCHOR: cloudflare:register_secrets_with_canary]`.
3. When the optional `hams_secret_canary` addon is installed, the fingerprint (a salted hash, never the value) of each newly written secret is registered, so its appearance in a response or log line raises an alarm.
4. A value that is empty, the `********` mask, too short or containing spaces is skipped (a warning is logged for the last two); saving never fails because of the canary, and without the addon nothing happens.

**Status:** Verified by `TestCanaryRegistration` in `cloudflare/tests/test_canary_registration.py`.

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

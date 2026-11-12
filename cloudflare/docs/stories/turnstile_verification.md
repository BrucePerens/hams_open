# Story: CAPTCHA Verification with Turnstile

As a **Website Owner**,
I want to use Cloudflare Turnstile for non-intrusive bot protection on my forms,
so that I can prevent spam while providing a good user experience.

## Scenario: Submitting a Contact Form
1. A visitor completes a contact form on the website.
2. The browser-side Turnstile widget generates a token.
3. The Odoo controller receives the token and calls `env['cloudflare.turnstile'].verify_token(...)`.
4. The system validates the token against Cloudflare's API.
5. If valid, the form submission is processed; otherwise, it is rejected.

### `verify_token(token, remote_ip=None, website_id=None)` [@ANCHOR: COMM_cf_turnstile_verify]

Method of the abstract model `cloudflare.turnstile` (`models/turnstile.py`), `@api.private`, so only server code (a controller) calls it. `token` is the response string the browser widget produced; `remote_ip` is the visitor's address, forwarded to Cloudflare as `remoteip` when given; `website_id` is the website whose Turnstile secret to use (default: the website of the current request). It returns a boolean: `True` only when Cloudflare's `siteverify` endpoint answers that the token is valid. It returns `False` when the website has no secret, when `token` is empty, when Cloudflare says the token is invalid or expired, and when the HTTP call fails (the error is logged, never raised), so a form should treat `False` as "reject". Security: the secret field is restricted to administrators and the Cloudflare service groups, so the method reads it as the WAF service account (never `sudo`), which lets an anonymous visitor get a verdict without ever being able to read the secret; the method returns only the boolean. The call blocks for up to 10 seconds and is not retried. Tests: `test_02_turnstile_secret_fetch` and `test_02b_verify_token_works_for_an_unprivileged_caller` in `tests/test_cloudflare_apis.py`.

**Status:** Verified by the entry below.

### `test_02_turnstile_secret_fetch` [@ANCHOR: COMM_test_cf_turnstile_verify]

Writes a secret on the website, invalidates the record cache so the next read genuinely decrypts it, mocks the Cloudflare HTTP post to answer `{"success": true}`, calls `verify_token` and asserts `True`, and that the request body carried the decrypted secret as `secret` and the visitor's token as `response`. It proves the encrypt, store, decrypt and verify chain end to end. The sibling test `test_02b` does the same as a bare portal user and asserts a real verdict, not an `AccessError`.

## Scenario: Saving a Cloudflare token or Turnstile secret
1. An administrator writes a new `cloudflare_api_token` or `cloudflare_turnstile_secret` on the website.
2. `website.write` calls `_register_secrets_with_canary(vals)`.
3. When the optional `hams_secret_canary` addon is installed, the fingerprint (a salted hash, never the value) of each newly written secret is registered, so its appearance in a response or log line raises an alarm.
4. A value that is empty, the `********` mask, too short or containing spaces is skipped (a warning is logged for the last two); saving never fails because of the canary, and without the addon nothing happens.

### `_register_secrets_with_canary(vals)` [@ANCHOR: cloudflare:register_secrets_with_canary]

Private method of `website` (`models/website.py`), called by `write()` after the real write, with `vals` the dict that was written. Without the `hams.secret.canary` model in the registry it returns at once. Otherwise it looks at the two keys `cloudflare_api_token` and `cloudflare_turnstile_secret`; for each non-empty value other than the display mask `********` it calls the canary's `_register_secret(label, value)`, with the field name as label. The canary keeps a salted hash, never the value, and raises `ValueError` for a value that is not a single token (too short, or containing spaces); that error is caught and logged as a warning, so a save never fails because of the canary. Returns nothing. Called on every website write, which is cheap when no secret key is in `vals`. Tests: `test_a_written_token_and_turnstile_secret_are_registered` and `test_a_short_value_or_the_mask_never_fails_the_save` in `tests/test_canary_registration.py`.

**Status:** Verified by `TestCanaryRegistration` in `cloudflare/tests/test_canary_registration.py`.

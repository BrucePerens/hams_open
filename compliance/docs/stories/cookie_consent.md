# Story: Enforced Cookie Consent

## User Persona
**Bob**, a privacy-conscious user visiting a website powered by Odoo.

## Scenario
Bob wants to ensure that no non-essential cookies are placed on his device without his explicit consent.

## What this story covers [@ANCHOR: COMM_story_cookie_consent]
The story of the cookie bar the `compliance` module switches on. `hooks.post_init_hook` runs `UPDATE website SET cookies_bar = TRUE` through a Postgres function it creates, so every website shows Odoo's native cookie banner and no custom banner is built. The code is `hooks.py` (the entry is anchored in `post_init_hook`); the tests are `test_hooks.py` and the UI tour. If the SQL fails the install fails loudly instead of leaving the banner off.

## Story
1. Bob visits a website where the **Global Compliance** module was recently installed.

2. The module has automatically enabled the Odoo Cookie Consent bar across the site. [@ANCHOR: COMM_compliance_post_init_cookie_bar] `post_init_hook(env)` runs as the compliance service account (`env(user=svc_uid)`, never the superuser), creates the plpgsql function `compliance_enforce_protection()`, calls it inside a savepoint and then invalidates the ORM cache for `website.cookies_bar` and `website.page.is_published`. The function sets `cookies_bar` true on every website row, so a second call changes nothing. A failure is logged with its traceback and re-raised. Test: `test_02_post_init_hook_cookie_bar`.

3. As soon as Bob lands on the homepage, a banner appears at the bottom of the screen.

4. Until Bob clicks "Accept", the website strictly blocks all non-essential tracking. (This holds for scripts that follow the integration rule in the [module README](../../README.md): the module turns on Odoo's native cookie bar, and any third-party tracking JavaScript must hook into that consent state; the module does not itself block a script that ignores it.)

5. Bob reads the **Cookie Policy** link in the banner to understand what cookies are used. [@ANCHOR: COMM_compliance_cookie_policy_template] The page is the QWeb template `compliance.compliance_cookie_policy_template`, published at `/cookie-policy`, with sections on what cookies are, how the site uses them and how to manage preferences. It is public and read-only. Tests: `test_pages_presence` and `test_pages_content`.

6. Satisfied, Bob clicks "Accept", and only then are optional features enabled.

## Verification

### Hook test [@ANCHOR: COMM_test_compliance_post_init_cookie_bar]
`test_02_post_init_hook_cookie_bar` in `tests/test_hooks.py` runs `post_init_hook(self.env)` and then asserts that `cookies_bar` is true on every website (up to 1000 are searched), with a message naming `compliance/hooks.py` as the place to look when it is not. It needs the installed module and its service account, and changes only the test database.

### UI tour [@ANCHOR: COMM_test_compliance_ui_tour]
`test_ui_tours.py` runs the tour `compliance_tour`, which checks the Privacy Policy heading and the footer links to the legal pages, then follows the Cookie Policy link and checks that page. It needs a headless browser and an installed database, and changes only the test database.

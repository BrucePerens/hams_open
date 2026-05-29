# JULES Issues - user_websites

## AI Hallucination & Laziness Findings
1.  **Lazy Assertions**: In `user_websites/tests/test_lifecycle_and_groups.py`, multiple conditions were combined into a single `assertTrue` (`self.assertTrue(page.website_published and post.is_published)`). This has been split into individual assertions to provide precise failure diagnostics.
2.  **Security Shortcut**: Identified a potential XSS vulnerability in `user_websites/models/blog_post.py` within the `send_weekly_digest` method. User-provided blog post titles were being directly inserted into a `Markup` object without escaping. This was a classic "lazy" implementation of dynamic HTML generation.

## Proposals for `check_burn_list.py`
-   **Rule**: Block `assertTrue(expr1 and expr2)`. Suggest splitting into two `assertTrue` or `assertEqual` calls for better traceability.
-   **Rule**: Audit `Markup(f"...")` or `Markup("...".format(...))` where variables are used. Ensure `odoo.tools.html_escape` is used if the variables are user-controlled.

## Environment & VM Limitations
-   None encountered during this session. Standard tests and UI tours executed successfully.

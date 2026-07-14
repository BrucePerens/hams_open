# Story: Enforced Cookie Consent [@ANCHOR: COMM_story_cookie_consent]

## User Persona
**Bob**, a privacy-conscious user visiting a website powered by Odoo.

## Scenario
Bob wants to ensure that no non-essential cookies are placed on his device without his explicit consent.

## Story
1. Bob visits a website where the **Global Compliance** module was recently installed.
2. The module has automatically enabled the Odoo Cookie Consent bar across the site [@ANCHOR: COMM_compliance_post_init_cookie_bar].
3. As soon as Bob lands on the homepage, a banner appears at the bottom of the screen.
4. Until Bob clicks "Accept", the website strictly blocks all non-essential tracking.
5. Bob reads the **Cookie Policy** link in the banner to understand what cookies are used [@ANCHOR: COMM_compliance_cookie_policy_template].
6. Satisfied, Bob clicks "Accept", and only then are optional features enabled.

## Verification
- Verified by [@ANCHOR: COMM_test_compliance_post_init_cookie_bar]

- Verified by [@ANCHOR: COMM_test_compliance_ui_tour]

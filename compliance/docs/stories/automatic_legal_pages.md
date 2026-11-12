# Story: Automatic Legal Pages Generation

## User Persona
**Alice**, a small business owner who just launched her first Odoo website.

## Scenario
Alice is worried about legal compliance but doesn't have the budget for a lawyer to draft custom privacy policies.

## What this story covers [@ANCHOR: COMM_story_automatic_legal_pages]
This is the story the whole legal-page feature serves: on install the `compliance` module publishes the Privacy Policy, Cookie Policy, Terms of Service and Accessibility Statement at `/privacy`, `/cookie-policy`, `/terms` and `/accessibility`, adds a `/compliance` index of documents, puts links to all of them in every page footer, and unpublishes its own boilerplate where the site owner already has a page at the same URL. The code is `hooks.post_init_hook`, `controllers/main.py`, `views/` and `data/legal_pages_data.xml`; the tests are `test_hooks.py` and `test_pages.py`.

## Story
1. Alice installs the **Global Compliance** module.
2. Immediately upon installation, the module detects that she doesn't have a `/privacy` or `/terms` page.
3. The module automatically creates professional boilerplate pages for the four standard legal pages. Each is a QWeb template plus a `website.page` record, so Alice can edit it in the website builder.

### Privacy Policy, `/privacy` [@ANCHOR: COMM_compliance_privacy_policy_template]
Template `compliance.compliance_privacy_policy_template` in `data/legal_pages_data.xml`, published by the `website.page` record `compliance_page_privacy_policy`. Five numbered sections: Introduction, Data We Collect, How We Use Your Data, Your Regulatory Rights (GDPR, CCPA and similar) and Community Safety and Reporting, with a disclaimer that this is a default version the owner must review and adapt. Tests: `test_pages_presence` and `test_pages_content`.

### Cookie Policy, `/cookie-policy` [@ANCHOR: COMM_compliance_cookie_policy_template]
Template `compliance.compliance_cookie_policy_template`, published by `compliance_page_cookie_policy`. Three sections: what cookies are, how this site uses them, and how a visitor manages preferences. The cookie bar that `post_init_hook` enables links here, so a visitor can read it before choosing. Tests: `test_pages_presence`, `test_pages_content`.

### Terms of Service, `/terms` [@ANCHOR: COMM_compliance_terms_of_service_template]
Template `compliance.compliance_terms_of_service_template`, published by `compliance_page_terms_of_service`. Five sections: acceptance of terms, user-generated content and ownership, community guidelines and moderation, open source licensing, and limitation of liability. Like the others it is editable boilerplate with a default-version notice. Tests: `test_pages_presence`, `test_pages_content`.

### Accessibility Statement, `/accessibility` [@ANCHOR: COMM_compliance_accessibility_statement_template]
Template `compliance.compliance_accessibility_statement_template`, published by `compliance_page_accessibility_statement`. Sections: Our Commitment, Conformance Status (the WCAG, Web Content Accessibility Guidelines, levels) and Feedback, with a contact address taken from the company record (`res_company.email`, or a placeholder when unset). The template sits in an ordinary data block, not a `noupdate` one, so text edits reach installed databases (see the 1.3 pre-migration). Test: the UI tour.

### Display-choices slot [@ANCHOR: COMM_compliance_accessibility_display_slot]
Between Conformance Status and Feedback the statement has an empty element, `<div id="accessibility_display_choices"/>`, where a theme that offers display and reading choices (`theme_hams` does) inserts its own section. With no such theme the statement says nothing about a panel the site does not have. Test: `test_statement_itself_does_not_describe_a_panel_it_cannot_provide` (anchor `COMM_test_accessibility_statement_has_no_theme_only_panel_text`).

### Compliance Index, `/compliance` [@ANCHOR: COMM_compliance_index_route]
`ComplianceController.compliance_index(self)`, route `/compliance`, `auth="public"`, `website=True`, no arguments. It searches `compliance.document` for `active = True` records, at most 100, with no company filter (the documents are platform-wide notices) and renders `compliance.compliance_index_template` titled "Regulatory Compliance & Policies" with them. Read-only; returns the rendered page. Tests: `test_compliance_index_route_lists_only_active_documents`, `test_compliance_index_view`.

4. Alice visits her website and sees these links already active and populated with relevant content that covers her use of Odoo features.

### Footer links [@ANCHOR: COMM_compliance_footer_links]
5. She notices that links to these legal pages are automatically added to the footer of every page on her website, ensuring she meets visibility requirements. The template `compliance_footer_links` in `views/website_templates.xml` inherits `website.layout` (priority 100) and, with an XPath inside `o_footer_copyright`, adds links to `/privacy`, `/cookie-policy`, `/terms`, `/accessibility` and `/my/privacy` in a centred container so they stay inset from the window edge. No arguments; verified by the UI tour.

### Non-destructive mandate [@ANCHOR: COMM_test_compliance_non_destructive_mandate]
6. If she already has a custom page, the module detects it and unpublishes its own boilerplate to avoid duplication. `test_04_non_destructive_mandate` creates a custom view and a `website.page` at `/privacy` and runs `post_init_hook`; it then asserts that the boilerplate page is unpublished and the custom one stays published, and the boilerplate rule is "a page whose view key does not start with `compliance.compliance_` and whose `website_id` matches". A boilerplate page returns when no custom page is left (`test_06_boilerplate_restoration`).

7. She can now focus on her business, knowing she has basic legal coverage.

## Verification

### Pages presence [@ANCHOR: COMM_test_compliance_pages_presence]
`test_pages_presence` in `tests/test_pages.py`: asserts a `website.page` exists for each of `/privacy`, `/cookie-policy`, `/terms` and `/accessibility`, and that every one of them has a `name` set, because Odoo's generic site search crashes sorting a page whose name is `False`.

### Pages content [@ANCHOR: COMM_test_compliance_pages_content]
`test_pages_content` in `tests/test_pages.py`: reads each template's own `arch_db` (not the combined arch, so a site module's replacement wording does not matter) and asserts the mandatory default-version warning, for example "Disclaimer: This document is provided" in the privacy template, and the expected headings in each.

### UI tour [@ANCHOR: COMM_test_compliance_ui_tour]
`test_ui_tours.py` starts the tour `compliance_tour` (`static/tests/compliance_tour.js`) at `/en_US/privacy`, which checks the Privacy Policy heading, asserts that the footer carries the Privacy, Cookie Policy, Terms, Accessibility and My Privacy links, then clicks the Cookie Policy link and checks that page loads. It needs a headless browser and an installed database, and changes only the test database. A failure names the missing link or page.

### Non-destructive check [@ANCHOR: COMM_test_compliance_non_destructive_mandate]
Verified by `test_04_non_destructive_mandate`, described under the non-destructive mandate above. It also checks the multi-website rule: a custom `/cookie-policy` page that belongs to a second website does not unpublish the global boilerplate, because the hook only shadows a boilerplate page whose `website_id` matches the custom page's. Both pages and the second website are removed again at the end of the test.

### Index view [@ANCHOR: COMM_test_compliance_index_view]
`test_compliance_index_view` in `tests/test_pages.py`: builds the combined arch of `compliance.compliance_index_template` and asserts the title "Regulatory Compliance & Policies" is present, so the index template cannot silently lose its heading. It reads the combined arch of the view (the module's template plus any inheriting site module) and makes no HTTP request; the route itself is covered by `test_compliance_index_route_lists_only_active_documents`, which also asserts an inactive `compliance.document` is not listed, and `test_index_signed_in.py`.

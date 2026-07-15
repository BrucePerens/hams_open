# Story: Automatic Legal Pages Generation [@ANCHOR: COMM_story_automatic_legal_pages]

## User Persona
**Alice**, a small business owner who just launched her first Odoo website.

## Scenario
Alice is worried about legal compliance but doesn't have the budget for a lawyer to draft custom privacy policies.

## Story
1. Alice installs the **Global Compliance** module.
2. Immediately upon installation, the module detects that she doesn't have a `/privacy` or `/terms` page.
3. The module automatically creates professional boilerplate pages for:
   - **Privacy Policy** (`/privacy`) [@ANCHOR: COMM_compliance_privacy_policy_template]

   - **Cookie Policy** (`/cookie-policy`) [@ANCHOR: COMM_compliance_cookie_policy_template]

   - **Terms of Service** (`/terms`) [@ANCHOR: COMM_compliance_terms_of_service_template]

   - **Accessibility Statement** (`/accessibility`) [@ANCHOR: COMM_compliance_accessibility_statement_template]
   
   - **Compliance Index** (`/compliance`) [@ANCHOR: COMM_compliance_index_route]
4. Alice visits her website and sees these links already active and populated with relevant content that covers her use of Odoo features.
5. She notices that links to these legal pages are automatically added to the footer of every page on her website, ensuring she meets visibility requirements. [@ANCHOR: COMM_compliance_footer_links]

6. If she already has a custom page, the module detects it and unpublishes its own boilerplate to avoid duplication. [@ANCHOR: COMM_test_compliance_non_destructive_mandate]
7. She can now focus on her business, knowing she has basic legal coverage.

## Verification
- Verified by [@ANCHOR: COMM_test_compliance_pages_presence]

- Verified by [@ANCHOR: COMM_test_compliance_pages_content]

- Verified by [@ANCHOR: COMM_test_compliance_ui_tour]

- Verified by [@ANCHOR: COMM_test_compliance_non_destructive_mandate]

- Verified by [@ANCHOR: COMM_test_compliance_index_view]

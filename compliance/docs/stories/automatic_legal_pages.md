# Story: Automatic Legal Pages Generation [@ANCHOR: story_automatic_legal_pages]

## User Persona
**Alice**, a small business owner who just launched her first Odoo website.

## Scenario
Alice is worried about legal compliance but doesn't have the budget for a lawyer to draft custom privacy policies.

## Story
1. Alice installs the **Global Compliance** module.
2. Immediately upon installation, the module detects that she doesn't have a `/privacy` or `/terms` page.
3. The module automatically creates professional boilerplate pages for:
   - **Privacy Policy** (`/privacy`) [@ANCHOR: compliance_privacy_policy_template]

   - **Cookie Policy** (`/cookie-policy`) [@ANCHOR: compliance_cookie_policy_template]

   - **Terms of Service** (`/terms`) [@ANCHOR: compliance_terms_of_service_template]

   - **Accessibility Statement** (`/accessibility`) [@ANCHOR: compliance_accessibility_statement_template]
4. Alice visits her website and sees these links already active and populated with relevant content that covers her use of Odoo features.
5. She notices that links to these legal pages are automatically added to the footer of every page on her website, ensuring she meets visibility requirements. [@ANCHOR: compliance_footer_links]

7. If she already has a custom page, the module detects it and unpublishes its own boilerplate to avoid duplication. This process is optimized using a high-performance Postgres procedure. [@ANCHOR: compliance_postgres_procedures]
6. She can now focus on her business, knowing she has basic legal coverage.

## Verification
- Verified by [@ANCHOR: test_compliance_pages_presence]

- Verified by [@ANCHOR: test_compliance_pages_content]

- Verified by [@ANCHOR: test_compliance_ui_tour]

- Verified by [@ANCHOR: test_compliance_non_destructive_mandate]

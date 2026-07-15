# Individual SEO Control Story

## Scenario
Alice, a researcher, wants to make her personal blog on the organization's platform more discoverable by search engines. She has specific keywords and a catchy title in mind.

## Story
Alice navigates to her blog page at `/alice-research/blog`. Because she is the owner of this blog, she sees the "Optimize SEO" option in the "Promote" menu of the Odoo frontend.

When she opens the SEO widget, she can see that her current blog title is being used as the default Meta Title. She decides to change it to "Alice's Breakthroughs in Quantum Computing" and adds a meta description that highlights her recent publications.

When she clicks "Save", the system validates that she is indeed the user she claims to be. Even though she doesn't have administrative permissions to the entire database, the `user_websites_seo` module allows her to write to these specific SEO fields on her own user record using an elevated service account context.

Alice's changes are saved immediately, and she can verify that the `<title>` tag on her blog index now reflects her new custom title.

## Technical Anchors
- Frontend SEO Widget Visibility: `[ANCHOR: controller_user_blog_index_seo_override]`

- Secure Permission Elevation: `[ANCHOR: res_users_seo_write_elevation]`

- Functional Verification: `[ANCHOR: test_seo_widget_tour]`

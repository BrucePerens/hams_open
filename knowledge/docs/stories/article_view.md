# Story: Viewing Manual Articles
[@ANCHOR: story_article_view]

This story describes how users access and view articles in the Knowledge.

## Scenario
A user visits the `/manual` URL to read documentation.

## Process
1. The user requests an article via its slug or visits the root `/manual` path.
2. The `manual_article_view` controller `[@ANCHOR: controller_manual_article_view]` handles the request.
3. If a slug is provided, the controller extracts the ID and fetches the article.
4. If no slug is provided, it defaults to the first available root article.
5. The controller enforces strict access control by checking the user's permissions on the article record.
6. The article content and sidebar navigation are rendered using the QWeb template.

## Technical Details
- Controller: `ManualLibraryController.manual_article_view`
- Access Control: Native Odoo ORM `check_access('read')` and record rules.
- Verification: `[@ANCHOR: test_manual_templates_rendering]`

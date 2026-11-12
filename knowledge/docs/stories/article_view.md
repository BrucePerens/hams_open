# Story: Viewing Manual Articles

## Summary [@ANCHOR: story_article_view]

This story is how a visitor reads a manual article at `/manual`. `manual_article_view` takes an optional slug, reads the article id from the number before the first hyphen, and answers 404 for an article that does not exist, is archived, or that the visitor's own record rules hide. With no slug it renders the Help index instead. A stale slug redirects to the current canonical one. The page is rendered with the sidebar and breadcrumbs, and the story is verified by the template rendering test below.

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
### Verification [@ANCHOR: test_manual_templates_rendering]

`tests/test_controllers.py` `test_06_manual_templates_rendering` requests `/manual` as a public visitor and checks the page renders, and `tests/test_manual_redesign_t21.py` `test_01_one_title_sticky_columns_and_initial_icons` reads the template source and checks it has one page title, a sticky sidebar that follows the site header height, the initial-letter icons and the accent border. Both run in the `knowledge` module test suite and need no browser.

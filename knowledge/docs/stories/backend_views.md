# Story: Backend Management Views

## Summary [@ANCHOR: story_manual_backend_views]

This story is the backend side of the Manuals app: the list and form views of `knowledge.article` that a Manual Administrator uses to create and organise articles. The list shows each article with its sequence, parent and publication status; the form edits the title, body, parent, `internal_permission` and shared members. Only members of the Manual Administrator group may create, change or delete, and the views are verified by compiling them in a test.

This story describes the backend user interface for managing articles.

## Scenario
A documentation manager opens the Articles menu in the Odoo backend.

## Process
1. The user navigates to the Knowledge app.
2. The list view shows all articles with their sequence and publication status.
3. The user opens an article form view to edit the title, body, or hierarchy.
4. The form view allows managing `internal_permission` and `member_ids` for fine-grained access control.

## Technical Details
- Views: `view_knowledge_article_list`, `view_knowledge_article_form`
### Verification [@ANCHOR: test_manual_backend_views_rendering]

`tests/test_robustness.py` `test_04_backend_views_rendering` asks the ORM for the compiled arch of `view_knowledge_article_list` (list) and `view_knowledge_article_form` (form) and checks that the list carries `parent_id` and the form carries `internal_permission`. A view that no longer compiles, or that lost those fields, fails here instead of at the first administrator who opens the Manuals menu. It needs a database but no browser.

# Story: Backend Management Views
[@ANCHOR: story_manual_backend_views]

This story describes the backend user interface for managing articles.

## Scenario
A documentation manager opens the Articles menu in the Odoo backend.

## Process
1. The user navigates to the Knowledge app.
2. The list view `[@ANCHOR: test_manual_backend_views_rendering]` shows all articles with their sequence and publication status.
3. The user opens an article form view to edit the title, body, or hierarchy.
4. The form view allows managing `internal_permission` and `member_ids` for fine-grained access control.

## Technical Details
- Views: `view_knowledge_article_list`, `view_knowledge_article_form`
- Verification: `[@ANCHOR: test_manual_backend_views_rendering]`

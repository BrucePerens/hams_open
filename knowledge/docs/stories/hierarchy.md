# Story: Article Hierarchy Integrity

## Summary [@ANCHOR: story_manual_hierarchy]

This story is the guarantee that the article tree stays a tree. An administrator who sets an article's parent to one of its own descendants is stopped by the `_check_hierarchy` constraint with a validation error, and the write is rolled back, so the sidebar and the breadcrumb query can never loop. Breadcrumb computation additionally stops at a depth of 50 as a second guard.

This story describes how the system ensures a valid tree structure for articles.

## Scenario
An administrator attempts to set an article's parent to one of its own descendants.

## Process
1. The user attempts to update the `parent_id` of an article.
2. The `_check_hierarchy` constraint is triggered: an `api.constrains("parent_id")` method that calls the ORM's `_has_cycle()` and raises `ValidationError` ("You cannot create recursive articles.") when the article would become its own ancestor. It takes no arguments and writes nothing, so a refused change leaves no trace. `[@ANCHOR: manual_check_hierarchy]`
3. The system checks for circular references in the article tree.
4. If a cycle is detected, a `ValidationError` is raised, and the change is blocked.

## Technical Details
- Model: `knowledge.article`
- Method: `_check_hierarchy`
### Verification [@ANCHOR: test_manual_check_hierarchy]

`tests/test_orm_logic.py` `test_01_prevent_circular_hierarchy` builds the chain A, then B under A, then C under B, tries to write C as the parent of A, and requires a `ValidationError` (the flush inside the block forces the constraint to run). It also covers breadcrumbs, because the same chain is what the breadcrumb query walks. Database only, no browser.

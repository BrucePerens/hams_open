# Story: Article Hierarchy Integrity
[@ANCHOR: story_manual_hierarchy]

This story describes how the system ensures a valid tree structure for articles.

## Scenario
An administrator attempts to set an article's parent to one of its own descendants.

## Process
1. The user attempts to update the `parent_id` of an article.
2. The `_check_hierarchy` constraint `[@ANCHOR: manual_check_hierarchy]` is triggered.
3. The system checks for circular references in the article tree.
4. If a cycle is detected, a `ValidationError` is raised, and the change is blocked.

## Technical Details
- Model: `knowledge.article`
- Method: `_check_hierarchy`
- Verification: `[@ANCHOR: test_manual_check_hierarchy]`

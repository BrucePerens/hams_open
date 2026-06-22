# Story: Dynamic URL Generation
[@ANCHOR: story_manual_url_generation]

This story describes how SEO-friendly URLs (slugs) are generated for articles.

## Scenario
A new article is created, or an existing article is renamed.

## Process
1. The `name` field of an article is modified.
2. The `_compute_website_url` method `[@ANCHOR: manual_compute_website_url]` is triggered.
3. The method normalizes the article name (lowercasing, removing special characters).
4. A slug is generated in the format `/manual/<id>-<safe-name>`.
5. This URL is used for all public links to the article.

## Technical Details
- Model: `knowledge.article`
- Method: `_compute_website_url`
- Verification: `[@ANCHOR: test_manual_url_slug_generation]`

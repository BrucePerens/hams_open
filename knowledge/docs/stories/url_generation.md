# Story: Dynamic URL Generation

## Summary [@ANCHOR: story_manual_url_generation]

This story is how an article gets its readable address. Whenever the name changes, `_compute_website_url` rebuilds the URL as `/manual/<id>-<safe-name>`: lower case, accents folded to ASCII, punctuation collapsed to single hyphens. The id comes first, so the URL is unique even when two articles share a name, and an old slug of a renamed article still finds it because only the id is read.

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
### Verification [@ANCHOR: test_manual_url_slug_generation]

`tests/test_orm_logic.py` `test_02_url_slug_generation` creates an article named "API Documentation v2.0 (Alpha)!" and requires its `website_url` to be exactly `/manual/<id>-api-documentation-v2-0-alpha`: lower case, punctuation collapsed to single hyphens, no trailing hyphen, the id first so the URL is unique. Database only, no browser.

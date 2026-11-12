# Journey: Administrator Managing Articles

## Summary [@ANCHOR: journey_admin_managing]

This journey follows a Manual Administrator organising documentation in the backend: creating an article, placing it under a parent (a cycle is refused), checking its generated address, publishing it so it appears at `/manual`, and reading the helpful and unhelpful counts to judge its quality. Only members of the Manual Administrator group can do the writing steps.

This journey follows an administrator organizing documentation in the backend.

## Personas
- **Documentation Admin**: A user with full access to manage articles, i.e. a member of the Manual Administrator group (see "Security and Access Rights" in [the module README](../../README.md)).

## Steps
1. **Creation**: The admin creates a new `knowledge.article` in the backend.
2. **Organization**: The admin sets a `parent_id` for the article to place it in the hierarchy.
   - *Related Story:* `hierarchy.md`
   - *Entry:* `manual_check_hierarchy` below
3. **URL Verification**: The admin checks the generated SEO-friendly URL to ensure it's correct.
   - *Related Story:* `url_generation.md`
   - *Entry:* `manual_compute_website_url` below
4. **Publishing**: The admin sets the article to `is_published=True` to make it visible on the website.
5. **Monitoring**: The admin reviews the `helpful_count` and `unhelpful_count` to gauge content quality.
   - *Related Story:* `feedback.md`

## Entries

### `knowledge.article._check_hierarchy(self)` [@ANCHOR: manual_check_hierarchy]

An `api.constrains("parent_id")` check that calls `_has_cycle()` and raises `ValidationError` ("You cannot create recursive articles.") when an article would be its own ancestor, which aborts and rolls back the write. No arguments, no return value and no writes. It keeps the article tree a tree, so the breadcrumb and sidebar queries cannot loop. Tests: `tests/test_orm_logic.py` `test_01_prevent_circular_hierarchy`.

### `knowledge.article._compute_website_url(self)` [@ANCHOR: manual_compute_website_url]

Computed field method (depends on `name`) that overrides the website mixin's. It calls the mixin first, then for a saved article builds `/manual/<id>-<safe-name>`: the name lower-cased, accents folded to ASCII (characters with no ASCII form are dropped), each run of non-alphanumerics replaced by one hyphen, hyphens trimmed from the ends. An unsaved record gets an empty URL. No I/O. The id comes first so URLs stay unique and an old slug still resolves. Tests: `tests/test_orm_logic.py` `test_02_url_slug_generation`.

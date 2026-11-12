# Journey: User Browsing the Manual

## Summary [@ANCHOR: journey_user_browsing]

This journey follows a guest or portal user reading the manual: arriving at the Help index, searching for a topic, opening a result or a sidebar item, using the table of contents to jump between sections of a long article, and finally voting whether the article helped. Each step is a separate story and anchor, and only published member manuals are visible to them.

This journey follows a user as they interact with the Knowledge to find and read documentation.

## Personas
- **Guest**: An unauthenticated user visiting the website.
- **Portal User**: A registered user with basic access.

## Steps
1. **Discovery**: The user arrives at `/manual` and sees a list of available root articles.
   - *Related Story:* `article_view.md`
2. **Search**: The user uses the search bar to find a specific topic.
   - *Related Story:* `search.md`
   - *Entry:* `controller_manual_search` below
3. **Navigation**: The user clicks on a search result or an item in the sidebar to view an article.
   - *Related Story:* `article_view.md`
   - *Entry:* `controller_manual_article_view` below
4. **Reading**: The user reads the article, using the dynamic Table of Contents to jump between sections.
   - *Related Story:* `toc.md`
   - *Entry:* `manual_toc_logic` below
5. **Feedback**: After reading, the user indicates whether the article was helpful.
   - *Related Story:* `feedback.md`
   - *Entry:* `controller_manual_feedback` below

## Entries

### `ManualLibraryController.manual_search(self, search="")` [@ANCHOR: controller_manual_search]

Route `/manual/search` (public, website). Argument `search`: the text typed in the search bar. The domain is the Help domain (published member manuals and articles shared with the reader), plus a case-insensitive substring match on name or body when a term is given. At most 1000 articles are returned, rendered with `knowledge.search_results_template` and the sidebar context. It never raises for a term; record rules apply. Not a ranked full-text index. Tests: `static/tests/tours/manual_search_tour.js` via `tests/test_ui_tours.py`.

### `ManualLibraryController.manual_article_view(self, article_slug=None)` [@ANCHOR: controller_manual_article_view]

Routes `/manual` and `/manual/<slug>` (public, website). Argument `article_slug`: optional `<id>-<name>`; the integer before the first hyphen is the id. A non-number, a missing or archived article, or a failed `check_access("read")` under the visitor's own user gives 404; with no slug it renders the Help index. `_can_open_article` then refuses internal documentation (404). A slug that differs from the article's current `website_url` is answered with a 301 to it. Otherwise the body goes through `_compile_markdown` and `knowledge.article_template` is rendered with the sidebar context. Read-only. Tests: `tests/test_controllers.py`.

### `ManualTOC` widget start [@ANCHOR: manual_toc_logic]

In `static/src/js/manual_toc.js`, after the page loads the widget finds `#manual_toc_container` (it stops if there is none), collects the `h2` and `h3` headings of the article body (it stops if there are none), clears the container, adds an "On this page" header and a `nav` list. Each heading gets an id (its own, or `toc-heading-<n>`) and a link to it, with `h2` entries bold and `h3` entries indented. Browser-only DOM code: no server call, no storage. Tests: `manual_toc_tour` in `tests/test_ui_tours.py`.

### `ManualLibraryController.manual_feedback(self, article_id, is_helpful, website_feedback_honeypot=None)` [@ANCHOR: controller_manual_feedback]

POST route `/manual/feedback` (public, website). Arguments: `article_id`, `is_helpful` (`"1"` helpful, anything else unhelpful) and a honeypot field that must be empty. The redirect target is the referer's path only (never another site). A filled honeypot or a repeat vote in the same session just redirects. Otherwise it checks `check_access("read")`, then as the Knowledge service account runs one `UPDATE` that adds 1 to `helpful_count` or `unhelpful_count`, invalidates the cache and marks the session. Bad ids or access errors are logged and swallowed. Tests: `manual_feedback_tour`, `tests/test_features.py`.

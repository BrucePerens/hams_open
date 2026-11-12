# Story: Help Shows Only User Manuals

## Summary [@ANCHOR: story_help_member_only]

This story keeps developer documentation away from members. Each article has an audience, `member` for a user manual or `internal` for administrators only. Help, search, the sidebar and by-name links show published member manuals and articles shared with the reader; a direct link to an internal article answers 404 to everyone except a Manual Administrator, who reaches the internal documents in a separate administrators-only view. The entries below give each function that does this.

This story describes how the Help pages keep developer documentation away from members.

## Scenario
A member clicks Help. They expect how-to guides and user manuals for the features they use: the console, relay setup, getting verified, the logbook, forums, classifieds. They must not meet stories, journeys, feature lists, "External Dependencies", architecture notes or module reference, and an administrator opening Help sees the same thing.

## Process
1. Every article has an `audience`: `member` (a user manual) or `internal` (administrators only).
2. The documentation loader in zero_sudo classifies each `knowledge_docs` manifest entry. A document is a member manual only when its entry says `"public": True` (or `"audience": "member"`) and it is not in a stories, journeys or runbooks directory, not a README, and not titled "Story:" or "Journey:". Anything else is internal. The same classification is available as one call on the module model, so other code and tests ask the model instead of repeating the rules. A member manual also names its `help_topic`, the heading the Help index files it under.
3. Help and the help search use one domain: published member manuals, plus articles shared with the reader by name.
4. `/manual` and `/knowledge/home` with no search term show the Help index: the manuals by topic and a search box. They no longer open whichever article was installed first.
5. Search, `/manual/by_name/...`, the site-wide search and the sidebar all use that domain.
6. A direct URL to an internal article answers 404 to everyone except a Manual Administrator.
7. Who counts as a Manual Administrator is one check, membership of `knowledge.group_manual_manager`; it decides the sidebar, the direct-URL rule below and the administrators-only view.
8. The Help index groups the manuals by help topic: the topics in the fixed reader-friendly order (Getting started and your account, Operating your station, Community, Training and testing, Your website), then any other topic alphabetically, then "Other guides" for a manual with no topic. Inside a topic a lower `sequence` leads and the rest are alphabetical.
9. A reader may open an article by its URL when they are a Manual Administrator, when it was shared with them by name, or when it is a member manual that is published (an internal user may open an unpublished member manual too). Nothing else opens.
10. The sidebar is built from the same domain: the articles the reader may read, the ones shared with them, their private ones, and whether they are an internal user or a Manual Administrator.
11. A Manual Administrator reaches internal documentation at `/manual/internal`, labelled administrators-only, and sees a banner on each internal page. Help itself never lists it. That view lists exactly what Help hides: every article whose audience is not member, and every member manual that is not yet published.

## Technical Details
- Controller: `ManualLibraryController.manual_article_view`, `manual_search`, `manual_internal_index`, `manual_article_by_name`
- Verification: `knowledge/tests/test_help_audience.py`, `knowledge/tests/test_doc_audience_installer.py`

## Entries

### `knowledge.article.audience` [@ANCHOR: knowledge:article_audience]

A selection field on `knowledge.article`, `member` (a user manual) or `internal` (administrators only), default `member`. Member articles are the ones Help lists, searches and opens for everyone; internal ones (stories, journeys, architecture and module reference, external dependencies, claims) appear in no Help page and no search, and a direct URL gives anyone but a Manual Administrator a 404. The documentation loader in `zero_sudo` always sets it explicitly and treats anything it cannot classify as internal; the default is `member` only so that an article an administrator writes and publishes by hand keeps working. Tests: `tests/test_help_audience.py`.

### `classify_doc_audience(path, doc_info) -> str` [@ANCHOR: zero_sudo:classify_doc_audience]

Classifies one `knowledge_docs` manifest entry as `member` or `internal`. Arguments: `path` (the file path in the entry; a missing one is internal) and `doc_info` (the entry dict: `name`, `public`, `audience`). Internal wins over any opt-in when the path has a `stories`, `journeys` or `runbooks` directory, the file is a README, or the name starts "Story:" or "Journey:". Otherwise an explicit `audience` of `member` or `"public": True` gives `member`; anything else, including an unknown audience word, is `internal`. Pure, no I/O, never raises. The markers beating the opt-in means a stray flag cannot expose a developer document. Tests: `tests/test_doc_audience_installer.py`.

### `ir.module.module._doc_audience_for(path, doc_info)` [@ANCHOR: zero_sudo:doc_audience_for]

The model-level door to `classify_doc_audience`, so other code and tests ask the model instead of repeating the rules. Arguments and return are the same as the function: `path` and `doc_info` in, `member` or `internal` out. It is an `api.model` method with no side effects, reads nothing from the database and cannot fail on a well-formed entry. The loader's own install step calls the function directly; the Knowledge migration and the tests call it through here. Tests: `tests/test_doc_audience_installer.py`.

### `ManualLibraryController._help_domain(self) -> list` [@ANCHOR: knowledge:help_audience_domain]

The search domain for what Help may list, search and open. No arguments; it reads the current website and user. Returns a domain requiring `website_id` to be empty or the current website, and either (published and audience `member`) or the user being in `member_ids`. Internal documentation never matches, whoever asks, a Manual Administrator included: administrators use the separate internal view. It is applied to the Help index, `manual_search`, by-name lookups and the sidebar, so all of them agree. Tests: `tests/test_help_audience.py`.

### `ManualLibraryController._render_help_index(self)` [@ANCHOR: knowledge:help_index]

Renders the Help landing page that `/manual` and `/knowledge/home` show when no article or search term is given. It searches root articles (no parent) under the Help domain, at most 1000, groups them with `_help_topics`, and renders `knowledge.help_index_template` with the topics, an empty search term and the sidebar context. It replaces the old behaviour of opening whichever article was installed first. No writes. Record rules still apply to the search. Tests: `tests/test_help_audience.py`.

### `manual_search` and the Help domain [@ANCHOR: knowledge:help_search_member_only]

Inside `manual_search`, the search domain is taken from `_help_domain()` and only then narrowed by the term (`ilike` on name or body). So only member manuals, and articles shared with the reader, are ever searched, for every reader including administrators, and a search can never act as a way to find internal documentation. Arguments of the route: `search` (default empty). Returns the rendered search page with at most 1000 articles. Tests: `tests/test_help_audience.py`, `static/tests/tours/manual_search_tour.js`.

### `knowledge.article._search_get_detail(self, website, order, options)` [@ANCHOR: knowledge:COMM_search_get_detail]

Registers articles with Odoo's site-wide search (the website's own search does not cover them by default). Arguments: the `website`, the `order` and the `options` dict (`displayDescription` adds the body to the matched fields). Returns the dispatch dict for model `knowledge.article`: searching `name` (and `body`), fetching `name` and `website_url`, icon `fa-book`. The base domain is audience `member` for staff (drafts included), and for others published member manuals plus articles shared with them, always limited by the website domain. Internal documentation is never found here. Read-only. Tests: `tests/test_help_audience.py`.

### `ManualLibraryController.manual_article_view` internal check [@ANCHOR: knowledge:internal_article_url_404]

Step 4 of `manual_article_view`: after the article is found and the visitor's own environment has passed `check_access("read")`, `_can_open_article` must be true, otherwise the controller raises `NotFound` (404) rather than 403. Answering 404 means the existence of an internal article is not revealed to anyone but a Manual Administrator, and a guessed `/manual/<id>-slug` reveals nothing. Arguments of the route: `article_slug` (optional). Tests: `tests/test_help_audience.py`.

### `ManualLibraryController._is_manual_manager(self) -> bool` [@ANCHOR: knowledge:is_manual_manager]

True when the current user is in the group `knowledge.group_manual_manager` (Manual Administrator). No arguments; reads the request environment. It is the single definition used by the sidebar (which lets managers see every article), `_can_open_article` and the administrators-only view, so the rule cannot drift between them. The group is the only interactive one whose access rights allow create, write or delete. No side effects. Tests: `tests/test_help_audience.py`.

### `ManualLibraryController._help_topics(self, articles) -> list` [@ANCHOR: knowledge:help_topics]

Groups member manuals for the Help index. Argument `articles`: a recordset. Returns a list of `(topic, [articles])`. A manual with no `help_topic` goes under "Other guides". Topics in the fixed reader-friendly order (Getting started and your account, Operating your station, Community, Training and testing, Your website) come first, any other topic follows alphabetically (case-insensitive), and "Other guides" is last. Inside a topic a lower `sequence` leads and ties are alphabetical. Pure apart from reading fields. Tests: `tests/test_help_audience.py`.

### `ManualLibraryController._can_open_article(self, article) -> bool` [@ANCHOR: knowledge:can_open_article]

Whether the current reader may open an article by its URL. Argument `article`: a record. A Manual Administrator may open anything; a user listed in `member_ids` may open the article shared with them; otherwise an internal article is refused; a member manual is open to any internal (staff) user, even unpublished, and to everyone else only when published. Returns a bool and has no side effects. This is a second layer on top of the record rules, which already limit what `check_access` allows. Tests: `tests/test_help_audience.py`.

### `ManualLibraryController._sidebar_context(self) -> dict` [@ANCHOR: knowledge:sidebar_context]

The values every manual page needs for its sidebar. No arguments. It calls `_get_sidebar_articles` (root articles under the Help domain, then split into the workspace list, the articles shared with the reader, and the reader's own private ones, in one query) and returns `workspace_articles`, `shared_articles`, `private_articles`, `is_internal_user` (staff or portal) and `is_manual_manager`. A Manual Administrator sees every member manual in the main list. No writes. Tests: `tests/test_help_audience.py`.

### `ManualLibraryController.manual_internal_index(self, search="")` [@ANCHOR: knowledge:internal_index]

Route `/manual/internal` (public, website). Answers 404 to anyone who is not a Manual Administrator. For a manager it lists, sorted by name, up to 1000 articles matching `_internal_domain()`, optionally narrowed by `ilike` on name or body, and renders `knowledge.internal_index_template` labelled administrators-only with the sidebar. Each internal page shows a banner. Help itself never links it as a manual list. Read-only. Tests: `tests/test_help_audience.py`.

### `ManualLibraryController._internal_domain(self) -> list` [@ANCHOR: knowledge:help_internal_domain]

The domain for the administrators-only view: exactly what Help hides. No arguments. It requires the website to be empty or current and either audience other than `member` or `is_published` false, so it lists every internal article and every member manual not yet published, and never anything that Help shows. The two domains are complements, so every article appears in exactly one of the two places. Tests: `tests/test_help_audience.py`.

### `classify_existing_articles(cr) -> int` in `migrations/1.3/post-migration.py` [@ANCHOR: knowledge:classify_existing_articles_migration]

Classifies articles installed before `audience` existed, so the field default `member` does not expose old developer documents. Argument `cr`: the migration cursor. Each article the loader installed (rows `zero_sudo.doc_article_id_<module>_<name>` in `zero_sudo_kv`) is matched to its manifest entry and set to member (published, readable, with its help topic) or internal (unpublished, permission none, also when no manifest lists it any more). An unpublished hand-written article becomes internal; a published one stays member. Returns the number of rows changed. Plain SQL, deletes nothing, repeatable. Tests: `tests/test_doc_audience_installer.py`.

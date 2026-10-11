# Story: Help Shows Only User Manuals
[@ANCHOR: story_help_member_only]

This story describes how the Help pages keep developer documentation away from members.

## Scenario
A member clicks Help. They expect how-to guides and user manuals for the features they use: the console, relay setup, getting verified, the logbook, forums, classifieds. They must not meet stories, journeys, feature lists, "External Dependencies", architecture notes or module reference, and an administrator opening Help sees the same thing.

## Process
1. Every article has an `audience` `[@ANCHOR: knowledge:article_audience]`: `member` (a user manual) or `internal` (administrators only).
2. The documentation loader in zero_sudo classifies each `knowledge_docs` manifest entry `[@ANCHOR: zero_sudo:classify_doc_audience]`. A document is a member manual only when its entry says `"public": True` (or `"audience": "member"`) and it is not in a stories, journeys or runbooks directory, not a README, and not titled "Story:" or "Journey:". Anything else is internal. A member manual also names its `help_topic`, the heading the Help index files it under.
3. Help and the help search use one domain `[@ANCHOR: knowledge:help_audience_domain]`: published member manuals, plus articles shared with the reader by name.
4. `/manual` and `/knowledge/home` with no search term show the Help index `[@ANCHOR: knowledge:help_index]`: the manuals by topic and a search box. They no longer open whichever article was installed first.
5. Search `[@ANCHOR: knowledge:help_search_member_only]`, `/manual/by_name/...`, the site-wide search `[@ANCHOR: knowledge:COMM_search_get_detail]` and the sidebar all use that domain.
6. A direct URL to an internal article answers 404 to everyone except a Manual Administrator `[@ANCHOR: knowledge:internal_article_url_404]`.
7. A Manual Administrator reaches internal documentation at `/manual/internal` `[@ANCHOR: knowledge:internal_index]`, labelled administrators-only, and sees a banner on each internal page. Help itself never lists it.

## Existing databases
`knowledge/migrations/1.3/post-migration.py` `[@ANCHOR: knowledge:classify_existing_articles_migration]` classifies the articles installed before the field existed, from the same classifier. It deletes nothing and can be run again.

## Technical Details
- Controller: `ManualLibraryController.manual_article_view`, `manual_search`, `manual_internal_index`, `manual_article_by_name`
- Verification: `knowledge/tests/test_help_audience.py`, `knowledge/tests/test_doc_audience_installer.py`

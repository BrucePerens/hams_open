# Story: Help Shows Only User Manuals
[@ANCHOR: story_help_member_only]

This story describes how the Help pages keep developer documentation away from members.

## Scenario
A member clicks Help. They expect how-to guides and user manuals for the features they use: the console, relay setup, getting verified, the logbook, forums, classifieds. They must not meet stories, journeys, feature lists, "External Dependencies", architecture notes or module reference, and an administrator opening Help sees the same thing.

## Process
1. Every article has an `audience` `[@ANCHOR: knowledge:article_audience]`: `member` (a user manual) or `internal` (administrators only).
2. The documentation loader in zero_sudo classifies each `knowledge_docs` manifest entry `[@ANCHOR: zero_sudo:classify_doc_audience]`. A document is a member manual only when its entry says `"public": True` (or `"audience": "member"`) and it is not in a stories, journeys or runbooks directory, not a README, and not titled "Story:" or "Journey:". Anything else is internal. The same classification is available as one call on the module model `[@ANCHOR: zero_sudo:doc_audience_for]`, so other code and tests ask the model instead of repeating the rules. A member manual also names its `help_topic`, the heading the Help index files it under.
3. Help and the help search use one domain `[@ANCHOR: knowledge:help_audience_domain]`: published member manuals, plus articles shared with the reader by name.
4. `/manual` and `/knowledge/home` with no search term show the Help index `[@ANCHOR: knowledge:help_index]`: the manuals by topic and a search box. They no longer open whichever article was installed first.
5. Search `[@ANCHOR: knowledge:help_search_member_only]`, `/manual/by_name/...`, the site-wide search `[@ANCHOR: knowledge:COMM_search_get_detail]` and the sidebar all use that domain.
6. A direct URL to an internal article answers 404 to everyone except a Manual Administrator `[@ANCHOR: knowledge:internal_article_url_404]`.
7. Who counts as a Manual Administrator is one check, membership of `knowledge.group_manual_manager` `[@ANCHOR: knowledge:is_manual_manager]`; it decides the sidebar, the direct-URL rule below and the administrators-only view.
8. The Help index groups the manuals by help topic `[@ANCHOR: knowledge:help_topics]`: the topics in the fixed reader-friendly order (Getting started and your account, Operating your station, Community, Training and testing, Your website), then any other topic alphabetically, then "Other guides" for a manual with no topic. Inside a topic a lower `sequence` leads and the rest are alphabetical.
9. A reader may open an article by its URL `[@ANCHOR: knowledge:can_open_article]` when they are a Manual Administrator, when it was shared with them by name, or when it is a member manual that is published (an internal user may open an unpublished member manual too). Nothing else opens.
10. The sidebar `[@ANCHOR: knowledge:sidebar_context]` is built from the same domain: the articles the reader may read, the ones shared with them, their private ones, and whether they are an internal user or a Manual Administrator.
11. A Manual Administrator reaches internal documentation at `/manual/internal` `[@ANCHOR: knowledge:internal_index]`, labelled administrators-only, and sees a banner on each internal page. Help itself never lists it. That view lists exactly what Help hides `[@ANCHOR: knowledge:help_internal_domain]`: every article whose audience is not member, and every member manual that is not yet published.

## Existing databases
`knowledge/migrations/1.3/post-migration.py` `[@ANCHOR: knowledge:classify_existing_articles_migration]` classifies the articles installed before the field existed, from the same classifier. It deletes nothing and can be run again.

## Technical Details
- Controller: `ManualLibraryController.manual_article_view`, `manual_search`, `manual_internal_index`, `manual_article_by_name`
- Verification: `knowledge/tests/test_help_audience.py`, `knowledge/tests/test_doc_audience_installer.py`

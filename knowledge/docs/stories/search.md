# Story: Searching the Manual

## Summary [@ANCHOR: story_manual_search]

This story is searching the manual from the search bar. `manual_search` matches the term, case-insensitively and as a substring, against article titles and bodies, but only among the articles the Help domain allows: published member manuals and those shared with the reader. It returns up to 1000 results on the search page with the usual sidebar. It is not a ranked full-text index.

This story describes the full-text search capability within the Knowledge.

## Scenario
A user wants to find information about a specific topic but doesn't know where it is in the hierarchy.

## Process
1. The user enters a search term in the search bar.
2. The `manual_search` controller (route `/manual/search`, public, website) receives the query. It builds the Help domain, so only published member manuals and articles shared with the reader are searched, for every reader including administrators; the result is limited to 1000 articles and rendered with `knowledge.search_results_template` and the sidebar context. `[@ANCHOR: controller_manual_search]`
3. The controller performs a full-text search on the `name` and `body` fields of accessible articles. In practice this is a case-insensitive substring match (`ilike`), not a ranked full-text index (see "Search Engine" in [the module README](../../README.md)).
4. Native record rules ensure that only articles the user has permission to see are returned.
5. Multi-website isolation ensures that results are restricted to the current website context.
6. The search results are displayed to the user, highlighting matches.

## Technical Details
- Controller: `ManualLibraryController.manual_search`
### Verification [@ANCHOR: test_tour_manual_search]

`tests/test_ui_tours.py` `test_02_manual_search_tour` creates a published article named "Odoo Search Test" and runs `manual_search_tour` (`static/tests/tours/manual_search_tour.js`): it types "Odoo" in the search box on `/manual`, submits the form, waits for the page to reload and requires the text "Search Results for:" on the result page. It checks the route, the form and the template together and needs a browser.

# Story: Searching the Manual
[@ANCHOR: story_manual_search]

This story describes the full-text search capability within the Manual Library.

## Scenario
A user wants to find information about a specific topic but doesn't know where it is in the hierarchy.

## Process
1. The user enters a search term in the search bar.
2. The `manual_search` controller `[@ANCHOR: controller_manual_search]` receives the query.
3. The controller performs a full-text search on the `name` and `body` fields of accessible articles.
4. Native record rules ensure that only articles the user has permission to see are returned.
5. Multi-website isolation ensures that results are restricted to the current website context.
6. The search results are displayed to the user, highlighting matches.

## Technical Details
- Controller: `ManualLibraryController.manual_search`
- Verification: `[@ANCHOR: test_tour_manual_search]`

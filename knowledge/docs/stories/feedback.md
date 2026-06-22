# Story: Article Feedback
[@ANCHOR: story_manual_feedback]

This story describes how users provide feedback on the helpfulness of articles.

## Scenario
A user finishes reading an article and wants to indicate whether it was helpful or not.

## Process
1. The user clicks the "Helpful" or "Not Helpful" button at the bottom of an article.
2. The `manual_feedback` controller `[@ANCHOR: controller_manual_feedback]` receives the POST request.
3. An anti-spam honeypot check is performed to prevent automated submissions.
4. The controller verifies that the user has read access to the article.
5. If valid, the `helpful_count` or `unhelpful_count` is incremented in the database using an atomic SQL update to prevent race conditions.
6. The user is redirected back to the article with a success message.

## Technical Details
- Controller: `ManualLibraryController.manual_feedback`
- Concurrency: Atomic SQL updates.
- Verification: `[@ANCHOR: test_tour_manual_feedback]`

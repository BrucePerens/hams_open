# Story: Article Feedback

## Summary [@ANCHOR: story_manual_feedback]

This story is how a reader says an article helped or did not. The `manual_feedback` controller refuses a filled honeypot field and a second vote from the same browser session, checks the reader can read the article, and then raises `helpful_count` or `unhelpful_count` with one atomic SQL update run as the Knowledge service account, so concurrent votes cannot overwrite each other. The reader is redirected back with a thank-you message.

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
### Verification [@ANCHOR: test_tour_manual_feedback]

`tests/test_ui_tours.py` `test_03_manual_feedback_tour` creates a published article and runs the `manual_feedback_tour` browser tour on it (`static/tests/tours/manual_feedback_tour.js`): it clicks the "Yes, it helped" button (`is_helpful=1`), waits for the page to reload and checks that a success alert saying "Thank you for your feedback!" appears. The counter increment itself is checked in `tests/test_features.py`, which reads `helpful_count` and `unhelpful_count` back after a POST. The tour needs a browser.

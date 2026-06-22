# Journey: User Browsing the Manual
[@ANCHOR: journey_user_browsing]

This journey follows a user as they interact with the Knowledge to find and read documentation.

## Personas
- **Guest**: An unauthenticated user visiting the website.
- **Portal User**: A registered user with basic access.

## Steps
1. **Discovery**: The user arrives at `/manual` and sees a list of available root articles.
   - *Related Story:* `article_view.md`
2. **Search**: The user uses the search bar to find a specific topic.
   - *Related Story:* `search.md`
   - *Anchor:* `[@ANCHOR: controller_manual_search]`
3. **Navigation**: The user clicks on a search result or an item in the sidebar to view an article.
   - *Related Story:* `article_view.md`
   - *Anchor:* `[@ANCHOR: controller_manual_article_view]`
4. **Reading**: The user reads the article, using the dynamic Table of Contents to jump between sections.
   - *Related Story:* `toc.md`
   - *Anchor:* `[@ANCHOR: manual_toc_logic]`
5. **Feedback**: After reading, the user indicates whether the article was helpful.
   - *Related Story:* `feedback.md`
   - *Anchor:* `[@ANCHOR: controller_manual_feedback]`

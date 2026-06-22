# Journey: Administrator Managing Articles
[@ANCHOR: journey_admin_managing]

This journey follows an administrator organizing documentation in the backend.

## Personas
- **Documentation Admin**: A user with full access to manage articles.

## Steps
1. **Creation**: The admin creates a new `knowledge.article` in the backend.
2. **Organization**: The admin sets a `parent_id` for the article to place it in the hierarchy.
   - *Related Story:* `hierarchy.md`
   - *Anchor:* `[@ANCHOR: manual_check_hierarchy]`
3. **URL Verification**: The admin checks the generated SEO-friendly URL to ensure it's correct.
   - *Related Story:* `url_generation.md`
   - *Anchor:* `[@ANCHOR: manual_compute_website_url]`
4. **Publishing**: The admin sets the article to `is_published=True` to make it visible on the website.
5. **Monitoring**: The admin reviews the `helpful_count` and `unhelpful_count` to gauge content quality.
   - *Related Story:* `feedback.md`

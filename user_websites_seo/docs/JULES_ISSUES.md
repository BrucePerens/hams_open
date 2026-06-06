# JULES ISSUES - user_websites_seo

## Architectural Hurdles & Required Fallbacks

### 1. user_websites Field Whitelisting
The base `user_websites` module implements strict security overrides on `write()` for `website.page` and `blog.post`. These overrides explicitly strip any fields not present in a hardcoded whitelist.

This prevents `user_websites_seo` from writing SEO metadata to these models out-of-the-box. During development and testing, I had to locally modify `user_websites/models/website_page.py` and `user_websites/models/blog_post.py` to include SEO fields in their whitelists.

**Action Required:** The maintainer of `user_websites` should update the `allowed` field sets in `website.page.write()`, `website.page.create()`, `blog.post.write()`, and `blog.post.create()` to include:
- `website_meta_title`
- `website_meta_description`
- `website_meta_keywords`
- `website_meta_og_img`
- `seo_name`

I have backed out these changes from my PR to maintain module isolation, which means the new tests for page/post SEO will fail in a clean environment until `user_websites` is updated.

### 2. website.page view_id delegation
`website.page` delegates its SEO fields to the linked `ir.ui.view` (`view_id`). Writes to these fields on the page record are automatically proxied to the view. This delegation is handled by Odoo's core `website` module. My implementation of `user.websites.seo.metadata.mixin` correctly handles this by allowing the write to proceed under the service account, which has the necessary rights to modify `ir.ui.view` records.

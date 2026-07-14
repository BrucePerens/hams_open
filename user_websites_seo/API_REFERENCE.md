# User Websites SEO (`user_websites_seo`) - API Reference

## Purpose
A lightweight extension that fuses Odoo's native `website.seo.metadata` mixin into the `res.users` and `user.websites.group` models, enabling users to optimize their personal websites and blogs for search engines.

## API & Usage
This module contains no custom callable Python APIs.

### Architectural Modifications
* **Self-Writable Fields:** Appends `website_meta_title`, `website_meta_description`, `website_meta_keywords`, `website_meta_og_img`, and `seo_name` to the user's `SELF_WRITEABLE_FIELDS` property (ADR-0015). This allows standard users to save SEO data without requiring backend Administrator rights. Verified by `[@ANCHOR: res_users_self_writeable_fields]`.

* **Controller Interception:** Overrides the `/blog` route to inject the SEO-aware profile object into the QWeb context, seamlessly activating Odoo's interactive "Optimize SEO" UI widget for the profile owner. Verified by `[@ANCHOR: controller_user_blog_index_seo_override]`.

# Group SEO Collaboration Story

## Scenario
The "Community Outreach" group has a shared blog where several members contribute. Bob, who handles communications for the group, wants to update the SEO metadata for the group's blog index.

## Story
Bob logs in and goes to `/community-outreach/blog`. As a member of the "Community Outreach" group, he sees the "Optimize SEO" widget.

He updates the Meta Keywords to include "community", "volunteering", and "outreach". He also sets a specific Open Graph image that represents the group's latest event.

The `user_websites_seo` module checks Bob's membership in the group. Since he is a member, it allows the write operation to the `user.websites.group` record for the SEO fields. The operation is carried out securely via the `user_websites` service account.

The entire group benefits from Bob's SEO optimization, as the group's blog index now presents better on social media shares and search engine results.

## Technical Anchors
- Controller Injection for Groups: `[ANCHOR: controller_user_blog_index_seo_override]`

- Group Permission Elevation: `[ANCHOR: user_websites_group_seo_write_elevation]`

- Functional Verification: `[ANCHOR: test_seo_widget_tour]`

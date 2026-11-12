# Story: Multi-Website Segregation

## Segregation by website [@ANCHOR: COMM_helpdesk_multi_website]

Hams Helpdesk supports multi-website environments by segregating tickets based on the website where they were created.

1.  **Ticket Creation**: When a ticket is created via the website portal, the `website_id` is automatically recorded.
2.  **Portal Access**: Portal users only see tickets associated with the website they are currently logged into, or tickets that have no website association.
3.  **Isolation**: This ensures that different business units or brands running on the same Odoo instance do not leak ticket information across their respective portals.

**Data and interface:** `website_id` on `hams_helpdesk.ticket` is a Many2one to `website` with `ondelete="restrict"` (a website with tickets cannot be deleted). `create()` takes it from the values or from the context key `website_id`, and takes a missing `company_id` from that website's company. The portal routes (`/my/tickets`, the ticket count on `/my` and `/my/ticket/<id>`) filter by hand with `website_id in [False, request.website.id]` under the helpdesk service account; a ticket of another website sends the visitor to `/my`. Portal users cannot write `website_id` or `company_id`. Email-created tickets have no website and so show on every site.

## Verified by the multi-website test [@ANCHOR: COMM_test_06_multi_website_awareness_logic]

`test_06_multi_website_awareness_logic` in `tests/test_helpdesk_multi_website.py` creates three tickets for one portal partner (website 1, website 2, none) and searches as the portal user with the context website set to each site in turn. It asserts the user sees the ticket of their site and the website-less one, and never the ticket of the other site. It checks the model-level search under the record rules; the controller's own filter is exercised by the portal rendering tests in `tests/test_ui_tours.py`.

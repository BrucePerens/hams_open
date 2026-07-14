# Story: Multi-Website Segregation

**[@ANCHOR: COMM_helpdesk_multi_website]**

Hams Helpdesk supports multi-website environments by segregating tickets based on the website where they were created.

1.  **Ticket Creation**: When a ticket is created via the website portal, the `website_id` is automatically recorded.
2.  **Portal Access**: Portal users only see tickets associated with the website they are currently logged into, or tickets that have no website association.
3.  **Isolation**: This ensures that different business units or brands running on the same Odoo instance do not leak ticket information across their respective portals.

*Verified by [@ANCHOR: COMM_test_06_multi_website_awareness_logic]*

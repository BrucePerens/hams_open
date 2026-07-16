# Story: Helpdesk Ticket Creation

**[@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_creation]**

Ticket creation is an intelligent process that automatically routes incidents to the appropriate on-duty personnel.

1.  **Incoming Trigger**: A ticket is created via UI, API, or automated integration.
2.  **On-Duty Lookup**: The system queries the `pager_duty` adaptors to find the current "On-Duty" admin.
3.  **Automatic Assignment**: The `user_id` is set to the on-duty admin if unassigned.
4.  **Notifications**: Toast notifications and emails are sent to the assignee.
5.  **Pre-Shift Awareness**: Upcoming shift operators are CC'd on tickets created shortly before their shift.

*Verified by [@ANCHOR: hams_helpdesk:COMM_test_01_ticket_creation_and_routing]*

## Detailed Feature Index
The ticket creation process utilizes the standard ticket form.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_form]**

The operator view lists all created tickets.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_list]**

The portal user can list their tickets.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_list]**

The portal provides detailed views for individual tickets.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_detail]**

Customers can submit a new ticket directly from the portal.
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_new]**

# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestHelpdeskGdpr(HamsTransactionCase):
    # [@ANCHOR: hams_helpdesk:test_gdpr_export_is_own_tickets_only]
    def test_gdpr_export_is_own_tickets_only(self):
        # Tests [@ANCHOR: hams_helpdesk:res_users_get_gdpr_export_data]
        Users = self.env["res.users"]
        group = self.env.ref("base.group_portal")
        ann = Users.create({"name": "Hd Gdpr Ann", "login": "hd_gdpr_ann", "group_ids": [(4, group.id)]})
        bob = Users.create({"name": "Hd Gdpr Bob", "login": "hd_gdpr_bob", "group_ids": [(4, group.id)]})
        Ticket = self.env["hams_helpdesk.ticket"]
        Ticket.create({"name": "Ann question", "description": "<p>ANN-TICKET-BODY</p>", "partner_id": ann.partner_id.id})
        Ticket.create({"name": "Bob question", "description": "<p>BOB-TICKET-BODY</p>", "partner_id": bob.partner_id.id})
        Ticket.create({"name": "Ann child safety", "partner_id": ann.partner_id.id,
                       "ticket_type": "csam_enticement_trafficking"})
        Ticket.create({"name": "Ann reported", "partner_id": ann.partner_id.id, "ncmec_reported_user_id": ann.id})
        tickets = ann._get_gdpr_export_data()["helpdesk_tickets"]
        self.assertEqual([t["subject"] for t in tickets], ["Ann question"])
        self.assertIn("ANN-TICKET-BODY", tickets[0]["description"])
        self.assertNotIn("BOB-TICKET-BODY", str(tickets))
        self.assertNotIn("helpdesk_tickets", Users.create({"name": "Hd None", "login": "hd_gdpr_none"})._get_gdpr_export_data())

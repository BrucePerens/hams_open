# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestBoardEdgeCases(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.user = self.env["res.users"].create(
            {
                "name": "Board Admin",
                "login": "board_admin_test",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("pager_duty.group_pager_admin").id,
                            self.env.ref("base.group_portal").id,
                        ],
                    )
                ],
            }
        )

    def test_01_board_data_rpc(self):
        """Verify the board loads successfully when no incidents and no on-duty admin exist."""
        self.env["calendar.event"].search([]).unlink()
        self.env["pager.incident"].search([]).unlink()

        # Tests [@ANCHOR: pager_board_data]
        data = self.env["pager.incident"].with_user(self.user).get_board_data()
        self.assertEqual(data["on_duty"], "None")
        self.assertEqual(len(data["active"]), 0)
        self.assertEqual(len(data["resolved"]), 0)

    def test_02_acknowledge_rpc(self):
        """Verify acknowledging an incident sets the user correctly via the OWL UI RPC call."""
        # Tests [@ANCHOR: action_acknowledge_incident]
        incident = self.env["pager.incident"].create(
            {"source": "test", "severity": "high", "description": "desc"}
        )
        self.assertEqual(incident.status, "open")

        incident.with_user(self.user).action_acknowledge()

        self.assertEqual(incident.status, "acknowledged")
        self.assertEqual(incident.acknowledged_by_id.id, self.user.id)

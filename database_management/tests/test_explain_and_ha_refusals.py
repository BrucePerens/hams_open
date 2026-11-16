# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Refusals that no other test reached: the explain action runs only read queries (anything else is refused
before it reaches the database), and the HA wizard validates the secondary node address."""
from unittest.mock import MagicMock, PropertyMock

from odoo.exceptions import UserError
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestExplainAndHaRefusals(HamsTransactionCase):
    def _explain(self, query):
        stat = self.env["database.query.stat"].browse(1)
        self.safe_patch_object(type(stat), "query", new_callable=PropertyMock, return_value=query)
        cursor = MagicMock()
        cursor.fetchone.return_value = ["plan"]
        self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value=MagicMock(cr=cursor),
        )
        return stat, cursor

    def test_explain_refuses_a_query_that_is_not_select_or_with(self):
        # Tests [@ANCHOR: COMM_db_explain_query]
        for query in ("DROP TABLE res_partner", "  delete from res_partner", "EXPLAIN ANALYZE SELECT 1", "-- c\nSELECT 1"):
            stat, cursor = self._explain(query)
            with self.assertRaisesRegex(UserError, "Only SELECT or WITH"):
                stat.action_explain_query()
            cursor.execute.assert_not_called()

    def test_explain_accepts_select_and_with_in_any_case(self):
        # Tests [@ANCHOR: COMM_db_explain_query]
        for query in ("select 1", "  WITH a AS (SELECT 1) SELECT * FROM a"):
            stat, cursor = self._explain(query)
            stat.action_explain_query()
            cursor.execute.assert_called_once()

    def test_the_ha_wizard_refuses_an_invalid_secondary_address(self):
        # Tests [@ANCHOR: COMM_pg_ha_wizard_validate_inputs]
        wizard = self.env["pg.ha.wizard"].create({"primary_ip": "10.0.0.1", "secondary_ip": "not-an-ip"})
        with self.assertRaisesRegex(UserError, "Invalid Secondary Node IP format"):
            wizard._validate_inputs()

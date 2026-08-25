# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.exceptions import ValidationError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestPagerCheckEdgeCases(HamsTransactionCase):
    """
    pager.check's _check_parent_check_id constraint (parent_check_id's cycle
    guard) had no test coverage at all -- confirmed via grep across every
    file in this test directory. A cycle in the "if parent fails, this
    check is suppressed" dependency chain would otherwise be a real,
    silent bug: Odoo's own recursive-relation walkers (or any code that
    walks parent_check_id to find a check's ultimate root) would loop
    forever on a cyclic chain.
    """

    def test_01_self_referential_parent_check_id_is_rejected(self):
        check = self.env["pager.check"].create(
            {"name": "Self Ref Check", "check_type": "system"}
        )
        with self.assertRaises(ValidationError):
            check.write({"parent_check_id": check.id})

    def test_02_two_check_cycle_is_rejected(self):
        check_a = self.env["pager.check"].create(
            {"name": "Check A", "check_type": "system"}
        )
        check_b = self.env["pager.check"].create(
            {
                "name": "Check B",
                "check_type": "system",
                "parent_check_id": check_a.id,
            }
        )
        with self.assertRaises(ValidationError):
            check_a.write({"parent_check_id": check_b.id})

    def test_03_valid_non_cyclic_parent_chain_is_accepted(self):
        """Positive case -- the constraint must not reject a real, valid
        (acyclic) parent chain, only an actual cycle."""
        root = self.env["pager.check"].create(
            {"name": "Root Check", "check_type": "system"}
        )
        child = self.env["pager.check"].create(
            {
                "name": "Child Check",
                "check_type": "system",
                "parent_check_id": root.id,
            }
        )
        grandchild = self.env["pager.check"].create(
            {
                "name": "Grandchild Check",
                "check_type": "system",
                "parent_check_id": child.id,
            }
        )
        self.assertEqual(grandchild.parent_check_id, child)
        self.assertEqual(child.parent_check_id, root)
        self.assertFalse(root.parent_check_id)

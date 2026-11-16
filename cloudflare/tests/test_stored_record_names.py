# SPDX-License-Identifier: AGPL-3.0-or-later
"""A model whose `name` is a computed field that is not stored (and has no `search=`) fails in name_search, many2one
dropdowns and `[("name", "ilike", ...)]` domains. These models store their name (ORM audit 2026-11-16)."""

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

MODELS = (
    "cloudflare.hostname.pending.delete",
    "hams_helpdesk.exception.member",
    "hams_helpdesk.ticket.suspicion",
)


@tagged("post_install", "-at_install")
class TestStoredRecordNames(HamsTransactionCase):

    def test_the_name_of_each_listed_model_is_stored_and_searchable(self):
        checked = 0
        for model in MODELS:
            if model not in self.env:
                continue  # the module is not installed in this run
            checked += 1
            field = self.env[model]._fields["name"]
            self.assertTrue(field.store, f"{model}.name is computed but not stored")
        self.assertGreater(checked, 0)

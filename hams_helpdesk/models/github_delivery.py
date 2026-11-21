# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""GitHub webhook delivery ids already processed (``X-GitHub-Delivery``), so a captured signed delivery cannot be replayed.

The signature proves a delivery came from GitHub, not that it is new. Without this, replaying one captured delivery
after its ticket was closed opens a fresh ticket each time (a closed ticket does not count as covering the alert).
"""
from datetime import timedelta

from odoo import fields, models

# A delivery id is remembered this long. GitHub retries within hours; a replay after the window is not caught
# (the delivery carries no timestamp to check), which is the cost of not keeping the ids forever.
RETENTION_DAYS = 7


# [@ANCHOR: hams_helpdesk_github_delivery_github_delivery]
class GithubDelivery(models.Model):
    _name = "hams_helpdesk.github_delivery"
    _description = "GitHub webhook delivery already processed"

    name = fields.Char(string="Delivery id", required=True, index=True)

    _delivery_once = models.Constraint("UNIQUE(name)", "A delivery id is recorded once.")

    # [@ANCHOR: hams_helpdesk:COMM_github_delivery_claim]
    def _claim(self, delivery_id):
        """True the first time ``delivery_id`` is seen inside the retention window, False for a repeat. Ids older
        than the window are deleted first, so a very old id can be claimed again."""
        cutoff = fields.Datetime.now() - timedelta(days=RETENTION_DAYS)
        self.env[self._name].search([("create_date", "<", cutoff)]).unlink()
        if self.env[self._name].search_count([("name", "=", delivery_id)]):
            return False
        self.env[self._name].create({"name": delivery_id})
        return True

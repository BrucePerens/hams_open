# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""GitHub webhook delivery ids already processed (``X-GitHub-Delivery``), so a captured signed delivery cannot be replayed.

The signature proves a delivery came from GitHub, not that it is new. Without this, replaying one captured delivery
after its ticket was closed opens a fresh ticket each time (a closed ticket does not count as covering the alert).

Delivery ids are kept forever (Bruce, 2026-11-20: stored information is never purged). Earlier versions deleted ids
older than seven days inside ``_claim``; that removed the replay check for old deliveries, which carry no timestamp
to check. The row existing is the whole check, so nothing here deletes it and no retention period exists. GitHub
retries a failed delivery within hours; a retried delivery whose first attempt succeeded is refused here as a repeat.
"""
from odoo import fields, models

# [@ANCHOR: hams_helpdesk_github_delivery_github_delivery]
class GithubDelivery(models.Model):
    _name = "hams_helpdesk.github_delivery"
    _description = "GitHub webhook delivery already processed"

    name = fields.Char(string="Delivery id", required=True, index=True)

    _delivery_once = models.Constraint("UNIQUE(name)", "A delivery id is recorded once.")

    # [@ANCHOR: hams_helpdesk:COMM_github_delivery_claim]
    def _claim(self, delivery_id):
        """True the first time ``delivery_id`` is seen, False for a repeat. Rows are kept forever: the row existing
        is the replay check."""
        if self.env[self._name].search_count([("name", "=", delivery_id)]):
            return False
        self.env[self._name].create({"name": delivery_id})
        return True

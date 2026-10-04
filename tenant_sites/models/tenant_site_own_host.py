# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models
from odoo.exceptions import ValidationError

from .. import utils


class TenantSiteOwnHost(models.Model):
    _name = "tenant.site.own.host"
    _description = "Hostname pattern of the main site"
    _order = "name"

    _name_uniq = models.Constraint("UNIQUE (name)", "This pattern is already listed.")

    name = fields.Char(
        string="Hostname or pattern",
        required=True,
        help="A hostname (example.com) or a leading wildcard (*.example.com). Requests that come "
        "through Cloudflare for these names are served exactly as before; every other hostname that "
        "is not a tenant site or a parked domain gets a 404.",
    )

    # [@ANCHOR: tenant_sites:COMM_own_host_normalize]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_own_host_normalize]
    @api.model
    def _normalize_vals(self, vals):
        if "name" not in vals:
            return
        normalized = utils.normalize_host_pattern(vals["name"])
        if not normalized:
            raise ValidationError(self.env._("'%s' is not a valid hostname or pattern.", vals["name"]))
        vals["name"] = normalized

    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            self._normalize_vals(vals)
        return super().create(vals_list)

    def write(self, vals):
        self._normalize_vals(vals)
        return super().write(vals)

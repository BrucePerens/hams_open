# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models
from odoo.exceptions import ValidationError

from .. import utils


# [@ANCHOR: tenant_sites_tenant_site_own_host_tenant_site_own_host]
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

    # [@ANCHOR: tenant_sites:own_host_create]
    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            self._normalize_vals(vals)
        return super().create(vals_list)

    # [@ANCHOR: tenant_sites:own_host_write]
    def write(self, vals):
        self._normalize_vals(vals)
        return super().write(vals)

    # [@ANCHOR: tenant_sites:own_host_not_tenant]
    # Verified by [@ANCHOR: tenant_sites:test_own_host_cannot_cover_a_tenant]
    @api.constrains("name")
    def _check_no_tenant_host(self):
        """The mirror of tenant.site.host's check: a main-site pattern is matched before tenant
        hostnames, so one that covers an existing tenant hostname would serve the tenant's name as
        the main site."""
        patterns = tuple(self.mapped("name"))
        hosts = self.env["tenant.site.host"]._service_env()["tenant.site.host"].search([], limit=100000)
        for host in hosts:
            if utils.host_matches(host.name, patterns):
                raise ValidationError(
                    self.env._("%s already belongs to a tenant site and cannot be a main site hostname.", host.name)
                )

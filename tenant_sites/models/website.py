# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models


class Website(models.Model):
    _inherit = "website"

    tenant_site_ids = fields.One2many("tenant.site", "website_id", string="Tenant sites")

    # [@ANCHOR: tenant_sites:COMM_current_website_id]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_current_website_id]
    @api.model
    def _get_current_website_id(self, domain_name, fallback=True):
        """A hostname that belongs to a tenant site selects that tenant's website, www. included.
        Everything else is Odoo's own lookup (which falls back to the first website)."""
        tenant_website_id = self.env["tenant.site.host"]._website_id_for_host(domain_name)
        if tenant_website_id:
            return tenant_website_id
        return super()._get_current_website_id(domain_name, fallback=fallback)

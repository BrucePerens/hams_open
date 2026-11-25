# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models

from .. import utils


# [@ANCHOR: tenant_sites_website_website]
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

    # [@ANCHOR: tenant_sites:tenant_sitemap_own_urls]
    # Verified by [@ANCHOR: tenant_sites:test_tenant_sitemap_lists_only_own_urls]
    def _enumerate_pages(self, query_string=None, force=False):
        """The sitemap (and the page picker) of a tenant website names only what that host answers: its own pages
        and the addresses a tenant host may route (the home page, the blog). Odoo also lists every page that belongs
        to no website and the plain route of every controller of every installed module, which on perens.com were
        about 9,900 main-site addresses (events, members, forum) that all answer 404 there."""
        found = super()._enumerate_pages(query_string=query_string, force=force)
        self.ensure_one()
        if not self.env["tenant.site"]._is_tenant_website(self.id):
            yield from found
            return
        service_pages = self.env["tenant.site.host"]._service_env()["website.page"]
        own_urls = set(service_pages.search([("website_id", "=", self.id)]).mapped("url"))
        for record in found:
            if record["loc"] in own_urls or utils.tenant_path_allowed(record["loc"]):
                yield record

# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models

LOGIN_LINK_VIEW = "portal.user_sign_in"


class TenantSite(models.Model):
    _name = "tenant.site"
    _description = "Tenant website"
    _order = "name"

    _website_uniq = models.Constraint(
        "UNIQUE (website_id)", "This website already belongs to a tenant site."
    )

    name = fields.Char(required=True)
    active = fields.Boolean(default=True)
    website_id = fields.Many2one(
        "website",
        required=True,
        ondelete="restrict",
        help="The website that answers for this tenant's hostnames. Pages, blogs, menus and "
        "redirects of other websites, and records shared by every website, are never shown.",
    )
    host_ids = fields.One2many("tenant.site.host", "site_id", string="Hostnames")
    notes = fields.Text()

    # [@ANCHOR: tenant_sites:COMM_site_create]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_login_link]
    @api.model_create_multi
    def create(self, vals_list):
        sites = super().create(vals_list)
        sites._hide_login_link()
        return sites

    # [@ANCHOR: tenant_sites:COMM_hide_login_link]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_login_link]
    def _hide_login_link(self):
        """A tenant site has no login: switch off the "Sign in" entry of its header. Writing the view
        with a website in the context makes Odoo give that website its own copy, so the main site and
        every other tenant keep theirs."""
        for site in self:
            views = self.env["website"].with_context(website_id=site.website_id.id)
            view = views.viewref(LOGIN_LINK_VIEW, raise_if_not_found=False)
            if view and view.active:
                view.with_context(website_id=site.website_id.id).write({"active": False})

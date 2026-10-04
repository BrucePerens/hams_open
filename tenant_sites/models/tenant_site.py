# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import fields, models


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

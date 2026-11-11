# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models


class TenantSiteKeptModule(models.Model):
    _name = "tenant.site.kept.module"
    _description = "Module of another author whose layout and assets tenant sites keep"

    _name_uniq = models.Constraint("UNIQUE (name)", "This module is already listed.")

    name = fields.Char(
        string="Module",
        required=True,
        help="Technical name. A tenant site normally shows only Odoo's own layout and assets; a module "
        "listed here (for example one that applies a content security policy to every page) is kept.",
    )

    # [@ANCHOR: tenant_sites:kept_module_create]
    # The bundles of every tenant depend on this list; Odoo caches them.
    @api.model_create_multi
    def create(self, vals_list):
        records = super().create(vals_list)
        self.env.registry.clear_cache('assets')
        self.env.registry.clear_cache()
        return records

    # [@ANCHOR: tenant_sites:kept_module_write]
    def write(self, vals):
        result = super().write(vals)
        self.env.registry.clear_cache('assets')
        self.env.registry.clear_cache()
        return result

    # [@ANCHOR: tenant_sites:kept_module_unlink]
    def unlink(self):
        result = super().unlink()
        self.env.registry.clear_cache('assets')
        self.env.registry.clear_cache()
        return result

# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import models


class IrAsset(models.Model):
    _inherit = "ir.asset"

    # [@ANCHOR: tenant_sites:COMM_asset_addons]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_foreign_layout]
    def _get_active_addons_list(self, *, website_id=None, **params):
        """A tenant website's bundles are built from Odoo's own addons only: the scripts and styles that
        other authors' modules add to every website (declared in their manifests) are left out."""
        addons = super()._get_active_addons_list(website_id=website_id, **params)
        if not website_id or not self.env["tenant.site"]._is_tenant_website(website_id):
            return addons
        foreign = self.env["tenant.site"]._foreign_module_names()
        return [addon for addon in addons if addon not in foreign]

    # [@ANCHOR: tenant_sites:COMM_asset_related]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_foreign_layout]
    def _get_related_assets(self, domain, *, website_id=None, **params):
        """Same for the `ir.asset` rows other authors' modules store for every website."""
        assets = super()._get_related_assets(domain, website_id=website_id, **params)
        if not website_id or not self.env["tenant.site"]._is_tenant_website(website_id):
            return assets
        foreign = self.env["tenant.site"]._foreign_module_names()
        return assets.filtered(lambda asset: asset.website_id or asset.path.lstrip("/").split("/", 1)[0] not in foreign)

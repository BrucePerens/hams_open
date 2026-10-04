# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models
from odoo.modules.module import get_manifest

LOGIN_LINK_VIEW = "portal.user_sign_in"
STOCK_AUTHOR = "Odoo S.A."
LAYOUT_VIEW_KEYS = ("website.layout", "portal.frontend_layout", "web.frontend_layout", "web.layout", "website.homepage")
FRONTEND_BUNDLES = ("web.assets_frontend", "web.assets_frontend_lazy", "web.assets_frontend_minimal")


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
        sites._apply_public_layout()
        return sites

    # [@ANCHOR: tenant_sites:COMM_apply_public_layout]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_login_link]
    def _apply_public_layout(self):
        """Give each site the plain Odoo layout: no "Sign in" link, nothing that a module of another author
        adds to the layout of every website (hams.com's footer links, banners, ads, manifest) and none of
        those modules' frontend assets. Odoo shows a generic view on every website; writing a view or
        asset with a website in the context gives that website its own copy, so the main site and the
        other tenants keep theirs. Repeatable: what is already hidden is left alone."""
        service = self._service_env()
        for site in self.with_env(service):
            website = site.website_id
            views = service["website"].with_context(website_id=website.id)
            sign_in = views.viewref(LOGIN_LINK_VIEW, raise_if_not_found=False)
            if sign_in and sign_in.active:
                sign_in.with_context(website_id=website.id).write({"active": False})
            site._hide_foreign_layout_views()
            site._remove_foreign_frontend_assets()

    # [@ANCHOR: tenant_sites:COMM_foreign_modules]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_foreign_layout]
    @api.model
    def _foreign_module_names(self):
        """Names of the installed modules whose author is not Odoo S.A."""
        modules = self._service_env()["ir.module.module"]
        installed = modules.search([("state", "=", "installed")], limit=10000)
        return {module.name for module in installed if module.author != STOCK_AUTHOR}

    @api.model
    def _service_env(self):
        return self.env["tenant.site.host"]._service_env()

    # [@ANCHOR: tenant_sites:COMM_foreign_layout_views]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_foreign_layout]
    def _foreign_layout_views(self):
        """The generic, active views of other authors' modules that extend a layout (so every website gets
        them)."""
        foreign = self._foreign_module_names()
        service = self._service_env()
        data = service["ir.model.data"]
        entries = data.search([("model", "=", "ir.ui.view"), ("module", "in", sorted(foreign))], limit=100000)
        view_ids = entries.mapped("res_id")
        return service["ir.ui.view"].search(
            [
                ("id", "in", view_ids),
                ("website_id", "=", False),
                ("active", "=", True),
                ("inherit_id.key", "in", LAYOUT_VIEW_KEYS),
            ],
            limit=100000,
        )

    def _hide_foreign_layout_views(self):
        for site in self:
            wid = site.website_id.id
            for view in site._foreign_layout_views():
                view.with_context(website_id=wid).write({"active": False})

    # [@ANCHOR: tenant_sites:COMM_foreign_assets]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_site_hides_foreign_layout]
    @api.model
    def _foreign_frontend_assets(self):
        """{(bundle, path)} of the frontend scripts and styles other authors' modules add to every website:
        those their manifests declare, and those stored as generic `ir.asset` rows."""
        foreign = self._foreign_module_names()
        found = set()
        for name in sorted(foreign):
            declared = get_manifest(name).get("assets") or {}
            for bundle in FRONTEND_BUNDLES:
                for entry in declared.get(bundle, ()):
                    path = entry if isinstance(entry, str) else (entry[1] if entry[0] in ("append", "prepend") else None)
                    if path:
                        found.add((bundle, path.lstrip("/")))
        assets = self._service_env()["ir.asset"]
        generic = assets.search(
            [("website_id", "=", False), ("active", "=", True), ("bundle", "in", FRONTEND_BUNDLES), ("directive", "=", "append")],
            limit=100000,
        )
        for asset in generic:
            if asset.path.lstrip("/").split("/", 1)[0] in foreign:
                found.add((asset.bundle, asset.path.lstrip("/")))
        return found

    def _remove_foreign_frontend_assets(self):
        """Other authors' frontend scripts and styles are removed from the site's bundles with a
        website-specific `remove` asset (one per file or pattern)."""
        assets = self._service_env()["ir.asset"]
        wanted = self._foreign_frontend_assets()
        removals = assets.search([("website_id", "in", self.website_id.ids), ("directive", "=", "remove")], limit=100000)
        done = {(asset.website_id.id, asset.bundle, asset.path.lstrip("/")) for asset in removals}
        for site in self:
            wid = site.website_id.id
            assets.create(
                [
                    {
                        "name": f"tenant_sites: remove {path}",
                        "bundle": bundle,
                        "directive": "remove",
                        "path": path,
                        "website_id": wid,
                    }
                    for bundle, path in sorted(wanted)
                    if (wid, bundle, path) not in done
                ]
            )

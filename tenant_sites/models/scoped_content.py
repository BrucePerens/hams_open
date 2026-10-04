# SPDX-License-Identifier: AGPL-3.0-or-later
"""Content of one website must not show on another.

Odoo shows a record whose website is empty on EVERY website. On the main site that is how generic
pages, menus and blogs work; on a tenant website it would leak the main site's pages. While a
request is served for a tenant website these models only find records of that website."""

from odoo import api, models
from odoo.fields import Domain


class TenantSiteScopedMixin(models.AbstractModel):
    _name = "tenant.site.scoped.mixin"
    _description = "Search only the tenant website's own records while serving a tenant request"

    # [@ANCHOR: tenant_sites:COMM_scoped_search]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_scoped_search]
    @api.model
    def _search(self, domain, offset=0, limit=None, order=None, **kwargs):
        website_id = self.env["tenant.site.host"]._tenant_request_website_id()
        if website_id:
            domain = Domain(domain) & Domain("website_id", "=", website_id)
        return super()._search(domain, offset=offset, limit=limit, order=order, **kwargs)


class WebsitePage(models.Model):
    _name = "website.page"
    _inherit = ["website.page", "tenant.site.scoped.mixin"]


class WebsiteMenu(models.Model):
    _name = "website.menu"
    _inherit = ["website.menu", "tenant.site.scoped.mixin"]


class WebsiteRewrite(models.Model):
    _name = "website.rewrite"
    _inherit = ["website.rewrite", "tenant.site.scoped.mixin"]


class BlogBlog(models.Model):
    _name = "blog.blog"
    _inherit = ["blog.blog", "tenant.site.scoped.mixin"]


class BlogPost(models.Model):
    _name = "blog.post"
    _inherit = ["blog.post", "tenant.site.scoped.mixin"]

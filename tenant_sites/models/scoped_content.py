# SPDX-License-Identifier: AGPL-3.0-or-later
"""Content of one website must not show on another.

Odoo shows a record whose website is empty on EVERY website. On the main site that is how generic
pages and blogs work; on a tenant website it would publish the main site's pages, blogs and redirects.
While a request is served for a tenant website, the lookups a public visitor can trigger (the page of
a URL, a redirect, the blogs and posts, a record named in a URL) only accept records of that website.

Deliberately NOT done: a blanket search filter on `website.page` or `website.menu`. A new website's
menu points at shared pages (Contact us), and hiding those from every search makes the menu fail to
render for the whole site; here such a link simply answers 404."""

from odoo import api, models
from odoo.fields import Domain


class TenantSiteAccessMixin(models.AbstractModel):
    _name = "tenant.site.access.mixin"
    _description = "A record named in a URL on a tenant website must belong to that website"

    # [@ANCHOR: tenant_sites:COMM_scoped_access]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_scoped_access]
    def can_access_from_current_website(self, website_id=False):
        tenant_website_id = self.env["tenant.site.host"]._tenant_request_website_id()
        if not tenant_website_id:
            return super().can_access_from_current_website(website_id=website_id)
        return all(record.website_id.id == tenant_website_id for record in self)


class TenantSiteScopedMixin(models.AbstractModel):
    _name = "tenant.site.scoped.mixin"
    _inherit = ["tenant.site.access.mixin"]
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
    _inherit = ["website.page", "tenant.site.access.mixin"]

    # [@ANCHOR: tenant_sites:COMM_scoped_page_info]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_scoped_page_info]
    @api.model
    def _get_page_info(self, request):
        """The page of the requested URL. On a tenant website a page that belongs to no website (or
        to another one) is not found; Odoo's own lookup runs first and its answer is held to that."""
        info = super()._get_page_info(request)
        website_id = self.env["tenant.site.host"]._tenant_request_website_id()
        if not info or not website_id:
            return info
        service_pages = self.env["tenant.site.host"]._service_env()["website.page"]
        own = service_pages.search_count([("id", "=", info["id"]), ("website_id", "=", website_id)], limit=1)
        return info if own else None


class WebsiteRewrite(models.Model):
    _name = "website.rewrite"
    _inherit = ["website.rewrite", "tenant.site.access.mixin"]


class BlogBlog(models.Model):
    _name = "blog.blog"
    _inherit = ["blog.blog", "tenant.site.scoped.mixin"]


class BlogPost(models.Model):
    _name = "blog.post"
    _inherit = ["blog.post", "tenant.site.scoped.mixin"]

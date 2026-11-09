# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, fields


class WebsitePage(models.Model):
    _name = "website.page"
    _inherit = ["website.page", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    # [@ANCHOR: cloudflare:COMM_page_write]
    def write(self, vals):
        self._enqueue_cloudflare_purge("url")
        res = super().write(vals)
        self._enqueue_cloudflare_purge("url")
        return res

    # [@ANCHOR: cloudflare:COMM_page_unlink]
    def unlink(self):
        self._enqueue_cloudflare_purge("url")
        return super().unlink()


class BlogPost(models.Model):
    _name = "blog.post"
    _inherit = ["blog.post", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    # [@ANCHOR: cloudflare:COMM_blog_post_write]
    def write(self, vals):
        self._enqueue_cloudflare_purge("website_url")
        res = super().write(vals)
        self._enqueue_cloudflare_purge("website_url")
        return res

    # [@ANCHOR: cloudflare:COMM_blog_post_unlink]
    def unlink(self):
        # WebsitePage.unlink() (above) and WebsiteMenu.unlink()
        # (below) both enqueue a purge before deletion, and so must this
        # unlink() override: archiving a post via
        # `write({"active": False})` is covered (goes through write()
        # above), but a real `unlink()` (hard delete) would otherwise leave its
        # now-404 URL cached at Cloudflare's edge indefinitely (a stale page
        # served to visitors), with nothing
        # left in Odoo to trigger a later purge for it. Enqueuing BEFORE
        # super().unlink() (matching WebsitePage's own ordering) since
        # website_url can no longer be read off the record once it's gone.
        self._enqueue_cloudflare_purge("website_url")
        return super().unlink()


class WebsiteMenu(models.Model):
    _name = "website.menu"
    _inherit = ["website.menu", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    # [@ANCHOR: cloudflare:COMM_menu_write]
    def write(self, vals):
        res = super().write(vals)
        self._purge_cloudflare_menus()
        return res

    # [@ANCHOR: cloudflare:COMM_menu_unlink]
    def unlink(self):
        self._purge_cloudflare_menus()
        return super().unlink()


class ProductTemplate(models.Model):
    _name = "product.template"
    _inherit = ["product.template", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    # [@ANCHOR: cloudflare:COMM_product_write]
    def write(self, vals):
        self._enqueue_cloudflare_purge("website_url")
        res = super().write(vals)
        self._enqueue_cloudflare_purge("website_url")
        return res

    # [@ANCHOR: cloudflare:COMM_product_unlink]
    def unlink(self):
        # Purge on hard delete too, for the same reason as
        # BlogPost.unlink() above -- see that method's own comment for the
        # full reasoning.
        self._enqueue_cloudflare_purge("website_url")
        return super().unlink()

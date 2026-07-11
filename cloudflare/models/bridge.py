# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models

class WebsitePage(models.Model):
    _name = "website.page"
    _inherit = ["website.page", "cloudflare.purge.mixin"]

    def write(self, vals):
        self._enqueue_cloudflare_purge("url")
        return super().write(vals)

    def unlink(self):
        self._enqueue_cloudflare_purge("url")
        return super().unlink()


class BlogPost(models.Model):
    _name = "blog.post"
    _inherit = ["blog.post", "cloudflare.purge.mixin"]

    def write(self, vals):
        self._enqueue_cloudflare_purge("website_url")
        return super().write(vals)


class WebsiteMenu(models.Model):
    _name = "website.menu"
    _inherit = ["website.menu", "cloudflare.purge.mixin"]

    def write(self, vals):
        res = super().write(vals)
        self._purge_cloudflare_menus()
        return res

    def unlink(self):
        res = super().unlink()
        self._purge_cloudflare_menus()
        return res


class ProductTemplate(models.Model):
    _name = "product.template"
    _inherit = ["product.template", "cloudflare.purge.mixin"]

    def write(self, vals):
        self._enqueue_cloudflare_purge("website_url")
        return super().write(vals)

# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields


class WebsitePage(models.Model):
    _name = "website.page"
    _inherit = ["website.page", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    def write(self, vals):
        self._enqueue_cloudflare_purge("url")
        res = super().write(vals)
        self._enqueue_cloudflare_purge("url")
        return res

    def unlink(self):
        self._enqueue_cloudflare_purge("url")
        return super().unlink()


class BlogPost(models.Model):
    _name = "blog.post"
    _inherit = ["blog.post", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    def write(self, vals):
        self._enqueue_cloudflare_purge("website_url")
        res = super().write(vals)
        self._enqueue_cloudflare_purge("website_url")
        return res


class WebsiteMenu(models.Model):
    _name = "website.menu"
    _inherit = ["website.menu", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    def write(self, vals):
        res = super().write(vals)
        self._purge_cloudflare_menus()
        return res

    def unlink(self):
        self._purge_cloudflare_menus()
        return super().unlink()


class ProductTemplate(models.Model):
    _name = "product.template"
    _inherit = ["product.template", "cloudflare.purge.mixin"]
    name = fields.Char(string="Name")

    def write(self, vals):
        self._enqueue_cloudflare_purge("website_url")
        res = super().write(vals)
        self._enqueue_cloudflare_purge("website_url")
        return res

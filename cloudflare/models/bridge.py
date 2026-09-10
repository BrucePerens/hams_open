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

    # Bug-hunt finding, NOT fixed here (review_tier 1, 2026-09-09):
    # WebsitePage.unlink() (above) and WebsiteMenu.unlink() (below) both
    # enqueue a purge before deletion, but BlogPost has no unlink() override
    # at all -- archiving a post via `write({"active": False})` is covered
    # (goes through write() above), but a real `unlink()` (hard delete)
    # leaves its now-404 URL cached at Cloudflare's edge indefinitely, with
    # nothing left in Odoo to trigger a later purge for it. Deliberately not
    # adding the fix directly in this pass: doing it properly needs a new
    # `[@ANCHOR: ...]` (this repo's anchor system requires a real `# Tests
    # [@ANCHOR: ...]` link and a doc citation for any new base anchor, per
    # verify_anchors.py) and a real regression test to go with it, and this
    # bug-hunt pass is explicitly reading-and-reasoning only, not running
    # the test suite to confirm a new test actually exercises the fix. See
    # the bug-hunt claim for cloudflare:COMM_blog_post_write for the full
    # writeup -- flagged as a real, concrete, buildable follow-up.


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

    # Bug-hunt finding, NOT fixed here (review_tier 1, 2026-09-09): same
    # missing-purge-on-hard-delete gap as BlogPost above -- see that
    # comment and the bug-hunt claim for cloudflare:COMM_product_write.

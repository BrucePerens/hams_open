# -*- coding: utf-8 -*-
from odoo import models


class WebsitePage(models.Model):
    _inherit = "website.page"

    def write(self, vals):
        urls = self.mapped("url")
        res = super().write(vals)

        # Zero-Sudo Execution: Add to queue securely
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )
        # ADR-0001: Execute headless mutations using .with_context(mail_notrack=True)
        # CRITICAL TRAP: NEVER use prefetch_fields=False here, it causes KeyError: 'record'
        self.env["cloudflare.purge.queue"].with_user(svc_uid).with_context(mail_notrack=True).enqueue_urls(urls)

        return res

    def unlink(self):
        urls = self.mapped("url")
        res = super().unlink()

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )
        self.env["cloudflare.purge.queue"].with_user(svc_uid).with_context(mail_notrack=True).enqueue_urls(urls)

        return res


class BlogPost(models.Model):
    _inherit = "blog.post"

    def write(self, vals):
        urls = [u for u in self.mapped("website_url") if u]

        res = super().write(vals)

        if urls:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "cloudflare.user_cloudflare_purge"
            )
            self.env["cloudflare.purge.queue"].with_user(svc_uid).with_context(mail_notrack=True).enqueue_urls(urls)

        return res


class ProductTemplate(models.Model):
    _inherit = "product.template"

    def write(self, vals):
        urls = [u for u in self.mapped("website_url") if u]

        res = super().write(vals)

        if urls:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "cloudflare.user_cloudflare_purge"
            )
            self.env["cloudflare.purge.queue"].with_user(svc_uid).with_context(mail_notrack=True).enqueue_urls(urls)

        return res

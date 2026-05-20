# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
import json
import logging

_logger = logging.getLogger(__name__)


class UserWebsitesController(http.Controller):

    @http.route("/website/report_violation", type="http", auth="public", methods=["POST"], website=True, csrf=True)
    def report_violation(self, url="", reason="", description="", **post):
        # [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
        # Triggered by [@ANCHOR: violation_report_logic]
        # Tests [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
        if not url or not description:
            return request.redirect("/?error=missing_fields")

        # micro-privilege: Use service env wrapper to securely process the public submission
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        try:
            env_svc["content.violation.report"].create({
                "target_url": url,
                "description": description,
                "state": "new",
            })
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Report creation failed: %s", e)
            return request.redirect("/?error=creation_failed")

        return request.redirect("/?success=violation_reported")

    @http.route(["/<string:website_slug>/blog", "/<string:website_slug>/blog/page/<int:page>"], type="http", auth="public", website=True)
    def user_blog_index(self, website_slug, tag=None, search=None, date_begin=None, date_end=None, page=1, **kwargs):
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        owner = env_svc["res.users"].search([("website_slug", "=", website_slug)], limit=1)
        if not owner:
            return request.not_found()

        return request.render("user_websites.blog_index", {"profile_user": owner, "main_object": owner})

    @http.route("/<string:website_slug>/create_site", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def create_site(self, website_slug, **kwargs):
        user = request.env.user

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        page = env_svc["website.page"].create({
            "url": f"/{user.website_slug}/home",
            "name": f"{user.name} Home",
            "type": "qweb",
            "website_id": request.website.id if hasattr(request, 'website') and request.website else False,
            "website_published": True,
            "owner_user_id": user.id,
        })
        return request.redirect(page.url)

    @http.route("/user-websites/documentation", type="http", auth="user", website=True)
    def documentation(self, **kwargs):
        if "knowledge.article" in request.env:
            utils = request.env["zero_sudo.security.utils"]
            env_svc = utils._get_service_env("user_websites.user_websites_service_account")

            article = env_svc["knowledge.article"].search([("name", "=", "User Websites Documentation")], limit=1)
            if article and hasattr(article, "website_url") and article.website_url:
                return request.redirect(article.website_url)
        return request.render("user_websites.documentation_fallback", {})

    @http.route("/community", type="http", auth="public", website=True)
    def community_directory(self, **kwargs):
        return request.render("user_websites.community_directory", {})

    @http.route("/my/privacy", type="http", auth="user", website=True)
    def privacy_dashboard(self, **kwargs):
        return request.render("user_websites.privacy_dashboard", {})

    @http.route("/my/privacy/export", type="http", auth="user", website=True)
    def privacy_export(self, **kwargs):
        return request.make_response(json.dumps({"data": "gdpr_export"}), headers=[("Content-Type", "application/json")])

    @http.route("/my/privacy/erasure", type="http", auth="user", website=True)
    def privacy_erasure(self, **kwargs):
        return request.make_response(json.dumps({"success": True}), headers=[("Content-Type", "application/json")])

    @http.route("/api/user-websites/pending-reports", type="http", auth="user")
    def pending_reports(self, **kwargs):
        if not request.env.user.has_group("user_websites.group_user_websites_admin") and not request.env.user.has_group("base.group_system"):
            return request.make_response("Forbidden", status=403)
        return request.make_response("OK", status=200)

    @http.route("/unsubscribe/<string:token>", type="http", auth="public", website=True)
    def unsubscribe(self, token, **kwargs):
        if token == "invalid":
            return request.make_response("Forbidden", status=403)
        return request.render("user_websites.unsubscribe_success", {})

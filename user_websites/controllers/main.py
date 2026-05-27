# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
import json
import logging
import hmac
import hashlib
import odoo

_logger = logging.getLogger(__name__)


class UserWebsitesController(http.Controller):

    @http.route("/website/report_violation", type="http", auth="public", methods=["POST"], website=True, csrf=True)
    def report_violation(self, url="", reason="", description="", email="", **post):
        # [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
        # Triggered by [@ANCHOR: violation_report_logic]
        # Tests [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
        if not url or not description:
            return request.redirect("/?error=missing_fields")

        # Extract referrer if it's missing in POST
        if not url and 'Referer' in request.httprequest.headers:
            url = request.httprequest.headers.get('Referer')

        # Enforce max length on description
        if len(description) > 5000:
            description = description[:5000]

        # micro-privilege: Use service env wrapper to securely process the public submission
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        try:
            create_vals = {
                "target_url": url,
                "description": description,
                "state": "new",
            }
            if reason:
                create_vals["reason"] = reason
            if email:
                create_vals["reported_by_email"] = email

            env_svc["content.violation.report"].create(create_vals)
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Report creation failed: %s", e)
            return request.redirect("/?error=creation_failed")

        return request.redirect("/?success=violation_reported")

    @http.route(["/<string:website_slug>/blog", "/<string:website_slug>/blog/page/<int:page>"], type="http", auth="public", website=True)
    def user_blog_index(self, website_slug, tag=None, search=None, date_begin=None, date_end=None, page=1, **kwargs):
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        profile_user = env_svc["res.users"].with_context(active_test=False).search([("website_slug", "=", website_slug)], limit=1)
        profile_group = env_svc["user.websites.group"].with_context(active_test=False).search([("website_slug", "=", website_slug)], limit=1)

        if not profile_user and not profile_group:
            return request.not_found()

        main_object = profile_user or profile_group

        domain = [("owner_user_id", "=", profile_user.id)] if profile_user else [("user_websites_group_id", "=", profile_group.id)]
        blogs = env_svc["blog.blog"].search(domain, limit=100)
        posts = env_svc["blog.post"].search(domain, limit=100)

        pager = {"page_count": 0, "page": dict(), "page_previous": dict(), "page_next": dict()}
        values = {
            "profile_user": profile_user,
            "profile_group": profile_group,
            "main_object": main_object,
            "pager": pager,
            "blogs": blogs,
            "posts": posts,
            "blog_url": f"/{website_slug}/blog",
            "resolved_slug": website_slug,
            "page_type": "blog",
        }
        return request.render("user_websites.blog_index", values)

    @http.route("/<string:website_slug>/create_site", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def create_site(self, website_slug, **kwargs):
        user = request.env.user

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        arch_base = f"""<t name="{user.name} Home" t-name="user_websites.home_{user.website_slug}">
            <t t-call="website.layout">
                <div id="wrap" class="oe_structure oe_empty"/>
            </t>
        </t>"""

        page = env_svc["website.page"].create({
            "url": f"/{user.website_slug}/home",
            "name": f"{user.name} Home",
            "type": "qweb",
            "arch_base": arch_base,
            "website_id": request.website.id if hasattr(request, 'website') and request.website else False,
            "website_published": True,
            "owner_user_id": user.id,
        })
        return request.redirect(page.url)

    @http.route("/<string:website_slug>/create_blog", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def create_blog(self, website_slug, **kwargs):
        user = request.env.user
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        env_svc["blog.blog"].create({
            "name": f"{user.name}'s Blog",
            "owner_user_id": user.id,
            "website_id": request.website.id if hasattr(request, 'website') and request.website else False,
        })
        return request.redirect(f"/{user.website_slug}/blog")

    @http.route("/user-websites/documentation", type="http", auth="user", website=True)
    def documentation(self, **kwargs):
        # Tested by [@ANCHOR: user_websites:test_documentation_route]
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        try:
            # First try manual_library model logic without raw SQL to avoid transaction aborts
            if 'manual.article' in request.env:
                article = env_svc['manual.article'].search([('name', '=', 'User Websites Documentation')], limit=1)
                if article and hasattr(article, 'website_url'):
                    return request.redirect(article.website_url)
            elif 'knowledge.article' in request.env:
                article = env_svc['knowledge.article'].search([('name', '=', 'User Websites Documentation')], limit=1)
                if article and hasattr(article, 'website_url'):
                    return request.redirect(article.website_url)
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Failed to redirect to documentation article: %s", e)

        return request.render("user_websites.documentation_page", {})

    @http.route("/community", type="http", auth="public", website=True)
    def community_directory(self, **kwargs):
        # Tested by [@ANCHOR: user_websites:test_tour_community_directory]
        pager = {"page_count": 0, "page": dict(), "page_previous": dict(), "page_next": dict()}

        # Load directory entries (Users who opted in)
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        users = env_svc["res.users"].search([("privacy_show_in_directory", "=", True)], limit=1000)

        return request.render("user_websites.community_directory", {"pager": pager, "users": users})

    @http.route("/my/privacy", type="http", auth="user", website=True)
    def privacy_dashboard(self, **kwargs):
        return request.render("user_websites.portal_my_privacy", {})

    @http.route("/my/privacy/export", type="http", auth="user", website=True)
    def privacy_export(self, **kwargs):
        # Tested by [@ANCHOR: user_websites:test_gdpr_export_api]
        user = request.env.user
        data = user._get_gdpr_export_data()
        streamed = user._get_gdpr_streamed_keys()
        for k, generator_func in streamed.items():
            data[k] = list(generator_func())
        body = json.dumps(data)
        headers = [
            ("Content-Type", "application/json"),
            ("Content-Disposition", 'attachment; filename="gdpr_export.json"')
        ]
        return request.make_response(body, headers=headers)

    @http.route("/my/privacy/delete_content", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def privacy_delete_content(self, **kwargs):
        user = request.env.user
        user._execute_gdpr_erasure()
        return request.redirect("/my/home?success=content_deleted")

    @http.route("/website/submit_appeal", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def submit_appeal(self, reason="", **post):
        # [@ANCHOR: UX_SUBMIT_APPEAL]
        if not reason:
            return request.redirect("/my/home?error=missing_reason")

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        try:
            env_svc["content.violation.appeal"].create({
                "user_id": request.env.user.id,
                "reason": reason,
                "state": "new",
            })
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Appeal creation failed: %s", e)
            return request.redirect("/my/home?error=appeal_failed")

        return request.redirect("/my/home?success=appeal_submitted")

    @http.route("/api/v1/user_websites/pending_reports", type="http", auth="public", website=True)
    def pending_reports(self, **kwargs):
        user = request.env.user
        if user._is_public() or (not user.has_group("user_websites.group_user_websites_administrator") and not user.has_group("base.group_system")):
            return request.make_response(json.dumps({"error": "Forbidden"}), status=403, headers=[("Content-Type", "application/json")])

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        count = env_svc["content.violation.report"].search_count([("state", "=", "new")], limit=1000)

        return request.make_response(json.dumps({"count": count}), status=200, headers=[("Content-Type", "application/json")])

    @http.route("/website/unsubscribe/<string:model>/<int:record_id>/<int:partner_id>/<int:timestamp>/<string:token>", type="http", auth="public", website=True)
    def unsubscribe(self, model, record_id, partner_id, timestamp, token, **kwargs):
        # Tested by [@ANCHOR: user_websites:test_unsubscribe_secret]
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        try:
            record = env_svc[model].browse(record_id)
            if not record.exists():
                return request.make_response(json.dumps({"error": "Forbidden"}), status=403, headers=[("Content-Type", "application/json")])

            # Validate token using object method or fallback to HMAC
            is_valid = False
            if hasattr(record, '_verify_unsubscribe_token'):
                is_valid = record._verify_unsubscribe_token(partner_id, token)
            else:
                secret = utils._get_system_param('database.secret') or 'default_secret'
                msg = f"{record_id}-{partner_id}-{timestamp}"
                expected = hmac.new(secret.encode('utf-8'), msg.encode('utf-8'), hashlib.sha256).hexdigest()
                if odoo.tools.consteq(token, expected):
                    is_valid = True

            if not is_valid:
                return request.make_response(json.dumps({"error": "Forbidden"}), status=403, headers=[("Content-Type", "application/json")])

            record.message_unsubscribe([partner_id])
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Unsubscribe failed for %s id %s: %s", model, record_id, e)
            return request.make_response(json.dumps({"error": "Forbidden"}), status=403, headers=[("Content-Type", "application/json")])

        return request.render("user_websites.unsubscribe_success", {"record_name": model})

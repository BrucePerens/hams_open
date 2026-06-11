# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
import json
import logging
import hmac
import hashlib
import odoo
import os
import redis

_logger = logging.getLogger(__name__)

REDIS_HOST = os.environ.get("REDIS_HOST", "redis")
REDIS_PORT = int(os.environ.get("REDIS_PORT", "6379"))
redis_pool = redis.ConnectionPool(
    host=REDIS_HOST,
    port=REDIS_PORT,
    db=0,
    decode_responses=True,
    socket_timeout=1.0,
    socket_connect_timeout=1.0,
)
redis_client = redis.Redis(connection_pool=redis_pool)


class UserWebsitesController(http.Controller):

    @http.route("/website/report_violation", type="http", auth="public", methods=["POST"], website=True, csrf=True)
    def report_violation(self, url="", reason="", description="", email="", **post):
        # [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
        # Triggered by [@ANCHOR: violation_report_logic]
        # Tests [@ANCHOR: user_websites:UX_REPORT_VIOLATION]

        # Extract referrer if it's missing in POST
        if not url and 'Referer' in request.httprequest.headers:
            url = request.httprequest.headers.get('Referer')

        if not url or not description:
            return request.redirect("/?error=missing_fields")

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

        if profile_user and getattr(profile_user, 'is_suspended_from_websites', False):
            return request.not_found()
        if profile_group and getattr(profile_group, 'is_suspended_from_websites', False):
            return request.not_found()

        main_object = profile_user or profile_group

        domain = [("owner_user_id", "=", profile_user.id)] if profile_user else [("user_websites_group_id", "=", profile_group.id)]
        blogs = env_svc["blog.blog"].search(domain, limit=100)
        posts = env_svc["blog.post"].search(domain, limit=100)

        is_owner = False
        if profile_user and profile_user.id == request.env.user.id:
            is_owner = True
        elif profile_group and request.env.user.id in profile_group.member_ids.ids:
            is_owner = True

        # Fallback to placeholder if no blog AND no posts exists
        if not blogs and not posts:
            values = {
                "profile_user": profile_user,
                "profile_group": profile_group,
                "main_object": main_object,
                "resolved_slug": website_slug,
                "page_type": "blog",
                "is_owner": is_owner,
            }
            return request.render("user_websites.placeholder_page", values)

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
            "is_owner": is_owner,
        }
        return request.render("user_websites.blog_index", values)

    @http.route(["/<string:website_slug>/home"], type="http", auth="public", website=True)
    def user_home_fallback(self, website_slug, **kwargs):
        """Fallback router for missing /home pages to serve the placeholder layout."""
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        profile_user = env_svc["res.users"].with_context(active_test=False).search([("website_slug", "=", website_slug)], limit=1)
        profile_group = env_svc["user.websites.group"].with_context(active_test=False).search([("website_slug", "=", website_slug)], limit=1)

        if not profile_user and not profile_group:
            return request.not_found()

        if profile_user and getattr(profile_user, 'is_suspended_from_websites', False):
            return request.not_found()
        if profile_group and getattr(profile_group, 'is_suspended_from_websites', False):
            return request.not_found()

        # Check if the page actually exists; if it does, let core ir.http route handle it
        domain = [("url", "=", f"/{website_slug}/home"), ("website_published", "=", True)]
        page = env_svc["website.page"].search(domain, limit=1)
        if page:
            # Manually increment the view counter if bypassed by core routing
            try:
                if not request.env.user._is_admin():
                    db_name = request.env.cr.dbname
                    redis_client.incr(f"views:{db_name}:page:{page.id}")
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Redis view counter increment failed in controller: %s", e)
            return request.env['ir.http']._serve_fallback()

        is_owner = False
        if profile_user and profile_user.id == request.env.user.id:
            is_owner = True
        elif profile_group and request.env.user.id in profile_group.member_ids.ids:
            is_owner = True

        main_object = profile_user or profile_group
        values = {
            "profile_user": profile_user,
            "profile_group": profile_group,
            "main_object": main_object,
            "resolved_slug": website_slug,
            "page_type": "home",
            "is_owner": is_owner,
        }
        return request.render("user_websites.placeholder_page", values)


    @http.route("/<string:website_slug>/create_site", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def create_site(self, website_slug, **kwargs):
        user = request.env.user

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        profile_user = env_svc["res.users"].search([("website_slug", "=", website_slug)], limit=1)
        profile_group = env_svc["user.websites.group"].search([("website_slug", "=", website_slug)], limit=1)

        if not profile_user and not profile_group:
            return request.not_found()

        if profile_user and profile_user.id != user.id:
            return request.not_found()
        if profile_group and user.id not in profile_group.member_ids.ids:
            return request.not_found()

        entity_name = profile_user.name if profile_user else profile_group.name

        arch_base = f"""<t name="{entity_name} Home" t-name="user_websites.home_{website_slug}">
            <t t-call="user_websites.template_default_home">
                <div id="wrap" class="oe_structure oe_empty"/>
            </t>
        </t>"""

        create_vals = {
            "url": f"/{website_slug}/home",
            "name": f"{entity_name} Home",
            "type": "qweb",
            "arch_base": arch_base,
            "website_id": request.website.id if getattr(request, 'website', False) else False,
            "website_published": True,
        }
        if profile_user:
            create_vals["owner_user_id"] = profile_user.id
        elif profile_group:
            create_vals["user_websites_group_id"] = profile_group.id

        page = env_svc["website.page"].create(create_vals)
        return request.redirect(page.url)

    @http.route("/<string:website_slug>/create_blog", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def create_blog(self, website_slug, **kwargs):
        user = request.env.user
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        profile_user = env_svc["res.users"].search([("website_slug", "=", website_slug)], limit=1)
        profile_group = env_svc["user.websites.group"].search([("website_slug", "=", website_slug)], limit=1)

        if not profile_user and not profile_group:
            return request.not_found()

        if profile_user and profile_user.id != user.id:
            return request.not_found()
        if profile_group and user.id not in profile_group.member_ids.ids:
            return request.not_found()

        entity_name = profile_user.name if profile_user else profile_group.name

        create_vals = {
            "name": f"{entity_name}'s Blog",
            "website_id": request.website.id if getattr(request, 'website', False) else False,
        }
        if profile_user:
            create_vals["owner_user_id"] = profile_user.id
        elif profile_group:
            create_vals["user_websites_group_id"] = profile_group.id

        env_svc["blog.blog"].create(create_vals)
        return request.redirect(f"/{website_slug}/blog")

    @http.route("/user-websites/documentation", type="http", auth="user", website=True)
    def documentation(self, **kwargs):
        # Tested by [@ANCHOR: user_websites:test_documentation_route]
        try:
            # We explicitly use request.env here instead of env_svc to ensure
            # the current user has the correct portal/public access rights to view the article,
            # avoiding artificial AccessErrors from the backend service account.
            if 'manual.article' in request.env:
                article = request.env['manual.article'].search([('name', 'ilike', 'User Websites Documentation%')], limit=1)
                if article and article.website_url:
                    return request.redirect(article.website_url)
            if 'knowledge.article' in request.env:
                article = request.env['knowledge.article'].search([('name', 'ilike', 'User Websites Documentation%')], limit=1)
                if article and article.website_url:
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
        entries = env_svc["user_websites.public_directory_view"].search([], limit=1000)

        return request.render("user_websites.community_directory", {"pager": pager, "entries": entries})

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
        return request.redirect("/my/home?erased=1")

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
            # Prevent JS Fetch from raising FATAL tracebacks during headless UI Tours
            return request.make_response(json.dumps({"count": 0, "error": "Forbidden"}), status=200, headers=[("Content-Type", "application/json")])

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        count = env_svc["content.violation.report"].search_count([("state", "=", "new")], limit=1000)

        return request.make_response(json.dumps({"count": count}), status=200, headers=[("Content-Type", "application/json")])

    @http.route("/<string:website_slug>/subscribe", type="http", auth="user", methods=["POST"], website=True, csrf=True)
    def subscribe(self, website_slug, **kwargs):
        # [@ANCHOR: UX_SUBSCRIBE]
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        profile_user = env_svc["res.users"].search([("website_slug", "=", website_slug)], limit=1)
        profile_group = env_svc["user.websites.group"].search([("website_slug", "=", website_slug)], limit=1)

        if not profile_user and not profile_group:
            return request.not_found()

        record_id = profile_user.partner_id.id if profile_user else profile_group.id
        model = 'res.partner' if profile_user else 'user.websites.group'

        env_svc[model].browse(record_id).message_subscribe([request.env.user.partner_id.id])
        return request.make_response(json.dumps({"success": True}), headers=[("Content-Type", "application/json")])

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
            try:
                is_valid = record._verify_unsubscribe_token(partner_id, token)
            except AttributeError:
                secret = utils._get_crypto_secret()
                msg = f"{model}-{record_id}-{partner_id}-{timestamp}"
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

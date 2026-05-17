# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import json
import hashlib
import hmac
import os
import traceback
import time
from concurrent.futures import ThreadPoolExecutor
import odoo
import redis
from odoo import http, _
from odoo.http import request, content_disposition
from odoo.tools import consteq
from werkzeug.urls import url_encode, url_parse
import werkzeug
import logging
from odoo.modules.registry import Registry
from ..models.res_users import RESERVED_SLUGS

_logger = logging.getLogger(__name__)

# Bounded executor to prevent OS thread exhaustion DoS
BACKGROUND_EXECUTOR = ThreadPoolExecutor(max_workers=4)

# ADR-0024: Global Connection Pooling for Non-ORM Datastores
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


def _async_redis_incr(db_name, page_id):
    """Quickly update the Redis view counter in the background so we don't hold up the web server."""
    try:
        redis_client.incr(f"views:{db_name}:page:{page_id}")
    except redis.exceptions.RedisError as e:
        _logger.error("Redis operation failed during view increment: %s", e)


def _async_gdpr_erasure(db_name, user_id):
    """Deletes user data in the background so the web server doesn't freeze up during large GDPR requests."""
    registry = Registry(db_name)
    cr = registry.cursor()
    try:
        # ADR-0001: Execute operations under a dedicated service account instead of SUPERUSER_ID
        env = odoo.api.Environment(cr, odoo.SUPERUSER_ID, {})
        try:
            svc_uid = env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )

            user = env["res.users"].with_user(svc_uid).with_context(
                active_test=False, mail_notrack=True
            ).browse(user_id)
            if user.exists():
                user._execute_gdpr_erasure()

                # Anonymize standard PII since Odoo relies heavily on create_uid
                user.write(
                    {
                        "name": f"Anonymized User {user_id}",
                        "login": f"deleted_{user_id}",
                        "email": False,
                        "website_slug": False,
                        "active": False,
                    }
                )
                env.cr.commit()
        except Exception as e: # audit-ignore-catch-all
            env.cr.rollback()
            _logger.exception(f"GDPR Erasure failed for user {user_id}: {e}")
            try:
                # Notifications should fall back to superuser (env) if service account initialization failed
                admin = env.ref("base.user_admin").with_context(active_test=False)
                admin_uid = admin.id
                error_details = traceback.format_exc()
                admin.partner_id.activity_schedule(
                    "mail.mail_activity_data_todo",
                    user_id=admin_uid,
                    summary=f"FAILED GDPR Erasure for User ID {user_id}",
                    note=f"The background GDPR erasure process failed. Exception: {e}<br/><pre>{error_details}</pre>",
                )
                env.cr.commit()
            except Exception as inner_e: # audit-ignore-catch-all
                _logger.critical(
                    f"Failed to notify admin of GDPR erasure failure: {inner_e}"
                )
    finally:
        cr.close()


class UserWebsitesController(http.Controller):

    # --- 1. Community Directory ---
    @http.route(
        ["/community", "/community/page/<int:page>"],
        type="http",
        auth="public",
        website=True,
    )
    def community_directory(self, page=1, **kwargs):
        # [@ANCHOR: UX_COMMUNITY_DIRECTORY]
        # Verified by [@ANCHOR: test_tour_community_directory]
        domain = []
        step = 24

        # Optimize COUNT(*) via Redis with a 5-minute TTL to prevent DB exhaustion on high traffic
        cache_key = "community_directory_total_users"
        total_users = None

        if not odoo.tools.config.get("test_enable"):
            try:
                cached_total = redis_client.get(cache_key)
                if cached_total is not None:
                    total_users = int(cached_total)
            except redis.exceptions.RedisError as e:
                _logger.error(
                    "Redis operation failed during community directory cache lookup: %s",
                    e,
                )

        if total_users is None:
            total_users = request.env[
                "user_websites.public.directory.view"
            ].search_count(domain)
            if not odoo.tools.config.get("test_enable"):
                try:
                    redis_client.setex(cache_key, 300, total_users)
                except redis.exceptions.RedisError as e:
                    _logger.error(
                        "Redis operation failed during community directory cache set: %s",
                        e,
                    )

        users = request.env["user_websites.public.directory.view"].search(
            domain, limit=step, offset=(page - 1) * step
        )

        pager = request.website.pager(
            url="/community",
            total=total_users,
            page=page,
            step=step,
        )

        return request.render(
            "user_websites.community_directory",
            {
                "users": users,
                "pager": pager,
                "default_title": "Community Directory",
                "default_description": "Discover the personal websites and blogs created by our community members.",
            },
        )

    # --- 2. Abuse Reporting ---
    @http.route(
        "/website/report_violation",
        type="http",
        auth="public",
        methods=["POST"],
        website=True,
    )
    def submit_violation_report(
        self, url="", description="", email="", website_honeypot="", **kwargs
    ):
        # [@ANCHOR: UX_REPORT_VIOLATION]
        # Verified by [@ANCHOR: test_tour_violation_report]
        url = url.strip()[:2000]
        description = description.strip()[:5000]

        referrer = request.httprequest.referrer or "/"
        parsed_referrer = url_parse(referrer)
        safe_redirect = (
            parsed_referrer.path if parsed_referrer.path.startswith("/") else "/"
        )

        # --- Anti-Spam Honeypot Enforcement ---
        honeypot = website_honeypot.strip()
        if honeypot:
            _logger.info(
                "Spam bot detected and blocked by honeypot on violation report endpoint."
            )
            # Silent fail: Redirect back as if it worked to confuse automated bots
            separator = "&" if "?" in safe_redirect else "?"
            return request.redirect(f"{safe_redirect}{separator}report_submitted=1")

        if not url or not description:
            return request.redirect(safe_redirect)

        is_public = request.env.user._is_public()
        user_id = False if is_public else request.env.user.id

        email = email.strip()[:255]
        if not is_public and request.env.user.email:
            email = request.env.user.email
        elif not email:
            email = "Anonymous"

        content_owner_id = False
        content_group_id = False

        route = request.env["user_websites.content.routing.view"].search(
            [("target_url", "=", url)], limit=1
        )
        if route:
            if route.content_owner_id:
                content_owner_id = route.content_owner_id.id
            elif route.content_group_id:
                content_group_id = route.content_group_id.id

        request.env["content.violation.report"].create(
            {
                "target_url": url,
                "description": description,
                "reported_by_user_id": user_id,
                "reported_by_email": email,
                "content_owner_id": content_owner_id,
                "content_group_id": content_group_id,
            }
        )

        separator = "&" if "?" in safe_redirect else "?"
        return request.redirect(f"{safe_redirect}{separator}report_submitted=1")

    # --- 3. Home Page Routing, Caching & View Tracking ---
    @http.route(
        [
            "/<string:website_slug>",
            "/<string:website_slug>/home",
            "/<string:website_slug>/home/",
            "/<string:website_slug>/<path:page_path>",
        ],
        type="http",
        auth="public",
        website=True,
    )
    def user_websites_home(self, website_slug, page_path=None, **kwargs):
        # Prevent accessing reserved routes via path routing
        if page_path and page_path.split("/")[0] in RESERVED_SLUGS:
            raise werkzeug.exceptions.NotFound()
        # [@ANCHOR: controller_user_websites_home]
        # Verified by [@ANCHOR: test_tour_create_site]
        # Verified by [@ANCHOR: test_group_site_routing]
        slug_lower = website_slug.lower()
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        user_id = request.env["res.users"]._get_user_id_by_slug(slug_lower)
        user = (
            request.env["res.users"].with_user(svc_uid).browse(user_id)
            if user_id
            else None
        )

        group = None
        if not user:
            group_id = request.env["user.websites.group"]._get_group_id_by_slug(
                slug_lower
            )
            group = (
                request.env["user.websites.group"].with_user(svc_uid).browse(group_id)
                if group_id
                else None
            )

        if user:
            if getattr(user, "is_suspended_from_websites", False):
                raise werkzeug.exceptions.NotFound()
            website_id = (
                request.website.id
                if hasattr(request, "website") and request.website
                else False
            )
            target_url = f"/{user.website_slug}/{page_path}" if page_path else f"/{user.website_slug}/home"
            page_id = request.env["website.page"]._get_page_id_by_url(
                target_url, website_id
            )
            page = (
                request.env["website.page"].with_user(svc_uid).browse(page_id)
                if page_id
                else None
            )

            if page and page.exists() and page.website_published:
                if not odoo.tools.config.get("test_enable"):
                    # RACE CONDITION FIX: Removed threading.Thread() to prevent OS thread exhaustion DoS.
                    # Redis INCR is O(1) and executes in microseconds, making async offloading dangerous and unnecessary here.
                    _async_redis_incr(request.env.cr.dbname, page.id)
                else:
                    svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                        "user_websites.user_user_websites_service_account"
                    )
                    page.with_user(svc_uid).write({"view_count": page.view_count + 1})

                # Retrieve avatar for OpenGraph og:image if available
                ts = int(user.write_date.timestamp()) if user.write_date else 0
                avatar_url = f"/web/image/res.users/{user.id}/avatar_128?unique={ts}"

                response = request.render(
                    page.view_id.id,
                    {
                        "main_object": page,
                        "profile_user": user,
                        "is_owner": request.env.user.id == user.id,
                        "default_title": f"{user.name}'s Homepage",
                        "default_description": f"Welcome to the personal site of {user.name}.",
                        "default_image": avatar_url,
                        "resolved_slug": user.website_slug,
                    },
                )

                if request.env.user._is_public():
                    response.headers["Cache-Control"] = "public, max-age=60"
                    response.headers["Cloudflare-CDN-Cache-Control"] = "max-age=604800"
                    response.headers["Cache-Tag"] = f"site-{user.website_slug or slug_lower}"
                return response

            return request.render(
                "user_websites.placeholder_page",
                {
                    "profile_user": user,
                    "profile_group": None,
                    "is_owner": request.env.user.id == user.id,
                    "page_type": "home",
                    "resolved_slug": user.website_slug or slug_lower,
                },
            )

        # Fallback to Groups
        if group:
            website_id = (
                request.website.id
                if hasattr(request, "website") and request.website
                else False
            )
            target_url = f"/{group.website_slug}/{page_path}" if page_path else f"/{group.website_slug}/home"
            page_id = request.env["website.page"]._get_page_id_by_url(
                target_url, website_id
            )
            page = (
                request.env["website.page"].with_user(svc_uid).browse(page_id)
                if page_id
                else None
            )

            if page and page.exists() and page.website_published:
                if not odoo.tools.config.get("test_enable"):
                    _async_redis_incr(request.env.cr.dbname, page.id)
                else:
                    page.write({"view_count": page.view_count + 1})

                is_member = request.env.user.id in group.odoo_group_id.user_ids.ids
                response = request.render(
                    page.view_id.id,
                    {
                        "main_object": page,
                        "profile_group": group,
                        "is_owner": is_member,
                        "default_title": f"{group.name} Homepage",
                        "default_description": f"Welcome to the official page of {group.name}.",
                        "resolved_slug": group.website_slug,
                    },
                )

                if request.env.user._is_public():
                    response.headers["Cache-Control"] = "public, max-age=60"
                    response.headers["Cloudflare-CDN-Cache-Control"] = "max-age=604800"
                    response.headers["Cache-Tag"] = f"site-{group.website_slug}"
                return response

            return request.render(
                "user_websites.placeholder_page",
                {
                    "profile_user": None,
                    "profile_group": group,
                    "is_owner": request.env.user.id in group.odoo_group_id.user_ids.ids,
                    "page_type": "home",
                    "resolved_slug": group.website_slug,
                },
            )

        raise werkzeug.exceptions.NotFound()

    # --- 4. Site Creation ---
    @http.route(
        ["/<string:website_slug>/create_site"],
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
    )
    def create_site(self, website_slug, **kwargs):
        # [@ANCHOR: UX_CREATE_SITE]
        # Verified by [@ANCHOR: test_tour_create_site]
        # Verified by [@ANCHOR: test_group_site_creation]
        slug_lower = website_slug.lower()
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        user_id = request.env["res.users"]._get_user_id_by_slug(slug_lower)
        user = (
            request.env["res.users"].with_user(svc_uid).browse(user_id)
            if user_id
            else None
        )

        group = None
        if not user:
            group_id = request.env["user.websites.group"]._get_group_id_by_slug(
                slug_lower
            )
            group = (
                request.env["user.websites.group"].with_user(svc_uid).browse(group_id)
                if group_id
                else None
            )

        target_uid = request.env.user.id
        resolved_slug = None

        if user:
            if user.id != request.env.user.id:
                raise werkzeug.exceptions.Forbidden(
                    description=_("You do not have permission to create this site.")
                )

            # If they are using a Virtual Slug, lock it in permanently before creating the page
            if not request.env.user.website_slug:
                request.env.user.write({"website_slug": slug_lower})

            resolved_slug = request.env.user.website_slug
        elif group:
            if request.env.user.id not in group.odoo_group_id.user_ids.ids:
                raise werkzeug.exceptions.Forbidden(
                    description=_("You do not have permission to create this site.")
                )
            resolved_slug = group.website_slug
        else:
            raise werkzeug.exceptions.NotFound()

        # RACE CONDITION FIX: Enforce an exclusive transaction lock keyed to the target slug
        # preventing multiple group members from bypassing the search_count simultaneously.
        lock_hash = request.env["zero_sudo.security.utils"]._get_deterministic_hash(
            resolved_slug
        )
        request.env.cr.execute("SELECT pg_advisory_xact_lock(%s)", (lock_hash,))

        # Make sure we don't accidentally create duplicate pages if the user clicks twice
        # Make sure we don't accidentally create duplicate pages if the user clicks twice
        existing_page = request.env["website.page"].search_count(
            [("url", "=", f"/{resolved_slug}/home")]
        )
        if existing_page > 0:
            return request.redirect(f"/{resolved_slug}/home")

        view_xml_id = "user_websites.template_default_home"
        unique_key = f"user_websites.home_{resolved_slug}"

        template_view = request.env.ref(view_xml_id)

        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        page_vals = {
            "url": f"/{resolved_slug}/home",
            "name": f"{user.name if user else group.name} Home",
            "is_published": True,
            "website_published": True,
            "type": "qweb",
            "website_id": (
                request.website.id
                if hasattr(request, "website") and getattr(request, "website", False)
                else request.env["website"].get_current_website().id
            ),
            "key": unique_key,
            "arch": template_view.with_user(svc_uid).arch,
        }

        if user:
            page_vals["owner_user_id"] = target_uid
        elif group:
            page_vals["user_websites_group_id"] = group.id

        request.env["website.page"].create(page_vals)

        return request.redirect(f"/{resolved_slug}/home")

    # --- 5. Blog Routing ---
    @http.route(
        ["/<string:website_slug>/blog", "/<string:website_slug>/blog/"],
        type="http",
        auth="public",
        website=True,
    )
    def user_blog_index(
        self,
        website_slug,
        tag=None,
        search=None,
        date_begin=None,
        date_end=None,
        page=1,
        **kwargs,
    ):
        # [@ANCHOR: controller_user_blog_index]
        # Verified by [@ANCHOR: test_tour_create_blog]
        slug_lower = website_slug.lower()
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        user_id = request.env["res.users"]._get_user_id_by_slug(slug_lower)
        user = (
            request.env["res.users"].with_user(svc_uid).browse(user_id)
            if user_id
            else None
        )

        group = None
        if not user:
            group_id = request.env["user.websites.group"]._get_group_id_by_slug(
                slug_lower
            )
            group = (
                request.env["user.websites.group"].with_user(svc_uid).browse(group_id)
                if group_id
                else None
            )

        if not user and not group:
            raise werkzeug.exceptions.NotFound()

        domain = [
            ("is_published", "=", True),
            "|",
            ("website_id", "=", False),
            ("website_id", "=", request.website.id),
        ]

        resolved_slug = (
            (user.website_slug or slug_lower) if user else group.website_slug
        )

        blog_domain = [
            "|",
            ("website_id", "=", False),
            ("website_id", "=", request.website.id),
        ]

        if user:
            if getattr(user, "is_suspended_from_websites", False):
                raise werkzeug.exceptions.NotFound()
            domain.append(("owner_user_id", "=", user.id))
            blog_domain.append(("owner_user_id", "=", user.id))
            main_object = user
            meta_title = f"{user.name}'s Blog"
        else:
            domain.append(("user_websites_group_id", "=", group.id))
            blog_domain.append(("user_websites_group_id", "=", group.id))
            main_object = group
            meta_title = f"{group.name}'s Blog"

        page_num = int(page)
        step = 10
        total_posts = request.env["blog.post"].search_count(domain)

        if total_posts == 0:
            return request.render(
                "user_websites.placeholder_page",
                {
                    "profile_user": user,
                    "profile_group": group,
                    "is_owner": (user and request.env.user.id == user.id)
                    or (
                        group
                        and request.env.user.id in group.odoo_group_id.user_ids.ids
                    ),
                    "page_type": "blog",
                    "resolved_slug": resolved_slug,
                },
            )

        posts = request.env["blog.post"].search(
            domain, limit=step, offset=(page_num - 1) * step
        )

        blogs = request.env["blog.blog"].search(blog_domain, limit=1)

        def blog_url(tag=None, date_begin=None, date_end=None, search=None):
            url = request.httprequest.path
            params = request.httprequest.args.to_dict()
            if search is not None:
                params["search"] = search
            if tag is not None:
                params["tag"] = tag
            if date_begin is not None:
                params["date_begin"] = date_begin
            if date_end is not None:
                params["date_end"] = date_end

            params = {k: v for k, v in params.items() if v}
            if params:
                return f"{url}?{url_encode(params)}"
            return url

        pager = request.website.pager(
            url=f"/{resolved_slug}/blog", total=total_posts, page=page_num, step=step
        )

        response = request.render(
            "website_blog.blog_post_short",
            {
                "posts": posts,
                "blog": (
                    (posts[0].blog_id if posts else blogs[0])
                    if (posts or blogs)
                    else False
                ),
                "blogs": blogs,
                "main_object": main_object,
                "profile_user": user,
                "profile_group": group,
                "blog_url": blog_url,
                "tag": tag,
                "search": search,
                "pager": pager,
                "default_title": meta_title,
                "default_description": f"Read the latest updates and posts on {meta_title}.",
                "resolved_slug": resolved_slug,
            },
        )

        if request.env.user._is_public():
            response.headers["Cache-Control"] = "public, max-age=60"
            response.headers["Cloudflare-CDN-Cache-Control"] = "max-age=604800"
            response.headers["Cache-Tag"] = f"site-{resolved_slug}"
        return response

    # --- 6. Blog Creation ---
    @http.route(
        ["/<string:website_slug>/create_blog"],
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
    )
    def create_blog_post(self, website_slug, **kwargs):
        # [@ANCHOR: UX_CREATE_BLOG_POST]
        # Verified by [@ANCHOR: test_tour_create_blog]
        # Verified by [@ANCHOR: test_group_blog_post_creation]
        slug_lower = website_slug.lower()
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        user_id = request.env["res.users"]._get_user_id_by_slug(slug_lower)
        user = (
            request.env["res.users"].with_user(svc_uid).browse(user_id)
            if user_id
            else None
        )

        group = None
        if not user:
            group_id = request.env["user.websites.group"]._get_group_id_by_slug(
                slug_lower
            )
            group = (
                request.env["user.websites.group"].with_user(svc_uid).browse(group_id)
                if group_id
                else None
            )

        resolved_slug = None

        if user:
            if user.id != request.env.user.id:
                raise werkzeug.exceptions.Forbidden(
                    description=_("You cannot create posts for this user.")
                )

            # If they are using a Virtual Slug, lock it in permanently before creating the post
            if not request.env.user.website_slug:
                request.env.user.write({"website_slug": slug_lower})

            resolved_slug = request.env.user.website_slug
        elif group:
            if request.env.user.id not in group.odoo_group_id.user_ids.ids:
                raise werkzeug.exceptions.Forbidden(
                    description=_(
                        "You do not have permission to create posts for this group."
                    )
                )
            resolved_slug = group.website_slug
        else:
            raise werkzeug.exceptions.NotFound()

        # RACE CONDITION FIX: Enforce an exclusive transaction lock keyed to the target slug
        # preventing multiple group members from duplicating the initial blog record.
        lock_hash = request.env["zero_sudo.security.utils"]._get_deterministic_hash(
            "blog_" + resolved_slug
        )
        request.env.cr.execute("SELECT pg_advisory_xact_lock(%s)", (lock_hash,))

        blog_domain = [
            "|",
            ("website_id", "=", False),
            ("website_id", "=", request.website.id),
        ]
        if user:
            blog_domain.append(("owner_user_id", "=", user.id))
        elif group:
            blog_domain.append(("user_websites_group_id", "=", group.id))

        blog = request.env["blog.blog"].search(blog_domain, limit=1)

        if not blog:
            svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )
            blog_vals = {
                "name": f"{user.name}'s Blog" if user else f"{group.name} Blog",
                "website_id": request.website.id,
            }
            if user:
                blog_vals["owner_user_id"] = user.id
            elif group:
                blog_vals["user_websites_group_id"] = group.id

            blog = request.env["blog.blog"].with_user(svc_uid).create(blog_vals)

        # Make sure we don't accidentally create duplicate posts if the user clicks twice
        domain = [("blog_id", "=", blog.id), ("is_published", "=", True)]
        if user:
            domain.append(("owner_user_id", "=", request.env.user.id))
        elif group:
            domain.append(("user_websites_group_id", "=", group.id))

        existing_post = request.env["blog.post"].search_count(domain)
        if existing_post > 0:
            return request.redirect(f"/{resolved_slug}/blog")

        post_vals = {
            "name": (
                "Welcome to my Blog" if user else f"Welcome to the {group.name} Blog"
            ),
            "blog_id": blog.id,
            "is_published": True,
            "website_id": request.website.id,
            "content": "<p>This is my first post!</p>",
        }

        if user:
            post_vals["owner_user_id"] = request.env.user.id
        elif group:
            post_vals["user_websites_group_id"] = group.id

        request.env["blog.post"].create(post_vals)

        return request.redirect(f"/{resolved_slug}/blog")

    # --- 7. Documentation ---
    @http.route("/user-websites/documentation", type="http", auth="user", website=True)
    def user_websites_documentation(self, **kwargs):
        # [@ANCHOR: controller_user_websites_documentation]
        # Verified by [@ANCHOR: test_documentation_route]
        article_model_name = None
        if "knowledge.article" in request.env:
            article_model_name = "knowledge.article"
        elif "manual.article" in request.env:
            article_model_name = "manual.article"

        if article_model_name:
            svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.odoo_facility_service_internal"
            )
            article = request.env[article_model_name].with_user(svc_uid).search(
                [("name", "=", "User Websites Documentation")], limit=1
            )
            if article and hasattr(article, "website_url") and article.website_url:
                return request.redirect(article.website_url)

        return request.render("user_websites.documentation_page", {})

    # --- 8. Moderation Appeals ---
    @http.route(
        "/website/submit_appeal",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
    )
    def submit_appeal(self, reason="", **kwargs):
        # [@ANCHOR: UX_SUBMIT_APPEAL]
        # Verified by [@ANCHOR: test_tour_moderation_appeal]
        reason = reason.strip()[:5000]
        user = request.env.user

        if user.is_suspended_from_websites and reason:
            # RACE CONDITION FIX: Prevent spamming multiple appeals via concurrent POST requests
            request.env.cr.execute("SELECT pg_advisory_xact_lock(%s)", (user.id,))

            svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )
            existing = (
                request.env["content.violation.appeal"]
                .with_user(svc_uid)
                .search([("user_id", "=", user.id), ("state", "=", "new")], limit=1)
            )
            if not existing:
                request.env["content.violation.appeal"].create(
                    {"user_id": user.id, "reason": reason}
                )

        return request.redirect("/my/home?appeal_submitted=1")

    # --- 9. Subscriptions & Unsubscribes ---
    @http.route(
        "/<string:website_slug>/subscribe",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
    )
    def subscribe_to_site(self, website_slug, **kwargs):
        # [@ANCHOR: UX_SUBSCRIBE]
        # Verified by [@ANCHOR: test_subscription_creation]
        # Verified by [@ANCHOR: test_subscribe_to_site]
        slug_lower = website_slug.lower()
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        user_id = request.env["res.users"]._get_user_id_by_slug(slug_lower)
        user = (
            request.env["res.users"].with_user(svc_uid).browse(user_id)
            if user_id
            else None
        )

        group = None
        if not user:
            group_id = request.env["user.websites.group"]._get_group_id_by_slug(
                slug_lower
            )
            group = (
                request.env["user.websites.group"].with_user(svc_uid).browse(group_id)
                if group_id
                else None
            )

        target_record = user.partner_id if user else group
        if target_record:
            svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )
            target_record.with_user(svc_uid).message_subscribe(
                partner_ids=[request.env.user.partner_id.id]
            )

        referrer = request.httprequest.referrer or "/"
        return request.redirect(f"{referrer}?subscribed=1")

    @http.route(
        "/website/unsubscribe/<string:model_name>/<int:record_id>/<int:partner_id>/<int:timestamp>/<string:token>",
        type="http",
        auth="public",
        website=True,
    )
    def unsubscribe_digest(
        self, model_name, record_id, partner_id, timestamp, token, **kwargs
    ):
        # [@ANCHOR: controller_unsubscribe_digest]
        # Verified by [@ANCHOR: test_unsubscribe_secret]
        if model_name not in ["res.partner", "user.websites.group"]:
            raise werkzeug.exceptions.NotFound()


        current_time = int(time.time())
        # ADR-0025: Enforce a strict 30-day TTL on the stateless token
        thirty_days_in_seconds = 30 * 24 * 60 * 60
        if current_time - timestamp > thirty_days_in_seconds:
            raise werkzeug.exceptions.Forbidden("This unsubscribe link has expired.")

        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        record = request.env[model_name].with_user(svc_uid).browse(record_id)
        if not record.exists():
            raise werkzeug.exceptions.NotFound()

        db_secret = request.env["zero_sudo.security.utils"]._get_crypto_secret()
        if not db_secret:
            _logger.error(
                "Security Alert: Crypto secret is not configured. Unsubscribe tokens cannot be verified."
            )
            raise werkzeug.exceptions.InternalServerError(
                "System configuration error: cryptographic secret missing."
            )

        message = f"{model_name}-{record_id}-{partner_id}-{timestamp}".encode("utf-8")
        expected_token = hmac.new(
            db_secret.encode("utf-8"), message, hashlib.sha256
        ).hexdigest()

        if not consteq(token, expected_token):
            raise werkzeug.exceptions.Forbidden()

        record.with_user(svc_uid).message_unsubscribe(partner_ids=[partner_id])

        return request.render(
            "user_websites.unsubscribe_success", {"record_name": record.name}
        )

    @http.route(
        "/api/v1/user_websites/pending_reports",
        type="http",
        auth="public",
        methods=["GET"],
        website=True,
    )
    def api_pending_reports(self, **kwargs):
        # [@ANCHOR: api_pending_reports]
        # Verified by [@ANCHOR: test_admin_violation_toast_rpc]
        if not request.env.user.has_group(
            "user_websites.group_user_websites_administrator"
        ):
            raise werkzeug.exceptions.Forbidden()

        count = request.env["content.violation.report"].search_count(
            [("state", "=", "new")]
        )
        return request.make_response(
            json.dumps({"count": count}), headers=[("Content-Type", "application/json")]
        )

    # --- 10. GDPR Privacy & Data Subject Access ---
    @http.route(["/my/privacy"], type="http", auth="user", website=True)
    def my_privacy_dashboard(self, **kwargs):
        # [@ANCHOR: controller_my_privacy_dashboard]
        # Verified by [@ANCHOR: test_tour_gdpr_privacy]
        """Renders the frontend portal dashboard for data portability and right to erasure."""
        return request.render(
            "user_websites.portal_my_privacy", {"default_title": "My Privacy Dashboard"}
        )

    @http.route(["/my/privacy/export"], type="http", auth="user", website=True)
    def export_user_data(self, **kwargs):
        # [@ANCHOR: UX_GDPR_EXPORT]
        # Verified by [@ANCHOR: test_gdpr_export_api]
        # Verified by [@ANCHOR: test_tour_gdpr_privacy]
        """
        Compiles user generated content into a machine-readable JSON format for data portability.
        Utilizes ADR-0022 Streaming Generators to prevent OOM crashes on massive exports.
        """
        user = request.env.user

        base_data = user._get_gdpr_export_data()
        streamed_keys = getattr(user, "_get_gdpr_streamed_keys", lambda: {})()

        def generate_json_stream():
            yield "{\n"
            first_key = True

            for key, val in base_data.items():
                if not first_key:
                    yield ",\n"
                yield f'  "{key}": {json.dumps(val)}'
                first_key = False

            for key, generator_func in streamed_keys.items():
                if not first_key:
                    yield ",\n"
                yield f'  "{key}": [\n'
                first_item = True
                for item in generator_func():
                    if not first_item:
                        yield ",\n"
                    yield f"    {json.dumps(item)}"
                    first_item = False
                yield "\n  ]"
                first_key = False

            yield "\n}"

        headers = {
            "Content-Disposition": content_disposition(f"{user.website_slug}_data_export.json"),
        }
        return werkzeug.wrappers.Response(
            generate_json_stream(),
            headers=headers,
            content_type="application/json"
        )

    @http.route(
        ["/my/privacy/delete_content"],
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
    )
    def delete_user_content(self, **kwargs):
        # [@ANCHOR: UX_GDPR_ERASURE]
        # Verified by [@ANCHOR: test_gdpr_erasure_pages]
        # Verified by [@ANCHOR: test_tour_gdpr_privacy]
        """Fulfills the 'Right to Erasure' by permanently unlinking all owned content in the background."""
        user_id = request.env.user.id
        db_name = request.env.cr.dbname

        if odoo.tools.config.get("test_enable"):
            user = (
                request.env["res.users"].with_context(active_test=False).browse(user_id)
            )
            user._execute_gdpr_erasure()
            svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )
            user.with_user(svc_uid).write(
                {
                    "name": f"Anonymized User {user_id}",
                    "login": f"deleted_{user_id}",
                    "email": False,
                    "website_slug": False,
                    "active": False,
                }
            )
        else:
            BACKGROUND_EXECUTOR.submit(_async_gdpr_erasure, db_name, user_id)

        request.session.logout()
        return request.redirect("/web/login?erased=1")

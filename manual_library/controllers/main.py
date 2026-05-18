# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
from odoo.exceptions import AccessError, UserError
import werkzeug.exceptions
import logging
from werkzeug.urls import url_parse

_logger = logging.getLogger(__name__)


class ManualLibraryController(http.Controller):

    def _get_sidebar_articles(self):
        """Helper to fetch and group root articles for the sidebar."""
        # Domain-based search is more efficient than .filtered() for large datasets
        base_domain = [("parent_id", "=", False)]

        workspace_articles = request.env["knowledge.article"].search(
            base_domain + [("internal_permission", "in", ("read", "write"))], limit=5000
        )

        # Shared articles: permission is 'none' but user is in member_ids
        shared_articles = request.env["knowledge.article"].search(
            base_domain + [
                ("internal_permission", "=", "none"),
                ("member_ids", "in", [request.env.user.id])
            ], limit=5000
        )

        # Private articles: permission is 'none', user is creator, and no other members
        private_articles = request.env["knowledge.article"].search(
            base_domain + [
                ("internal_permission", "=", "none"),
                ("create_uid", "=", request.env.user.id),
                ("member_ids", "=", False)
            ], limit=5000
        )

        return workspace_articles, shared_articles, private_articles

    @http.route(
        ["/manual", "/manual/<string:article_slug>"],
        type="http",
        auth="public",
        website=True,
    )
    def manual_article_view(self, article_slug=None, **kwargs):
        # [@ANCHOR: controller_manual_article_view]
        # See [@ANCHOR: story_article_view] and [@ANCHOR: journey_user_browsing]
        # Verified by [@ANCHOR: test_controller_manual_article_view]
        """
        Public/Frontend controller to render articles.
        Enforces access securely through the ORM environment.
        """
        article = None

        # 1. Manually extract the ID and fetch the record to allow ORM rules to handle visibility
        if article_slug:
            try:
                article_id = int(article_slug.split("-")[0])
                article = request.env["knowledge.article"].browse(article_id)
                if not article.exists() or not article.active:
                    raise werkzeug.exceptions.NotFound()
            except (ValueError, IndexError, AccessError, UserError):
                raise werkzeug.exceptions.NotFound()

        # 2. Fetch root articles for the sidebar navigation
        workspace_articles, shared_articles, private_articles = self._get_sidebar_articles()

        # 3. If no specific article is requested, default to the first available root article
        if not article:
            if workspace_articles:
                article = workspace_articles[0]
            elif shared_articles:
                article = shared_articles[0]
            elif private_articles:
                article = private_articles[0]

        # 4. Handle 404s gracefully
        if not article or not article.exists():
            raise werkzeug.exceptions.NotFound()

        # 5. Enforce Read Context (Public/Guest)
        try:
            article.check_access("read")
            _ = article.name
        except AccessError:
            raise werkzeug.exceptions.NotFound()

        # 6. Canonical Redirect for SEO: ensure the slug matches current name
        if article_slug and article.website_url != request.httprequest.path:
            return request.redirect(article.website_url, code=301)

        # 7. Render standard QWeb response
        return request.render(
            "manual_library.article_template",
            {
                "main_object": article,
                "article": article,
                "workspace_articles": workspace_articles,
                "shared_articles": shared_articles,
                "private_articles": private_articles,
                "search_term": "",
                "is_internal_user": request.env.user.has_group("base.group_user") or request.env.user.has_group("base.group_portal"),
            },
        )

    @http.route(["/manual/search"], type="http", auth="public", website=True)
    def manual_search(self, search="", **kwargs):
        # [@ANCHOR: controller_manual_search]
        # Verified by [@ANCHOR: test_tour_manual_search]
        # See story_manual_search and journey_user_browsing
        """
        Provides full-text search across accessible articles.
        """
        domain = []
        if search:
            domain += ["|", ("name", "ilike", search), ("body", "ilike", search)]

        # Removed .sudo() to allow native Record Rules to filter visibility by user persona
        articles = request.env["knowledge.article"].search(domain, limit=1000)

        # Fetch and group root articles for the sidebar navigation
        workspace_articles, shared_articles, private_articles = self._get_sidebar_articles()

        return request.render(
            "manual_library.search_results_template",
            {
                "articles": articles,
                "search_term": search,
                "workspace_articles": workspace_articles,
                "shared_articles": shared_articles,
                "private_articles": private_articles,
                "is_internal_user": request.env.user.has_group("base.group_user") or request.env.user.has_group("base.group_portal"),
            },
        )

    @http.route(
        ["/manual/feedback"], type="http", auth="public", methods=["POST"], website=True
    )
    def manual_feedback(
        self, article_id, is_helpful, website_feedback_honeypot=None, **kwargs
    ):
        # [@ANCHOR: controller_manual_feedback]
        # Verified by [@ANCHOR: test_tour_manual_feedback]
        # See story_manual_feedback and journey_user_browsing
        """
        Handles article helpfulness ratings via Service Account isolation.
        """
        # --- ANTI-SPAM: Honeypot Check ---
        if website_feedback_honeypot:
            referer = request.httprequest.referrer or "/manual"
            return request.redirect(referer)

        try:
            # Fetch without sudo() first to ensure the user actually has Read access to the article
            article = request.env["knowledge.article"].browse(int(article_id))
            if article.exists():
                # Enforce access check to prevent voting on articles the user can't read
                article.check_access("read")

                # Escalate to Service Account for atomic increment
                svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                    "manual_library.user_manual_library_service_account"
                )
                svc_env = request.env(user=svc_uid)

                # Utilize raw SQL to ensure absolute atomic increments and prevent 'Lost Update' race conditions.
                # Identifiers are hardcoded for security as they are not user-controlled.
                if is_helpful == "1":
                    svc_env.cr.execute(
                        'UPDATE "knowledge_article" SET "helpful_count" = COALESCE("helpful_count", 0) + 1 WHERE "id" = %s',
                        (article.id,),
                    )
                else:
                    svc_env.cr.execute(
                        'UPDATE "knowledge_article" SET "unhelpful_count" = COALESCE("unhelpful_count", 0) + 1 WHERE "id" = %s',
                        (article.id,),
                    )
        except (ValueError, AccessError):
            # Silently fail on bad input or unauthorized access to prevent brute-force discovery
            pass

        # Protect against open redirects by enforcing local paths
        referer = request.httprequest.referrer or "/manual"
        parsed_referrer = url_parse(referer)
        safe_redirect = (
            parsed_referrer.path if parsed_referrer.path.startswith("/") else "/manual"
        )

        separator = "&" if "?" in safe_redirect else "?"
        return request.redirect(f"{safe_redirect}{separator}feedback_submitted=1")

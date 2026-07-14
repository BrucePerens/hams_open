# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
from odoo.exceptions import AccessError, UserError
from odoo.tools import html2plaintext
import werkzeug.exceptions
import logging
import re
import markdown
from werkzeug.urls import url_parse
from markupsafe import Markup
from odoo.tools import lru

_logger = logging.getLogger(__name__)

_md_cache = lru.LRU(1024)

class ManualLibraryController(http.Controller):

    def _compile_markdown(self, html_body, article_id=None, write_date=None):
        """
        Heuristic detection and compilation of Markdown from Odoo's HTML fields.
        Odoo's WYSIWYG editor wraps pasted text in <p> tags and <br/>.
        html2plaintext safely extracts the raw text to recover the Markdown formatting.
        """
        if not html_body:
            return Markup("")

        cache_key = None
        if article_id and write_date:
            cache_key = f"{article_id}_{write_date}"
            if cache_key in _md_cache:
                return _md_cache[cache_key]

        text_content = html2plaintext(html_body)

        # Heuristic: Look for common Markdown signatures
        markdown_signatures = [
            r"(?m)^#{1,6}\s+.+",  # Headers
            r"(?m)^```[\s\S]*?^```",  # Code blocks
            r"(?m)^(\*|-|\d+\.)\s+",  # Lists
            r"\*\*.*?\*\*",  # Bold
            r"\[.+?\]\(.+?\)",  # Links
        ]

        is_md = any(re.search(sig, text_content) for sig in markdown_signatures)

        # Avoid compiling if it contains complex native Odoo UI snippets (tables, deep divs)
        has_complex_html = bool(
            re.search(
                r"<(div|table|section|article|img)[^>]*>", html_body, re.IGNORECASE
            )
        )

        if is_md and not has_complex_html:
            try:
                md_html = markdown.markdown(
                    text_content, extensions=["fenced_code", "tables", "nl2br", "toc"]
                )
                res = Markup(md_html)
                if cache_key:
                    _md_cache[cache_key] = res
                return res
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Markdown compilation failed: %s", e)

        res = Markup(html_body)
        if cache_key:
            _md_cache[cache_key] = res
        return res

    def _get_sidebar_articles(self):
        """Helper to fetch and group root articles for the sidebar."""
        # [@ANCHOR: manual_sidebar_search_optimization]
        # Performance: Reducing 3 RPC/DB round-trips to 1 by using a combined domain.
        user_id = request.env.user.id
        base_domain = [
            ("parent_id", "=", False),
            ("website_id", "in", (False, request.website.id)),
        ]
        
        is_internal = request.env.user.has_group("base.group_user")
        if not is_internal:
            base_domain.append(("is_published", "=", True))

        # Combined domain to fetch all relevant root articles in one go
        combined_domain = base_domain + [
            "|",
            ("internal_permission", "in", ("read", "write")),
            "|",
            ("member_ids", "in", [user_id]),
            "&",
            ("internal_permission", "=", "none"),
            ("create_uid", "=", user_id),
        ]

        all_roots = request.env["knowledge.article"].search(combined_domain, limit=5000)

        workspace_articles = all_roots.filtered(
            lambda a: a.internal_permission in ("read", "write")
        )
        shared_articles = all_roots.filtered(
            lambda a: a.internal_permission == "none" and user_id in a.member_ids.ids
        )
        private_articles = all_roots.filtered(
            lambda a: a.internal_permission == "none"
            and a.create_uid.id == user_id
            and not a.member_ids
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
        workspace_articles, shared_articles, private_articles = (
            self._get_sidebar_articles()
        )

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
            # Re-browse with the explicit user environment to trigger record rules
            user_article = (
                request.env["knowledge.article"]
                .with_user(request.env.user)
                .browse(article.id)
            )
            user_article.check_access("read")
            _ = user_article.name
        except AccessError:
            raise werkzeug.exceptions.NotFound()

        # Enforce explicitly that non-internal users cannot view unpublished articles
        is_internal = request.env.user.has_group("base.group_user")
        if not is_internal and not user_article.is_published:
            raise werkzeug.exceptions.NotFound()

        # 6. Canonical Redirect for SEO: ensure the slug matches current name
        if article_slug and article.website_url != request.httprequest.path:
            return request.redirect(article.website_url, code=301)

        # 7. Compile Markdown if detected
        compiled_body = self._compile_markdown(article.body, article.id, article.write_date)

        # 8. Render standard QWeb response
        return request.render(
            "knowledge.article_template",
            {
                "main_object": article,
                "article": article,
                "compiled_body": compiled_body,
                "workspace_articles": workspace_articles,
                "shared_articles": shared_articles,
                "private_articles": private_articles,
                "search_term": "",
                "is_internal_user": is_internal
                or request.env.user.has_group("base.group_portal"),
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

        # Explicitly filter by website_id in case record rules are not enough for frontend context
        domain += [("website_id", "in", (False, request.website.id))]
        
        is_internal = request.env.user.has_group("base.group_user")
        if not is_internal:
            domain += [("is_published", "=", True)]

        # Removed .sudo() to allow native Record Rules to filter visibility by user persona
        articles = request.env["knowledge.article"].search(domain, limit=1000)

        # Fetch and group root articles for the sidebar navigation
        workspace_articles, shared_articles, private_articles = (
            self._get_sidebar_articles()
        )

        return request.render(
            "knowledge.search_results_template",
            {
                "articles": articles,
                "search_term": search,
                "workspace_articles": workspace_articles,
                "shared_articles": shared_articles,
                "private_articles": private_articles,
                "is_internal_user": request.env.user.has_group("base.group_user")
                or request.env.user.has_group("base.group_portal"),
            },
        )

    @http.route(["/manual/by_name/<string:name>"], type="http", auth="public", website=True)
    def manual_article_by_name(self, name, **kwargs):
        normalized_name = name.replace("+", " ")
        domain = [("name", "=ilike", normalized_name)]
        is_internal = request.env.user.has_group("base.group_user")
        if not is_internal:
            domain.append(("is_published", "=", True))
        
        article = request.env["knowledge.article"].search(domain, limit=1)
        if article:
            return request.redirect(article.website_url)
        raise werkzeug.exceptions.NotFound()

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
        # Protect against open redirects by enforcing local paths
        referer = request.httprequest.referrer or "/manual"
        parsed_referrer = url_parse(referer)
        safe_redirect = (
            parsed_referrer.path if parsed_referrer.path.startswith("/") else "/manual"
        )
        
        # --- ANTI-SPAM: Honeypot Check ---
        if website_feedback_honeypot:
            return request.redirect(safe_redirect)
            
        session_key = f"feedback_submitted_{article_id}"
        if request.session.get(session_key):
            # Block duplicate voting in same session
            return request.redirect(safe_redirect)

        try:
            # Fetch without sudo() first to ensure the user actually has Read access to the article
            article = request.env["knowledge.article"].browse(int(article_id))
            if article.exists():
                # Enforce access check to prevent voting on articles the user can't read
                article.check_access("read")

                # Escalate to Service Account for atomic increment
                svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                    "knowledge.user_knowledge_service_account"
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
                article.invalidate_recordset(["helpful_count", "unhelpful_count"])
                request.session[session_key] = True
        except (ValueError, AccessError) as e:
            # Silently fail on bad input or unauthorized access to prevent brute-force discovery
            _logger.warning("Feedback submission failed gracefully: %s", e)

        separator = "&" if "?" in safe_redirect else "?"
        return request.redirect(f"{safe_redirect}{separator}feedback_submitted=1")

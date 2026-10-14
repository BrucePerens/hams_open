# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
from odoo.exceptions import AccessError, UserError
from odoo.tools import html2plaintext, html_sanitize
import werkzeug.exceptions
import logging
import re
import markdown
import lxml.html
from werkzeug.urls import url_parse
from markupsafe import Markup
from odoo.tools import lru

_logger = logging.getLogger(__name__)

_md_cache = lru.LRU(1024)

# Headings of the Help index, in the order readers meet them. A member
# manual names its heading with "help_topic" in its knowledge_docs entry;
# unknown headings follow alphabetically, then "Other guides".
HELP_TOPIC_ORDER = (
    "Getting started and your account",
    "Operating your station",
    "Community",
    "Training and testing",
    "Your website",
)
DEFAULT_HELP_TOPIC = "Other guides"

class ManualLibraryController(http.Controller):

    def _compile_markdown(self, html_body, article_id=None, write_date=None):
        # [@ANCHOR: manual_compile_markdown]
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
        try:
            tree = lxml.html.fromstring(html_body)
            # Headings, lists, code blocks and the like mean the body is already
            # real HTML (an installed documentation file or an editor-made
            # article). Only plain paragraphs of typed or pasted text can be
            # markdown; re-parsing structured HTML through html2plaintext
            # produced garbled output.
            has_complex_html = bool(
                tree.xpath(
                    "//div | //table | //section | //article | //img"
                    " | //h1 | //h2 | //h3 | //h4 | //h5 | //h6"
                    " | //ul | //ol | //pre | //blockquote | //hr | //dl"
                    # An inline SVG diagram (kept by zero_sudo's SVG allowlist
                    # sanitizer) is real HTML. Its text labels can look like
                    # markdown ("- 10k"), and re-parsing it through
                    # html2plaintext would destroy the drawing.
                    " | //svg"
                )
            )
        except (TypeError, ValueError, lxml.etree.ParserError, lxml.etree.XMLSyntaxError): # audit-ignore-catch-all
            has_complex_html = False

        if is_md and not has_complex_html:
            try:
                md_html = markdown.markdown(
                    text_content, extensions=["fenced_code", "tables", "nl2br", "toc"]
                )
                # [!] SECURITY: python-markdown passes inline HTML in its
                # source straight through to its output unless told
                # otherwise -- text_content came from html2plaintext(),
                # which unescapes entities back to literal characters, so
                # a user typing visible text that merely *looks* like
                # "<script>alert(1)</script>" (no raw-HTML-injection
                # tooling required, just normal typing in the WYSIWYG
                # editor) would compile straight into a live <script> tag
                # once wrapped in Markup(). Sanitize the compiled output
                # the same way Odoo sanitizes every other HTML field
                # before trusting it as safe to render unescaped.
                res = Markup(html_sanitize(md_html))
                if cache_key:
                    _md_cache[cache_key] = res
                return res
            except (KeyError, ValueError, RecursionError) as e:
                # bug-hunt (2026-09-13): this claim's own residual-risk note flagged
                # that `except (KeyError, ValueError)` was never checked against every
                # exception type markdown.markdown() can actually raise for malformed
                # input. Confirmed empirically: ordinary, trivially-typeable text whose
                # plain-text content is a long run of "- " list markers (no exploit
                # tooling needed) drives python-markdown's list parser past Python's
                # recursion limit, raising RecursionError -- uncaught here, this 500'd
                # the whole article page on every single view (no surrounding guard in
                # the caller either). Regression test:
                # test_09_markdown_compiler_deeply_nested_list_falls_back_gracefully
                # in tests/test_robustness.py.
                _logger.warning("Markdown compilation failed: %s", e)

        res = Markup(html_body)
        if cache_key:
            _md_cache[cache_key] = res
        return res

    def _is_manual_manager(self):
        # [@ANCHOR: knowledge:is_manual_manager]
        return request.env.user.has_group("knowledge.group_manual_manager")

    def _help_domain(self):
        # [@ANCHOR: knowledge:help_audience_domain]
        """Domain for what the Help pages may list, search and open.

        Member user manuals that are published, plus articles shared with
        this user by name. Internal documentation never matches, whoever
        is asking: a Manual Administrator reaches it through the labelled
        administrators-only view (`_internal_domain`), not through Help.
        """
        website_id = request.website.id if request.website else False
        return [
            ("website_id", "in", (False, website_id)),
            "|",
            "&",
            ("is_published", "=", True),
            ("audience", "=", "member"),
            ("member_ids", "in", request.env.user.ids),
        ]

    def _internal_domain(self):
        # [@ANCHOR: knowledge:help_internal_domain]
        """Domain for the administrators-only view: everything Help hides."""
        website_id = request.website.id if request.website else False
        return [
            ("website_id", "in", (False, website_id)),
            "|",
            ("audience", "!=", "member"),
            ("is_published", "=", False),
        ]

    def _help_topics(self, articles):
        # [@ANCHOR: knowledge:help_topics]
        """Group member manuals by help topic, in a stable reader-friendly order."""
        by_topic = {}
        for article in articles:
            topic = article.help_topic or DEFAULT_HELP_TOPIC
            by_topic.setdefault(topic, []).append(article)

        def topic_key(topic):
            if topic in HELP_TOPIC_ORDER:
                return (0, HELP_TOPIC_ORDER.index(topic), topic)
            if topic == DEFAULT_HELP_TOPIC:
                return (2, 0, topic)
            return (1, 0, topic.lower())

        return [
            # A manual may ask to lead its heading with "sequence" in its knowledge_docs entry (lower first); the rest share the
            # default and stay alphabetical.
            (topic, sorted(by_topic[topic], key=lambda a: (a.sequence, a.name.lower())))
            for topic in sorted(by_topic, key=topic_key)
        ]

    def _can_open_article(self, article):
        # [@ANCHOR: knowledge:can_open_article]
        """True when this reader may open the article by its URL."""
        user = request.env.user
        if self._is_manual_manager():
            return True
        if user.id in article.member_ids.ids:
            return True
        if article.audience != "member":
            return False
        if user.has_group("base.group_user"):
            return True
        return bool(article.is_published)

    def _get_sidebar_articles(self):
        """Helper to fetch and group root articles for the sidebar."""
        # [@ANCHOR: manual_sidebar_search_optimization]
        # Performance: Reducing 3 RPC/DB round-trips to 1 by using a combined domain.
        user_id = request.env.user.id

        base_domain = [("parent_id", "=", False)] + self._help_domain()

        # Manual Administrators may read every article, including internal
        # documentation whose permission is "none" (see the documentation
        # installer in zero_sudo). Internal documentation is not in the
        # help domain at all, so it never reaches this sidebar; the
        # permission filter below only sorts out personal articles.
        is_manager = self._is_manual_manager()

        # Combined domain to fetch all relevant root articles in one go
        combined_domain = list(base_domain)
        if not is_manager:
            combined_domain += [
                "|",
                ("internal_permission", "in", ("read", "write")),
                "|",
                ("member_ids", "in", [user_id]),
                "&",
                ("internal_permission", "=", "none"),
                ("create_uid", "=", user_id),
            ]

        all_roots = request.env["knowledge.article"].search(combined_domain, limit=5000)

        shared_articles = all_roots.filtered(
            lambda a: a.internal_permission == "none" 
            and a.member_ids 
            and (user_id in a.member_ids.ids or a.create_uid.id == user_id)
        )
        private_articles = all_roots.filtered(
            lambda a: a.internal_permission == "none"
            and a.create_uid.id == user_id
            and not a.member_ids
        )
        if is_manager:
            # Member manuals that are neither shared nor private to this
            # administrator still belong in the main list for them.
            workspace_articles = all_roots - shared_articles - private_articles
        else:
            workspace_articles = all_roots.filtered(
                lambda a: a.internal_permission in ("read", "write")
            )

        return workspace_articles, shared_articles, private_articles

    def _sidebar_context(self):
        # [@ANCHOR: knowledge:sidebar_context]
        workspace_articles, shared_articles, private_articles = (
            self._get_sidebar_articles()
        )
        user = request.env.user
        return {
            "workspace_articles": workspace_articles,
            "shared_articles": shared_articles,
            "private_articles": private_articles,
            "is_internal_user": user.has_group("base.group_user")
            or user.has_group("base.group_portal"),
            "is_manual_manager": self._is_manual_manager(),
        }

    @http.route(
        ["/manual", "/manual/<string:article_slug>"],
        type="http",
        auth="public",
        website=True,
    )
    def manual_article_view(self, article_slug=None, **kwargs):
        # [@ANCHOR: controller_manual_article_view]

        # See [@ANCHOR: story_article_view] and [@ANCHOR: journey_user_browsing]

        # # Verified by [@ANCHOR: test_controller_manual_article_view]
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

        # 2. No article asked for: this is the Help index, a landing page
        # listing the member user manuals by topic. It used to open whichever
        # article happened to be installed first (for an administrator,
        # "External Dependencies").
        if not article:
            return self._render_help_index()

        # 3. Enforce Read Context (Public/Guest)
        if not article.exists():
            raise werkzeug.exceptions.NotFound()
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

        # 4. [@ANCHOR: knowledge:internal_article_url_404] Internal
        # documentation answers 404, not 403, to everyone but a Manual
        # Administrator, so its existence is not revealed.
        if not self._can_open_article(user_article):
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
                "search_term": "",
                "is_internal_article": article.audience != "member",
                **self._sidebar_context(),
            },
        )

    # [@ANCHOR: knowledge:help_index]
    def _render_help_index(self):
        """The Help landing page: member user manuals by topic, with search."""
        articles = request.env["knowledge.article"].search(
            [("parent_id", "=", False)] + self._help_domain(), limit=1000
        )
        return request.render(
            "knowledge.help_index_template",
            {
                "topics": self._help_topics(articles),
                "search_term": "",
                **self._sidebar_context(),
            },
        )

    # [@ANCHOR: knowledge:internal_index]
    @http.route(["/manual/internal"], type="http", auth="public", website=True)
    def manual_internal_index(self, search="", **kwargs):
        """Administrators-only list of the documentation Help hides."""
        if not self._is_manual_manager():
            raise werkzeug.exceptions.NotFound()
        domain = self._internal_domain()
        if search:
            domain += ["|", ("name", "ilike", search), ("body", "ilike", search)]
        articles = request.env["knowledge.article"].search(domain, limit=1000)
        return request.render(
            "knowledge.internal_index_template",
            {
                "articles": articles.sorted(lambda a: a.name.lower()),
                "search_term": search,
                **self._sidebar_context(),
            },
        )

    @http.route(["/manual/search"], type="http", auth="public", website=True)
    def manual_search(self, search="", **kwargs):
        # [@ANCHOR: controller_manual_search]

        # # Verified by [@ANCHOR: test_tour_manual_search]
        # See story_manual_search and journey_user_browsing
        """
        Provides full-text search across accessible articles.
        """
        # [@ANCHOR: knowledge:help_search_member_only] Only member user
        # manuals are searched, for every reader including administrators.
        domain = self._help_domain()
        if search:
            domain += ["|", ("name", "ilike", search), ("body", "ilike", search)]

        articles = request.env["knowledge.article"].search(domain, limit=1000)

        return request.render(
            "knowledge.search_results_template",
            {
                "articles": articles,
                "search_term": search,
                **self._sidebar_context(),
            },
        )

    # [@ANCHOR: knowledge:COMM_knowledge_home_alias]
    @http.route(["/knowledge/home"], type="http", auth="public", website=True)
    def knowledge_home_alias(self, search="", **kwargs):
        """
        Alias for Odoo-Knowledge-module URL familiarity (/knowledge/home?search=...),
        added 2026-08-28 at Bruce's direction: found live that "Help" links across
        roughly a dozen modules were written assuming this URL shape (presumably
        matching muscle memory from Odoo's own real, proprietary Knowledge module),
        but this module -- a deliberate from-scratch work-alike, not a wrapper around
        Odoo's real one -- only ever implemented /manual/search and
        /manual/by_name/<name>, so every one of those links 404'd. Rather than
        rewrite every call site (this module's own knowledge_docs manifest name
        doesn't always match the search string a caller happened to write -- e.g.
        "Ham Events" vs the real "Ham Events & Nets Manual" -- found while fixing
        four of these by hand before Bruce redirected to this approach), alias the
        URL Odoo users already expect onto manual_article_by_name's own exact-match
        lookup by calling it directly (not reimplementing its domain query here --
        an earlier draft of this alias duplicated that logic inline, which Bruce
        flagged as a real DRY risk: the two copies could silently drift apart the
        next time one gets edited), so a caller's #anchor fragment still lands on
        the right heading with zero extra clicks. Falls through to the broader
        ilike-substring search-results page (manual_search) only when
        manual_article_by_name itself finds no exact match -- e.g. because the
        caller's search string was imprecise (this module's own knowledge_docs
        manifest name doesn't always match the search string a caller happened to
        write -- e.g. "Ham Events" vs the real "Ham Events & Nets Manual", found
        while fixing four of these links by hand before Bruce redirected to this
        approach) -- which still finds the right article (ilike is substring, not
        exact), just one click further away instead of a 404.

        Deliberately a direct method call, not an HTTP redirect to /manual/search
        or /manual/by_name/<name>: a redirect to by_name would still 404 on every
        imprecise search string (the exact problem this alias exists to absorb),
        and a redirect straight to manual_search would always show a results page
        even for the common exact-match case, losing the zero-extra-click jump a
        caller's #anchor fragment is counting on.
        """
        if search:
            try:
                return self.manual_article_by_name(search, **kwargs)
            except werkzeug.exceptions.NotFound:
                _logger.debug(
                    "No exact article match for %r; falling through to search", search
                )
        if not search:
            return self.manual_article_view(**kwargs)
        return self.manual_search(search=search, **kwargs)

    # [@ANCHOR: knowledge:COMM_manual_article_by_name]
    @http.route(["/manual/by_name/<string:name>"], type="http", auth="public", website=True)
    def manual_article_by_name(self, name, **kwargs):
        normalized_name = name.replace("+", " ")
        # Bug-hunt fix (2026-09-09): `normalized_name` is a raw,
        # attacker/visitor-controlled URL path segment. Odoo's `=ilike`
        # operator (unlike bare `ilike`) passes its operand to SQL `ILIKE`
        # verbatim -- no wildcard-wrapping AND no escaping of `%`/`_`
        # (same root cause already found and fixed this session in
        # edge_routing/models/res_users.py and routing_mixin.py). A
        # request for `/manual/by_name/%25` (URL-decoded to a bare "%")
        # would match every article's `name` instead of failing to find
        # an exact match, defeating this endpoint's whole "exact name"
        # contract and silently redirecting a caller (including
        # knowledge_home_alias's own exact-match lookup) to an arbitrary,
        # unintended article. `knowledge.article.name` is free-form,
        # user-authored text (not charset-constrained like
        # `website_slug`), so case-insensitivity must be preserved --
        # escaping `%`/`_` (and a literal backslash, PostgreSQL's own
        # default ILIKE escape character) neutralizes the wildcard
        # injection while keeping exact, case-insensitive matching for
        # every real title.
        escaped_name = (
            normalized_name.replace("\\", "\\\\")
            .replace("%", "\\%")
            .replace("_", "\\_")
        )
        # Help links resolve to member manuals only. A Manual
        # Administrator's links may also reach internal documentation.
        if self._is_manual_manager():
            website_id = request.website.id if request.website else False
            domain = [("website_id", "in", (False, website_id))]
        else:
            domain = self._help_domain()
        domain = [("name", "=ilike", escaped_name)] + domain

        article = request.env["knowledge.article"].search(
            domain, limit=1, order="audience desc, id"
        )
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

        # # Verified by [@ANCHOR: test_tour_manual_feedback]
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

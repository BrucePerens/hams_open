# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
from markupsafe import Markup
from odoo.addons.user_websites.models.member_html import MAX_BODY_CHARS, sanitize_member_html
from odoo.addons.user_websites.models.site_slug import legacy_slug_target
import json
import logging
import os
import re
import redis
import hashlib
import hmac
from urllib.parse import quote, urlparse, urlsplit
from xml.sax.saxutils import escape as xml_escape

from werkzeug.wrappers import Response

from odoo import _
from odoo.tools import email_normalize

from odoo.addons.form_shield.models.form_shield import (
    LIMITED,
    QUESTION,
    UNAVAILABLE,
)
from odoo.addons.form_shield.refusal import refusal_response

from odoo.addons.user_websites.models.ham_gdpr_export_token import TOKEN_EXPIRY_MINUTES
from odoo.addons.distributed_redis_cache.redis_pool import REDIS_PASS_DEFAULT, REDIS_USERNAME_DEFAULT

_logger = logging.getLogger(__name__)

REDIS_HOST = os.environ.get("REDIS_HOST", "redis")
REDIS_PORT = int(os.environ.get("REDIS_PORT", "6379"))
# Same credentials as distributed_redis_cache's shared pool (REDIS_USERNAME/REDIS_PASSWORD from
# redis.env); production Redis refuses unauthenticated clients since 2026-10-03.
redis_pool = redis.ConnectionPool(
    host=REDIS_HOST,
    port=REDIS_PORT,
    username=REDIS_USERNAME_DEFAULT,
    password=REDIS_PASS_DEFAULT,
    db=0,
    decode_responses=True,
    socket_timeout=1.0,
    socket_connect_timeout=1.0,
)
redis_client = redis.Redis(connection_pool=redis_pool)


BLOG_TITLE_MAX_CHARS = 120  # the form's own maxlength, and a page's title limit

# [@ANCHOR: user_websites:report_limits]
# Verified by [@ANCHOR: test_report_violation_is_rate_limited]
# Per network: 5 reports an hour (a member 30, counted per user); per reported page: 10 a day,
# because a page that really is a problem is reported by several people, not by hundreds; for
# the whole site: 100 an hour. (The limit that stood here before counted request.remote_addr,
# which behind the Cloudflare tunnel is always the loopback address, so every visitor shared
# one counter.)
REPORT_LIMITS = (
    ("subject", 5, 3600),
    ("target", 10, 86400),
    ("global", 100, 3600),
)
REPORT_MEMBER_LIMITS = (
    ("subject", 30, 3600),
    ("target", 10, 86400),
    ("global", 100, 3600),
)
MIN_DESCRIPTION_CHARS = 10
MAX_URLS_FROM_GUEST = 2
URL_IN_TEXT = re.compile(r"(?:https?://|www\.)", re.IGNORECASE)
RANDOM_TOKEN = re.compile(r"[A-Za-z0-9_+=/.\-]{16,}")
LOWER_UPPER_DIGIT = (re.compile("[a-z]"), re.compile("[A-Z]"), re.compile("[0-9]"))


# [@ANCHOR: user_websites:bare_host]
# Verified by [@ANCHOR: user_websites:test_normalize_report_target]
def _bare_host(host):
    host = (host or "").lower().rstrip(".")
    return host[4:] if host.startswith("www.") else host


# [@ANCHOR: user_websites:report_own_site_hosts]
# Verified by [@ANCHOR: user_websites:test_normalize_report_target]
def _own_site_hosts():
    """The hostnames a report may be about: this request's own host, the base URL and the
    domain of every website of the database, www. dropped, port dropped."""
    hosts = {_bare_host(request.httprequest.host.split(":")[0])}
    base_url = request.env["ir.config_parameter"]._get_param("web.base.url")
    hosts.add(_bare_host(urlsplit(base_url or "").hostname))
    domains = request.env["website"].search([], limit=500).mapped("domain")
    for domain in domains:
        text = (domain or "").strip()
        if text and "//" not in text:
            text = "//" + text
        hosts.add(_bare_host(urlsplit(text).hostname))
    hosts.discard("")
    return hosts


# [@ANCHOR: user_websites:normalize_report_target]
# Verified by [@ANCHOR: user_websites:test_normalize_report_target]
# Verified by [@ANCHOR: test_report_violation_refuses_another_sites_url]
def normalize_report_target(raw, hosts):
    """The reported URL in one canonical form, or None when it is not a page of this site.

    http and https are the same page (the tunnel in front of production makes the request see
    http), a leading "www." does not matter, a bare path means this host, and the fragment,
    credentials and control characters are dropped or refused. `hosts` is _own_site_hosts().
    """
    text = (raw or "").strip()
    if not text or len(text) > 2048 or any(ord(char) < 32 for char in text):
        return None
    try:
        parts = urlsplit(text)
        host = parts.hostname
    except ValueError:
        return None
    if parts.username or parts.password:
        return None
    if parts.scheme or parts.netloc:
        if parts.scheme not in ("http", "https") or not host:
            return None
        if _bare_host(host) not in hosts:
            return None
        origin = "%s://%s" % (parts.scheme, parts.netloc.lower())
    else:
        if not text.startswith("/") or text.startswith("//"):
            return None
        origin = request.httprequest.host_url.rstrip("/")
    path = parts.path or "/"
    query = "?" + parts.query if parts.query else ""
    return origin + path + query


# [@ANCHOR: user_websites:local_part_ok]
# Verified by [@ANCHOR: user_websites:test_report_problem_reasons]
def _local_part_ok(address):
    local = address.partition("@")[0]
    return not (local.startswith(".") or local.endswith(".") or ".." in local)


# [@ANCHOR: user_websites:report_problem]
# Verified by [@ANCHOR: user_websites:test_report_problem_reasons]
def report_problem(target, description, email, member):
    """The reason a report is refused on its content alone, else None. Deliberately only
    structural: a report that is short, unreadable or all links is refused; a rude, wrong or
    odd one is the moderators' to read."""
    if not target:
        return "foreign_target"
    text = (description or "").strip()
    if len(text) < MIN_DESCRIPTION_CHARS:
        return "short_description"
    if RANDOM_TOKEN.fullmatch(text) and all(
        pattern.search(text) for pattern in LOWER_UPPER_DIGIT
    ):
        return "gibberish"
    if member:
        return None
    if len(URL_IN_TEXT.findall(text)) > MAX_URLS_FROM_GUEST:
        return "too_many_urls"
    address = email_normalize((email or "").strip(), strict=True)
    if not address or not _local_part_ok(address):
        return "bad_email"
    return None


# [@ANCHOR: user_websites:reported_path]
# Verified by [@ANCHOR: user_websites:test_normalize_report_target]
def _reported_path(reported_url):
    """The PATH (and existing query string) of `reported_url`, or "/" -- never its scheme/host.
    See _report_violation_redirect_target() for why."""
    path = "/"
    if reported_url:
        try:
            parsed_path = urlparse(reported_url).path
        except ValueError:
            parsed_path = ""
        if parsed_path:
            path = parsed_path
    # "//host" and "/\\host" are protocol-relative to a browser: a path that arrives that way
    # (http://this-site//evil.example) must not leave the site, so exactly one leading slash.
    return "/" + path.lstrip("/\\")


# [@ANCHOR: user_websites:report_violation_redirect_target]
# Verified by [@ANCHOR: test_report_violation_redirects_back_to_reported_page]
def _report_violation_redirect_target(reported_url, query_param):
    """Where to send the visitor back after a report_violation() submission.

    Deliberately uses only the PATH (and existing query string) of `reported_url`, never its
    scheme/host -- `reported_url` comes from an untrusted POST field (or the Referer header),
    so echoing it back verbatim as a redirect target would be an open redirect. A bare path is
    always same-origin by construction, and Odoo's own request.redirect() resolves it against
    the current site regardless of what host `reported_url` claimed.
    """
    path = _reported_path(reported_url)
    separator = "&" if "?" in path else "?"
    return f"{path}{separator}{query_param}"


def _legacy_slug_redirect(website_slug, rest=""):
    """A 301 from a retired site slug to the same page of the site that holds it now, or None.

    [@ANCHOR: user_websites:fn_main_legacy_slug_redirect]
    Verified by [@ANCHOR: test_site_slug_one_site]
    `rest` is the part after the slug ("", "/blog"). The target is built from the slug as stored
    (limited to [a-z0-9-]) and quoted, so it is always a same-origin path.
    """
    target = legacy_slug_target(request.env, website_slug)
    if not target:
        return None
    return request.redirect("/" + quote(target, safe="") + rest, code=301, local=True)


HOME_NONE = "none"
HOME_PUBLISHED = "published"
HOME_UNPUBLISHED = "unpublished"
HOME_DELETED = "deleted"
HOME_BLOCKED = "blocked"


def _owner_home(env_svc, profile_user, profile_group, slug, website_id):
    """(page, state) for the owner's home page at /<slug>, whatever its state.

    [@ANCHOR: user_websites:fn_main_owner_home]
    Verified by [@ANCHOR: test_create_site_reuses_existing_home]
    One question, asked by the page that shows the "Create" button and by create_site() that
    acts on it, so they cannot disagree (found 2026-10-29: an unpublished home at /k6bp made the
    public page offer "Create your site", and the button then failed on the unique address).
    States: HOME_NONE (nothing: create one), HOME_UNPUBLISHED (the page exists, hidden: publish
    it), HOME_DELETED (the owner deleted it and its address is parked at /<slug>/deleted-<id>:
    restore it), HOME_BLOCKED (archived by moderation, not by the owner, or the address is held
    by a page of another owner: neither reused nor overwritten). A published home is served by
    user_home_fallback() before this is asked (HOME_PUBLISHED is for create_site()).
    """
    owner_domain = (
        [("owner_user_id", "=", profile_user.id)]
        if profile_user
        else [("user_websites_group_id", "=", profile_group.id)]
    )
    pages = env_svc["website.page"].with_context(active_test=False)
    at_address = pages.search(
        [("url", "=", f"/{slug}"), ("website_id", "=", website_id)], limit=1
    )
    if at_address:
        owner = (
            at_address.owner_user_id == profile_user
            if profile_user
            else at_address.user_websites_group_id == profile_group
        )
        if not owner or not at_address.active:
            return at_address, HOME_BLOCKED
        if at_address.website_published:
            return at_address, HOME_PUBLISHED
        return at_address, HOME_UNPUBLISHED
    parked = pages.search(
        owner_domain
        + [
            ("deleted_original_url", "=", f"/{slug}"),
            ("deleted_by_owner", "!=", False),
            ("website_id", "=", website_id),
        ],
        order="deleted_by_owner desc, id desc",
        limit=1,
    )
    if parked:
        return parked, HOME_DELETED
    return pages.browse(), HOME_NONE


def _serve_core_fallback():
    # [@ANCHOR: user_websites:fn_main_serve_core_fallback]
    """Odoo's own page / redirect / attachment lookup for the request's path.

    The /<website_slug> route puts website_slug into request.params, and
    website.page._allow_to_use_cache() refuses to cache any response whose request carries
    parameters ("the parameters are not part of the cache key"), so every stock page reached
    through this route would lose its edge caching. The path argument is not a query parameter:
    drop it before handing the request to the page lookup.
    """
    request.params.pop("website_slug", None)
    return request.env["ir.http"]._serve_fallback()


class UserWebsitesController(http.Controller):

    # [@ANCHOR: user_websites:report_violation_page]
    # Verified by [@ANCHOR: form_shield:test_question_form_page_renders_accessible_question]
    # The report form as a page of its own, with the plain-text question in place of the
    # proof of work. It serves a visitor whose browser runs no scripts (the Bootstrap modal
    # cannot even open without them) or cannot run the proof of work (the modal links here),
    # and it is what a post that carried no answer at all is shown. A question is issued per
    # request, so this page must never be cached: it opts into no edge caching.
    @http.route(
        "/website/report_violation",
        type="http",
        auth="public",
        methods=["GET"],
        website=True,
        sitemap=False,
    )
    def report_violation_form(self, url="", **_ignored):
        reported = url or request.httprequest.headers.get("Referer", "")
        return self._render_report_page(reported)

    # [@ANCHOR: user_websites:render_report_page]
    # Verified by [@ANCHOR: user_websites:test_report_violation_shield_outcomes]
    def _render_report_page(self, url, description="", email="", notice="", status=200):
        issued = request.env["form.shield"].issue("q")
        response = request.render(
            "user_websites.report_violation_page",
            {
                "url": url,
                "description": description,
                "email": email,
                "notice": notice,
                "issued": issued,
            },
        )
        response.status_code = status
        response.headers["Cache-Control"] = "no-store"
        return response

    @http.route(
        "/website/report_violation",
        type="http",
        auth="public",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    def report_violation(self, url="", reason="", description="", email="", website_honeypot="", **post):
        # [@ANCHOR: user_websites:UX_REPORT_VIOLATION]

        # Triggered by [@ANCHOR: violation_report_logic]

        # Tests [@ANCHOR: user_websites:UX_REPORT_VIOLATION]

        # Verified by [@ANCHOR: user_websites:test_report_violation_honeypot_rejected]
        # report_violation_modal (user_websites_templates.xml) already ships a hidden
        # "website_honeypot" field (d-none, aria-hidden, tabindex="-1" -- invisible and
        # unreachable for a real visitor, but many form-filling bots populate every input
        # regardless of CSS). Found live 2026-09-28/29: the field existed but this controller
        # never actually read it, so it caught nothing -- five near-identical spam submissions
        # ("Hi, I wanted to know your price," the same template in five different languages)
        # landed as real content.violation.report rows and emailed Bruce, over five days, while
        # the honeypot sat there doing nothing. Pretend success rather than surfacing an error:
        # a bot that gets useful rejection feedback learns to stop filling the field, which
        # defeats the whole mechanism; a real visitor never sees or fills this field at all, so
        # this branch never fires for one.
        #
        # The honeypot alone was not enough: reports 7 and 8 (2026-10-12 and 10-14) arrived
        # after it was fixed, from programs that left it empty. The form_shield module's
        # checks now run first (proof of work or question, a second honeypot, time-to-submit,
        # rate limits); see [@ANCHOR: user_websites:report_violation_shield].
        if website_honeypot:
            _logger.info("report_violation: honeypot triggered, discarding silently")
            request.env["form.shield"].refuse("report_violation", "honeypot")
            return request.redirect(
                _report_violation_redirect_target(url, "report_submitted=1")
            )

        # Extract referrer if it's missing in POST
        if not url and "Referer" in request.httprequest.headers:
            url = request.httprequest.headers.get("Referer")

        if not url or not description:
            return request.redirect(
                _report_violation_redirect_target(url, "report_error=missing_fields")
            )

        # [@ANCHOR: user_websites:report_violation_shield]
        # Verified by [@ANCHOR: user_websites:test_report_violation_shield_outcomes]
        target = normalize_report_target(url, _own_site_hosts())
        shield = request.env["form.shield"]
        member = not request.env.user._is_public()
        verdict = shield.check_form(
            "report_violation",
            post,
            target=target,
            limits=REPORT_MEMBER_LIMITS if member else REPORT_LIMITS,
        )
        if not verdict.ok:
            return self._report_refusal(verdict, url, description, email)

        # Structural checks, after the shield so that a program that cannot solve the proof of
        # work never reaches them. Each refusal is counted by reason (no text is kept).
        problem = report_problem(target, description, email, member)
        if problem:
            shield.refuse("report_violation", problem)
            return request.redirect(
                _report_violation_redirect_target(url, "report_error=" + problem)
            )
        url = target
        # Enforce max length on description
        if len(description) > 5000:
            description = description[:5000]

        # micro-privilege: Use service env wrapper to securely process the public submission
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        try:
            parsed = urlparse(url)
            path = parsed.path
            parts = [p for p in path.split("/") if p]
            slug = parts[0] if parts else None

            create_vals = {
                "target_url": url,
                "description": description,
                "state": "new",
            }
            if email:
                create_vals["reported_by_email"] = email
            # [@ANCHOR: report_violation_records_logged_in_reporter]
            # Verified by [@ANCHOR: test_report_violation_records_logged_in_reporter]
            # Record who filed it when a signed-in member did. The service env creates the row,
            # so nothing records the reporter unless it is set here; the id comes from the
            # session's own user, never from the posted form, so a reporter cannot name someone
            # else. The public user is not a reporter: a guest is identified only by the email
            # field the modal asks them for.
            if not request.env.user._is_public():
                create_vals["reported_by_user_id"] = request.env.user.id

            if slug:
                # Resolve the owner or group
                user_id = env_svc["res.users"].get_record_by_slug(slug)
                group_id = env_svc["user.websites.group"].get_record_by_slug(slug)

                if user_id:
                    create_vals["content_owner_id"] = user_id
                elif group_id:
                    create_vals["content_group_id"] = group_id

            env_svc["content.violation.report"].create(create_vals)
        except (KeyError, ValueError) as e:   # Tested by [@ANCHOR: test_tour_violation_report]
            _logger.warning("Report creation failed: %s", e)
            return request.redirect(
                _report_violation_redirect_target(url, "report_error=creation_failed")
            )

        return request.redirect(
            _report_violation_redirect_target(url, "report_submitted=1")
        )

    # [@ANCHOR: user_websites:report_refusal]
    # Verified by [@ANCHOR: user_websites:test_report_violation_shield_outcomes]
    def _report_refusal(self, verdict, url, description, email):
        """The response for a submission the shield refused. A program is told it succeeded; a
        person is told what to do next, and keeps what they typed where the page can hold it."""
        if verdict.outcome == QUESTION:
            notice = _("Please answer this short question to send your report.")
            return self._render_report_page(url, description, email, notice)
        if verdict.outcome == LIMITED:
            notice = _("Too many reports from your network. Please wait a while and try again.")
            return self._render_report_page(url, description, email, notice, status=429)
        if verdict.outcome == UNAVAILABLE:
            notice = _("Reporting is temporarily unavailable. Please try again later.")
            return self._render_report_page(url, description, email, notice, status=503)
        return refusal_response(
            verdict,
            _reported_path(url),
            _report_violation_redirect_target(url, "report_submitted=1"),
        )

    @http.route(
        ["/<uwslug:website_slug>/blog", "/<uwslug:website_slug>/blog/page/<int:page>"],
        type="http",
        auth="public",
        website=True,
    )
    # [@ANCHOR: user_websites:COMM_user_blog_index]
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
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        profile_user = (
            env_svc["res.users"].browse(user_id) if user_id else env_svc["res.users"]
        )
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        profile_group = (
            env_svc["user.websites.group"].browse(group_id)
            if group_id
            else env_svc["user.websites.group"]
        )

        if not profile_user and not profile_group:
            legacy = _legacy_slug_redirect(website_slug, "/blog")
            if legacy:
                return legacy
            raise request.not_found()

        if profile_user and not profile_user._website_publicly_visible():
            raise request.not_found()

        if profile_group and profile_group.is_suspended_from_websites:
            raise request.not_found()

        main_object = profile_user or profile_group

        is_owner = False
        if profile_user and profile_user.id == request.env.user.id:
            is_owner = True
        elif profile_group and request.env.user.id in profile_group.member_ids.ids:
            is_owner = True

        domain = (
            [("owner_user_id", "=", profile_user.id)]
            if profile_user
            else [("user_websites_group_id", "=", profile_group.id)]
        )
        blogs = env_svc["blog.blog"].search(domain, limit=100)
        # [!] SECURITY FIX (bug-hunt, 2026-09-09): env_svc runs as the
        # user_websites service account, whose own blog_post_svc_rule
        # ir.rule grants it an unconditional [(1,'=',1)] read domain (see
        # security/user_websites_security.xml) -- it does NOT get the
        # is_published-scoped rule that base.group_public/group_portal get.
        # A bare `env_svc["blog.post"].search(domain, ...)` therefore
        # returned every post (including unpublished drafts) to ANY visitor,
        # not just the profile's own owner -- the exact class of bug this
        # file's own user_home_fallback() already defends against for
        # website.page (see its explicit `("website_published", "=", True)`
        # filter a few methods below). Restore that same defense here: only
        # the profile's own owner gets to see unpublished drafts in this
        # listing; everyone else is scoped to published posts only, decided
        # in the controller rather than relying on the service account's
        # own (intentionally permissive, backend-CRUD-oriented) ir.rule.
        post_domain = domain if is_owner else domain + [("is_published", "=", True)]
        posts = env_svc["blog.post"].search(post_domain, limit=100)

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

        pager = {
            "page_count": 0,
            "page": dict(),
            "page_previous": dict(),
            "page_next": dict(),
        }
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

    @http.route(
        "/<uwslug:website_slug>/home",
        type="http",
        auth="public",
        website=True,
        sitemap=False,
    )
    # [@ANCHOR: user_websites:COMM_user_home_legacy_redirect]
    def user_home_legacy_redirect(self, website_slug, **kwargs):
        """Permanent redirect from the old /<slug>/home address to /<slug>.

        Every member and group site's home is /<slug> (Bruce, 2026-10-12). Existing links,
        bookmarks and search-engine entries for /<slug>/home keep working through this 301.
        The target is built from the slug as stored in the database (lower-case, limited to
        [a-z0-9-] by a database constraint) and quoted, never from the raw request path, so it is
        always a same-origin path and cannot be steered to another host. A single path segment
        can never redirect to itself, so there is no loop.
        """
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        record = (
            env_svc["res.users"].browse(user_id)
            if user_id
            else env_svc["user.websites.group"].browse(group_id)
            if group_id
            else None
        )
        if not record:
            legacy = _legacy_slug_redirect(website_slug)
            if legacy:
                return legacy
            # Not a member site: some other page may own this path (e.g. a stock CMS page).
            response = _serve_core_fallback()
            if response:
                return response
            raise request.not_found()
        return request.redirect(
            "/" + quote(record.website_slug, safe=""), code=301, local=True
        )

    @http.route(
        "/<uwslug:website_slug>",
        type="http",
        auth="public",
        website=True,
        sitemap=False,
    )
    # [@ANCHOR: user_websites:COMM_user_home_fallback]
    # Verified by [@ANCHOR: test_member_site_home_url]
    def user_home_fallback(self, website_slug, **kwargs):
        """A member or group site's home, at /<slug>.

        Serves the site's home page, or the placeholder that lets the owner create it. A single
        path segment that is not a member or group slug is not ours: it is handed to Odoo's own
        page / redirect / attachment lookup (the same fallback a request reaches when no route
        matches), so stock pages such as /aboutus and website.rewrite redirects keep working.
        """
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        profile_user = (
            env_svc["res.users"].browse(user_id) if user_id else env_svc["res.users"]
        )
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        profile_group = (
            env_svc["user.websites.group"].browse(group_id)
            if group_id
            else env_svc["user.websites.group"]
        )

        if not profile_user and not profile_group:
            # A retired slug leads to the live site before any stock page is looked for: a page
            # left at the old address would otherwise shadow the redirect and be a second site.
            legacy = _legacy_slug_redirect(website_slug)
            if legacy:
                return legacy
            response = _serve_core_fallback()
            if response:
                return response
            raise request.not_found()

        if profile_user and not profile_user._website_publicly_visible():
            raise request.not_found()

        if profile_group and profile_group.is_suspended_from_websites:
            raise request.not_found()

        canonical = (profile_user or profile_group).website_slug
        if website_slug != canonical:
            # /K6BP -> /k6bp: one address per site for caches, the sitemap and canonical URLs.
            return request.redirect(
                "/" + quote(canonical, safe=""), code=301, local=True
            )

        # Check if the page actually exists; if it does, let core ir.http route handle it
        domain = [
            ("url", "=", f"/{website_slug}"),
            ("website_published", "=", True),
        ]
        page = env_svc["website.page"].search(domain, limit=1)
        if page:
            # Manually increment the view counter if bypassed by core routing
            try:
                if not request.env.user._is_admin():
                    db_name = request.env.cr.dbname
                    redis_client.incr(f"views:{db_name}:page:{page.id}")
            except (KeyError, ValueError, redis.exceptions.RedisError) as e:   # Tested by [@ANCHOR: test_cron_redis_flush]
                _logger.warning(
                    "Redis view counter increment failed in controller: %s", e
                )
            # Deliberately NOT _serve_core_fallback(): with website_slug left in request.params
            # Odoo's page cache never serves this response, so a page unpublished by a
            # suspension is gone at once (test_03_suspension_public_access_leak).
            return request.env["ir.http"]._serve_fallback()

        is_owner = False
        if profile_user and profile_user.id == request.env.user.id:
            is_owner = True
        elif profile_group and request.env.user.id in profile_group.member_ids.ids:
            is_owner = True

        main_object = profile_user or profile_group
        home_state = HOME_NONE
        if is_owner:
            _home, home_state = _owner_home(
                env_svc, profile_user, profile_group, canonical, request.website.id
            )
        values = {
            "profile_user": profile_user,
            "profile_group": profile_group,
            "main_object": main_object,
            "resolved_slug": website_slug,
            "page_type": "home",
            "is_owner": is_owner,
            "home_state": home_state,
        }
        return request.render("user_websites.placeholder_page", values)

    @http.route(
        "/<uwslug:website_slug>/create_site",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_create_site]
    def create_site(self, website_slug, **kwargs):
        user = request.env.user

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        profile_user = (
            env_svc["res.users"].browse(user_id) if user_id else env_svc["res.users"]
        )
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        profile_group = (
            env_svc["user.websites.group"].browse(group_id)
            if group_id
            else env_svc["user.websites.group"]
        )

        if not profile_user and not profile_group:
            raise request.not_found()

        if profile_user and profile_user.id != user.id:
            raise request.not_found()
        if profile_user and profile_user.is_suspended_from_websites:
            raise request.not_found()
        if profile_user and not profile_user._hams_website_eligible():
            # Not a 404: the owner's tools say what is missing (owner_site.py).
            return request.redirect(f"/my/site/{quote(website_slug, safe='')}", code=303)
        if profile_group:
            if profile_group.is_suspended_from_websites:
                raise request.not_found()
            is_member = env_svc["user.websites.group"].search_count(
                [("id", "=", profile_group.id), ("member_ids", "=", user.id)]
            )
            if not is_member:
                raise request.not_found()

        existing, home_state = _owner_home(
            env_svc, profile_user, profile_group, website_slug, request.website.id
        )
        if home_state == HOME_BLOCKED:
            return request.redirect(
                f"/my/site/{quote(website_slug, safe='')}?error=home_blocked", code=303
            )
        if home_state == HOME_DELETED:
            # Back at its own address, published: the owner just asked for the home to exist.
            existing.with_context(no_cow=True).write(
                {
                    "url": f"/{website_slug}",
                    "active": True,
                    "deleted_by_owner": False,
                    "deleted_original_url": False,
                    "website_published": True,
                }
            )
        if home_state == HOME_UNPUBLISHED:
            existing.with_context(no_cow=True).write({"website_published": True})
        if home_state != HOME_NONE:
            return request.redirect(f"/{website_slug}")

        entity_name = profile_user.name if profile_user else profile_group.name
        # [!] SECURITY: entity_name is a user-controlled res.users/group name
        # (arbitrary text, not restricted to XML-safe characters) that gets
        # interpolated directly into a QWeb template's arch_base. Escape it
        # before it enters the XML/attribute context here at the source,
        # rather than relying solely on website_page.py's _sanitize_user_arch
        # blocklist as the only defense -- that sanitizer never runs for
        # this call path's service account and is a blocklist with known
        # gaps in any case.
        entity_name_safe = xml_escape(entity_name, {'"': "&quot;"})

        arch_base = f"""<t name="{entity_name_safe} Home" t-name="user_websites.home_{website_slug}">
            <t t-call="user_websites.template_default_home">
                <div id="wrap" class="oe_structure oe_empty"/>
            </t>
        </t>"""

        req_website_id = request.website.id

        create_vals = {
            "url": f"/{website_slug}",
            "name": f"{entity_name} Home",
            "type": "qweb",
            "arch_base": arch_base,
            "website_id": req_website_id,
            "website_published": True,
        }
        if profile_user:
            create_vals["owner_user_id"] = profile_user.id
        elif profile_group:
            create_vals["user_websites_group_id"] = profile_group.id

        page = env_svc["website.page"].create(create_vals)
        return request.redirect(page.url)

    @http.route(
        "/<uwslug:website_slug>/create_blog",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_create_blog]
    def create_blog(self, website_slug, **kwargs):
        user = request.env.user
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        profile_user = (
            env_svc["res.users"].browse(user_id) if user_id else env_svc["res.users"]
        )
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        profile_group = (
            env_svc["user.websites.group"].browse(group_id)
            if group_id
            else env_svc["user.websites.group"]
        )

        if not profile_user and not profile_group:
            raise request.not_found()

        if profile_user and profile_user.id != user.id:
            raise request.not_found()
        if profile_user and profile_user.is_suspended_from_websites:
            raise request.not_found()
        if profile_user and not profile_user._hams_website_eligible():
            # Not a 404: the owner's tools say what is missing (owner_site.py).
            return request.redirect(f"/my/site/{quote(website_slug, safe='')}", code=303)
        if profile_group:
            if profile_group.is_suspended_from_websites:
                raise request.not_found()
            if user.id not in profile_group.member_ids.ids:
                raise request.not_found()

        entity_name = profile_user.name if profile_user else profile_group.name

        req_website_id = request.website.id

        create_vals = {
            "name": f"{entity_name}'s Blog",
            "website_id": req_website_id,
        }
        if profile_user:
            create_vals["owner_user_id"] = profile_user.id
        elif profile_group:
            create_vals["user_websites_group_id"] = profile_group.id

        env_svc["blog.blog"].create(create_vals)
        return request.redirect(f"/{website_slug}/blog")

    @http.route(
        "/<uwslug:website_slug>/create_blog_post",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_create_blog_post]
    def create_blog_post(self, website_slug, **kwargs):
        # Real gap found live 2026-09-29: create_blog() (immediately above) lets an owner create
        # their own blog CONTAINER, but nothing ever let them add a POST to it -- blog_index's own
        # template (user_websites_templates.xml) renders "No posts available yet." with no button
        # at all, even for the owner, once a blog exists. A user who had just created their first
        # blog had no way to add its first post. This mirrors create_blog()'s own ownership checks
        # exactly, then creates a blank blog.post in the caller's own blog and redirects straight
        # into it via website_blog's own standard post-edit route -- the same route blog_index's
        # own "Read Post" links already use (/blog/<blog_id>/post/<post_id>), so the normal website
        # builder's editor toolbar (Edit/New) works on it exactly as it does on any other blog post,
        # with no new rendering path of this module's own to build or maintain.
        user = request.env.user
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        profile_user = (
            env_svc["res.users"].browse(user_id) if user_id else env_svc["res.users"]
        )
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        profile_group = (
            env_svc["user.websites.group"].browse(group_id)
            if group_id
            else env_svc["user.websites.group"]
        )

        if not profile_user and not profile_group:
            raise request.not_found()

        if profile_user and profile_user.id != user.id:
            raise request.not_found()
        if profile_user and profile_user.is_suspended_from_websites:
            raise request.not_found()
        if profile_user and not profile_user._hams_website_eligible():
            # Not a 404: the owner's tools say what is missing (owner_site.py).
            return request.redirect(f"/my/site/{quote(website_slug, safe='')}", code=303)
        if profile_group:
            if profile_group.is_suspended_from_websites:
                raise request.not_found()
            if user.id not in profile_group.member_ids.ids:
                raise request.not_found()

        blog_domain = (
            [("owner_user_id", "=", profile_user.id)]
            if profile_user
            else [("user_websites_group_id", "=", profile_group.id)]
        )
        blog = env_svc["blog.blog"].search(blog_domain, limit=1)
        if not blog:
            # No blog container yet. The owner toolbar's "New blog post" must work from any page
            # of the site, so make the container here with create_blog()'s own values.
            entity_name = profile_user.name if profile_user else profile_group.name
            blog_vals = {
                "name": f"{entity_name}'s Blog",
                "website_id": request.website.id,
            }
            if profile_user:
                blog_vals["owner_user_id"] = profile_user.id
            elif profile_group:
                blog_vals["user_websites_group_id"] = profile_group.id
            blog = env_svc["blog.blog"].create(blog_vals)

        create_vals = {
            "name": "New Post",
            "blog_id": blog.id,
            "website_id": blog.website_id.id,
        }
        # website_blog's own author_id defaults to self.env.user.partner_id at create time
        # (website_blog.py) -- since this create() runs elevated as the service account (see
        # blog_post.py's own create() override), that default silently attributed every post
        # created this way to "System Provisioner" instead of its real author. Set it explicitly
        # in both branches so the published byline always matches who the post actually belongs
        # to -- the group branch used the real requester's own partner (`user`, already captured
        # above), matching a group blog's own real authorship model: any member can post, and the
        # byline should credit whichever member actually wrote it, not the group as a whole.
        if profile_user:
            create_vals["owner_user_id"] = profile_user.id
            create_vals["author_id"] = profile_user.partner_id.id
        elif profile_group:
            create_vals["user_websites_group_id"] = profile_group.id
            create_vals["author_id"] = user.partner_id.id

        post = env_svc["blog.post"].create(create_vals)
        # Real gap found live 2026-10-01 (night_shift_todo/high/
        # personal-blog-owner-cannot-actually-edit-or-publish-their-own-new-post-0c44e7eb.md):
        # this used to redirect straight into website_blog's own generic post-view route
        # (/blog/<blog_id>/post/<post_id>), on the comment-stated assumption that "the normal
        # website builder's editor toolbar (Edit/New) works on it exactly as it does on any
        # other blog post." That assumption is false for this module's real target user --
        # Odoo core's own website/models/ir_http.py gates that whole toolbar on
        # website.group_website_designer, a GLOBAL, sitewide website-editing permission a
        # personal-site owner (base.group_portal + group_user_websites_user) does not and
        # structurally cannot hold without being hand-granted it through the backend Users UI.
        # Granting that group was rejected (see the to-do's own option (a) writeup) because it
        # is not scoped to "only this user's own post" -- it would let any owner edit ANY
        # website content sitewide. Redirecting here into this module's own dedicated,
        # ownership-scoped edit form (blog_post_edit/blog_post_edit_submit below) instead
        # gives the owner a real, working way to title, write, and publish the post they just
        # created, with no sitewide permission change at all.
        return request.redirect(f"/blog_post/edit/{post.id}")

    def _get_own_blog_post_for_edit(self, post_id):
        # [@ANCHOR: user_websites:COMM_get_own_blog_post_for_edit]

        # Verified by [@ANCHOR: test_blog_post_edit_denied_for_non_owner]

        # Verified by [@ANCHOR: test_blog_post_edit_denied_for_non_member]
        """Resolves `post_id` to a blog.post the CURRENT caller may edit, or False.

        Mirrors create_blog_post()'s own ownership resolution: reads through the
        user_websites_service_account's own elevated env (so a brand-new, still-unpublished
        post -- exactly what a fresh "New Post" is -- can be found at all, since an ordinary
        caller's own env is scoped by blog_post_private_rule/blog_post_group_rule to posts
        they already own), then checks the SAME ownership facts (owner_user_id, or membership
        in the owning group) those two ir.rules independently enforce, before ever handing the
        caller a live record. The caller's own env.user still does every subsequent read/write
        through the record returned here, so blog_post.py's own check_access()/
        _check_proxy_ownership_write() enforcement (and its allowed-fields whitelist) applies
        in full -- this helper only decides whether to let the request proceed at all.
        """
        user = request.env.user
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        post_svc = env_svc["blog.post"].browse(post_id)
        if not post_svc.exists():
            return False

        is_owner = post_svc.owner_user_id.id == user.id
        is_group_member = bool(
            post_svc.user_websites_group_id
            and user.id in post_svc.user_websites_group_id.member_ids.ids
        )
        if not is_owner and not is_group_member and not request.env.is_admin():
            return False

        # Browsed again through the caller's OWN env (not the elevated service account) --
        # every write below goes through this record, so blog_post.py's own write()
        # override (check_access, the proxy-ownership guard, and the non-admin allowed-
        # fields whitelist) is the real, enforced boundary, not just this helper's own
        # pre-check.
        return request.env["blog.post"].browse(post_id)

    @http.route("/blog_post/edit/<int:post_id>", type="http", auth="user", website=True)
    # [@ANCHOR: user_websites:COMM_blog_post_edit]
    def blog_post_edit(self, post_id, **kwargs):
        # Verified by [@ANCHOR: test_owner_can_edit_own_blog_post]

        # Verified by [@ANCHOR: test_blog_post_edit_denied_for_non_owner]
        post = self._get_own_blog_post_for_edit(post_id)
        if not post:
            return request.redirect("/my/home")
        return self._render_blog_post_editor(post)

    def _render_blog_post_editor(self, post, error=None, name=None, content=None, is_published=None):
        # [@ANCHOR: user_websites:fn_main_render_blog_post_editor]
        # Verified by [@ANCHOR: test_blog_post_save_error_keeps_what_was_typed]
        """The blog post form. After a refused save, `name`, `content` and `is_published` are what the member typed, so
        the form comes back with their words in it (and the reason), not with the stored post."""
        owner = post.owner_user_id or post.user_websites_group_id
        if not owner:
            return request.redirect("/my/home")
        env_svc = request.env["zero_sudo.security.utils"]._get_service_env(
            "user_websites.user_websites_service_account"
        )
        owner_svc = env_svc[owner._name].browse(owner.id)
        source = post.content if content is None else (content or "")[:MAX_BODY_CHARS]
        clean_html, _hostile = sanitize_member_html(source)
        return request.render(
            "user_websites.blog_post_edit_form",
            {
                "post": post,
                "default_title": "Edit Blog Post",
                "editor_initial": Markup(clean_html),
                "form_name": name,
                "form_published": is_published,
                "error": error,
                "site_slug": owner_svc.website_slug,
                "profile_user": owner_svc if owner._name == "res.users" else env_svc["res.users"],
                "profile_group": owner_svc if owner._name != "res.users" else env_svc["user.websites.group"],
                "main_object": owner_svc,
                "resolved_slug": owner_svc.website_slug,
                "member_site": True,
                "is_owner": True,
            },
            status=400 if error else 200,
        )

    @http.route(
        "/blog_post/edit/submit/<int:post_id>",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_blog_post_edit_submit]
    def blog_post_edit_submit(
        self, post_id, name="", content="", is_published="", **kwargs
    ):
        # Verified by [@ANCHOR: test_owner_can_edit_own_blog_post]

        # Verified by [@ANCHOR: test_blog_post_edit_denied_for_non_owner]

        # Verified by [@ANCHOR: test_blog_post_edit_denied_for_non_member]

        # Verified by [@ANCHOR: test_blog_post_edit_submit_sanitizes_content]
        post = self._get_own_blog_post_for_edit(post_id)
        if not post:
            return request.redirect("/my/home")

        name = (name or "").strip()
        if not name or len(name) > BLOG_TITLE_MAX_CHARS:
            return self._render_blog_post_editor(post, "missing_fields", name, content, is_published)

        # [!] SECURITY: blog.post's own "content" field is declared sanitize=False (stock
        # website_blog), so the ORM does not sanitize a write to it. The post body is written
        # through the strict allow-list sanitizer (models/member_html.py) in this request path
        # whoever the caller is: the model-level sanitizer skips administrators and the service
        # account, and an internal administrator who owns a site is exactly such a caller.
        # Hostile markup (script, frames, event handlers, QWeb directives) is stripped and also
        # filed as an automated content violation, as a hostile page body is.
        if len(content or "") > MAX_BODY_CHARS:
            return self._render_blog_post_editor(post, "too_long", name, content, is_published)
        sanitized_content, hostile = sanitize_member_html(content)
        if hostile:
            hostile_vals = {"url": f"/blog_post/edit/{post_id}"}
            if post.owner_user_id:
                hostile_vals["owner_user_id"] = post.owner_user_id.id
            elif post.user_websites_group_id:
                hostile_vals["user_websites_group_id"] = post.user_websites_group_id.id
            request.env["website.page"]._trigger_malicious_arch_violation(hostile_vals)

        # blog.post's own write() override (models/blog_post.py) is the real enforcement
        # boundary -- it re-checks ownership via check_access()/
        # _check_proxy_ownership_write(), restricts a non-admin caller to this exact
        # field set regardless of what's passed here, and applies the service-account
        # escalation, cache-invalidation and Cloudflare-purge side effects every other
        # blog.post write in this module already relies on.
        post.write(
            {
                "name": name,
                "content": sanitized_content,
                "is_published": bool(is_published),
            }
        )
        return request.redirect(f"/blog/{post.blog_id.id}/post/{post.id}")

    @http.route("/user-websites/documentation", type="http", auth="user", website=True)
    # [@ANCHOR: user_websites:COMM_documentation]
    def documentation(self, **kwargs):
        # # Tested by [@ANCHOR: user_websites:test_documentation_route]
        # We explicitly use request.env here instead of env_svc to ensure
        # the current user has the correct portal/public access rights to view the article,
        # avoiding artificial AccessErrors from the backend service account.

        # Enforce strict schema contract. Let missing models fail loudly.
        article = request.env["knowledge.article"].search(
            [
                (
                    "name",
                    "=ilike",
                    "User Websites Documentation%",
                )
            ],
            limit=1,
        )
        if article and article.website_url:
            return request.redirect(article.website_url)

        return request.render("user_websites.documentation_page", {})

    @http.route("/community", type="http", auth="public", website=True)
    # [@ANCHOR: user_websites:COMM_community_directory]
    def community_directory(self, **kwargs):
        # # Tested by [@ANCHOR: user_websites:test_tour_community_directory]
        pager = {
            "page_count": 0,
            "page": dict(),
            "page_previous": dict(),
            "page_next": dict(),
        }

        # Load directory entries (Users who opted in)
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        entries = env_svc["user_websites.public_directory_view"].search([], limit=1000)

        return request.render(
            "user_websites.community_directory", {"pager": pager, "entries": entries}
        )

    @http.route("/my/privacy", type="http", auth="user", website=True)
    # [@ANCHOR: user_websites:COMM_privacy_dashboard]
    def privacy_dashboard(self, **kwargs):
        return request.render("user_websites.portal_my_privacy", {})

    @http.route("/my/privacy/export", type="http", auth="user", website=True)
    # [@ANCHOR: user_websites:COMM_privacy_export]
    def privacy_export(self, **kwargs):
        # # Tested by [@ANCHOR: user_websites:test_gdpr_export_api]
        user = request.env.user
        data = user._get_gdpr_export_data()
        streamed = user._get_gdpr_streamed_keys()
        
        def generate():
            yield "{"
            first_key = True
            for k, v in data.items():
                if not first_key:
                    yield ","
                yield f'"{k}": {json.dumps(v)}'
                first_key = False
            
            for k, generator_func in streamed.items():
                if not first_key:
                    yield ","
                yield f'"{k}": ['
                first_item = True
                for item in generator_func():
                    if not first_item:
                        yield ","
                    yield json.dumps(item)
                    first_item = False
                yield "]"
                first_key = False
            yield "}"

        headers = [
            ("Content-Type", "application/json"),
            ("Content-Disposition", 'attachment; filename="gdpr_export.json"'),
            # A person's whole data export: never stored by a browser cache, a proxy or Cloudflare.
            ("Cache-Control", "private, no-store"),
        ]
        return Response(generate(), headers=headers)

    @http.route("/my/privacy/export.zip", type="http", auth="user", website=True)
    # [@ANCHOR: user_websites:COMM_privacy_export_zip]
    def privacy_export_zip(self, **kwargs):
        # # Tested by [@ANCHOR: user_websites:test_gdpr_export_zip_redirect]
        # Mints a short-lived, single-use token (ham.gdpr.export.token) and
        # redirects the browser to the standalone gdpr_csv_export daemon's
        # own download endpoint -- per docs/proposals/GDPR_CSV_EXPORT.md, the
        # actual CSV/zip streaming happens entirely in that daemon, not this
        # Odoo worker, so a slow client's download never ties up the shared
        # worker pool. This controller call itself is the only Odoo-worker
        # time this export costs on the redirect path.
        token = request.env["ham.gdpr.export.token"].create_for_current_user()
        response = request.redirect("/api/v1/gdpr_export/download")
        # Hardening, 2026-09-10 (Bruce approved "harden both" during the
        # ses_webhook/GDPR-token-handoff review): the token used to travel
        # in the redirect URL's own query string, where it would land in
        # this worker's and the daemon's own HTTP access logs, the
        # browser's history, and any Referer header sent onward from the
        # download page. A short-lived, HttpOnly, SameSite=Strict cookie
        # scoped to exactly the daemon's own download path crosses the same
        # single redirect hop but never appears in a URL anywhere. `secure`
        # tracks the request's own real scheme (https in production behind
        # the reverse proxy, plain http in local dev/test) rather than
        # being hardcoded True, so this doesn't silently break local
        # testing while still being Secure wherever it actually matters.
        response.set_cookie(
            "gdpr_export_token",
            token,
            max_age=TOKEN_EXPIRY_MINUTES * 60,
            path="/api/v1/gdpr_export/",
            secure=request.httprequest.scheme == "https",
            httponly=True,
            samesite="Strict",
        )
        return response

    @http.route(
        "/my/privacy/delete_content",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_privacy_delete_content]
    def privacy_delete_content(self, **kwargs):
        user = request.env.user
        user._execute_gdpr_erasure()
        # The account is deactivated by now, so any auth="user" landing page
        # would bounce to the login form and the person would never see that
        # the erasure happened. Land on the public, static confirmation page.
        return request.redirect("/privacy/erased")

    @http.route("/privacy/erased", type="http", auth="public", website=True)
    # [@ANCHOR: user_websites:COMM_privacy_erased]
    def privacy_erased(self, **kwargs):
        # # Tested by [@ANCHOR: user_websites:test_privacy_erased_public_page]
        # Public on purpose: the erased account's session is gone. The page is
        # identical for everyone and takes no input (query string ignored,
        # nothing reflected, no redirect target), so it cannot be used to
        # probe whether an account ever existed.
        return request.render("user_websites.privacy_erased", {})

    @http.route(
        "/website/submit_appeal",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    def submit_appeal(self, reason="", group_id=None, **post):
        # [@ANCHOR: UX_SUBMIT_APPEAL]
        if not reason:
            return request.redirect("/my/home?error=missing_reason")

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        try:
            vals = {
                "reason": reason,
                "state": "new",
            }
            if group_id:
                group = env_svc["user.websites.group"].browse(int(group_id))
                if not group or request.env.user.id not in group.member_ids.ids:
                    return request.redirect("/my/home?error=appeal_failed")
                vals["group_id"] = group.id
            else:
                vals["user_id"] = request.env.user.id

            env_svc["content.violation.appeal"].create(vals)
        except (KeyError, ValueError) as e:   # Tested by [@ANCHOR: UX_SUBMIT_APPEAL]
            _logger.warning("Appeal creation failed: %s", e)
            return request.redirect("/my/home?error=appeal_failed")

        return request.redirect("/my/home?appeal_submitted=1")

    # sitemap=False: an admin-only JSON API, not public content -- real bug found
    # live in production, 2026-09-23: `website=True` alone made it sitemap-eligible
    # by default, listing an internal moderation endpoint in the public sitemap.
    @http.route(
        "/api/v1/user_websites/pending_reports",
        type="http",
        auth="public",
        website=True,
        sitemap=False,
    )
    # [@ANCHOR: user_websites:COMM_pending_reports]
    def pending_reports(self, **kwargs):
        user = request.env.user
        if user._is_public() or (
            not user.has_group("user_websites.group_user_websites_administrator")
            and not user.has_group("base.group_system")
        ):
            # Prevent JS Fetch from raising FATAL tracebacks during headless UI Tours
            return request.make_response(
                json.dumps({"count": 0, "error": "Forbidden"}),
                status=200,
                headers=[("Content-Type", "application/json")],
            )

        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")
        count = env_svc["content.violation.report"].search_count(
            [("state", "=", "new")], limit=1000
        )

        latest = env_svc["content.violation.report"].search(
            [("state", "=", "new")], order="id desc", limit=1
        )
        return request.make_response(
            json.dumps(
                {
                    "count": count,
                    "latest_id": latest.id or 0,
                    # Back-office moderation queue (pending reports only);
                    # only ever returned to moderators, see the gate above.
                    "url": "/odoo/action-user_websites.action_content_violation_report_pending",
                }
            ),
            status=200,
            headers=[("Content-Type", "application/json")],
        )

    @http.route(
        "/<uwslug:website_slug>/subscribe",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    def subscribe(self, website_slug, **kwargs):
        # [@ANCHOR: UX_SUBSCRIBE]
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        profile_user = (
            env_svc["res.users"].browse(user_id) if user_id else env_svc["res.users"]
        )
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        profile_group = (
            env_svc["user.websites.group"].browse(group_id)
            if group_id
            else env_svc["user.websites.group"]
        )

        if not profile_user and not profile_group:
            raise request.not_found()

        record_id = profile_user.partner_id.id if profile_user else profile_group.id
        model = "res.partner" if profile_user else "user.websites.group"

        env_svc[model].browse(record_id).message_subscribe(
            [request.env.user.partner_id.id]
        )
        return request.make_response(
            json.dumps({"success": True}),
            headers=[("Content-Type", "application/json")],
        )

    @http.route(
        "/website/unsubscribe/<string:model>/<int:record_id>/<int:partner_id>/<int:timestamp>/<string:token>",
        type="http",
        auth="public",
        website=True,
        csrf=True,
    )  # fmt: skip
    # [@ANCHOR: user_websites:COMM_unsubscribe]
    def unsubscribe(self, model, record_id, partner_id, timestamp, token, **kwargs):
        # # Tested by [@ANCHOR: test_unsubscribe_secret]
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        if model not in ("res.partner", "user.websites.group"):
            res_content = json.dumps({"error": "Forbidden"})
            res_headers = [("Content-Type", "application/json")]
            return request.make_response(
                res_content,
                status=403,
                headers=res_headers,
            )

        record = env_svc[model].browse(record_id)
        if not record.exists():
            res_content = json.dumps({"error": "Forbidden"})
            res_headers = [("Content-Type", "application/json")]
            return request.make_response(
                res_content,
                status=403,
                headers=res_headers,
            )

        db_secret = utils._get_crypto_secret()
        if not db_secret:
            return request.make_response(
                json.dumps({"error": "Forbidden"}),
                status=403,
                headers=[("Content-Type", "application/json")],
            )
        message = f"{model}-{record_id}-{partner_id}-{timestamp}".encode("utf-8")
        expected_token = hmac.new(
            db_secret.encode("utf-8"), message, hashlib.sha256
        ).hexdigest()

        if not hmac.compare_digest(expected_token, token):
            return request.make_response(
                json.dumps({"error": "Forbidden"}),
                status=403,
                headers=[("Content-Type", "application/json")],
            )

        record.message_unsubscribe([partner_id])

        return request.render(
            "user_websites.unsubscribe_success", {"record_name": model}
        )

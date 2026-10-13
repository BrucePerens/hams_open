# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The site owner's own interface: dashboard, page editor, image upload, publish toggles.

Bruce, 2026-10-12: "I am on my own 'K6BP' website through user websites and I don't see any
interface for creating anything." The models and a bare blog-post form existed; nothing on the
site itself led to them. This controller is the missing owner-only interface:

* /my/site/<slug>                      dashboard: pages, posts, images, settings
* /my/site/<slug>/page/new|<id>        WYSIWYG page editor (title, address, body, published)
* /my/site/<slug>/page/<id>/publish    publish / unpublish (kept, never deleted)
* /my/site/<slug>/post/<id>/publish    publish / unpublish a blog post (kept, never deleted)
* /my/site/<slug>/image                image upload (JSON for the editor, redirect for the dashboard)

Security model, all of it enforced here and again by the models:

* Ownership is decided from the database by _resolve_site(): the caller must be the user whose
  slug it is, or a member of the group. Anyone else gets a 404, whatever their group (an internal
  administrator included: the owner interface edits the owner's own content only).
* Reads of other tables run in the user_websites service account's environment
  (utils._get_service_env), never .sudo(). Creates of pages and attachments run there too, as the
  existing create_site() does, after the ownership check above; edits of existing pages and posts
  run in the caller's own environment so website.page.write() and blog.post.write() (ownership
  guard, field allow-list, quotas) stay the enforced boundary.
* Every body goes through models.member_html.sanitize_member_html() before it is stored, whoever
  the caller is. Hostile markup (script, frames, event handlers, QWeb directives) is stripped and
  also filed as an automated content violation through the existing
  website.page._trigger_malicious_arch_violation().
* POST routes carry Odoo's CSRF check. Writes are rate limited per user in Redis; bodies,
  titles and images have size limits. Delete removes a page or post from view and the record is retained
  (archived, restorable); there is no hard delete and no promise of erasure.
"""
import base64
import functools
import json
import logging
from urllib.parse import quote

import redis
from lxml import etree, html as lxml_html
from markupsafe import Markup
from werkzeug.exceptions import ServiceUnavailable, TooManyRequests

from odoo import fields, http
from odoo.http import request
from odoo.addons.edge_routing.utils import slugify
from odoo.addons.user_websites.controllers.main import redis_client, UserWebsitesController
from odoo.addons.user_websites.models.site_slug import legacy_slug_target
from odoo.addons.user_websites.models.member_html import (
    MAX_BODY_CHARS,
    PAGE_SLUG_RE,
    RESERVED_PAGE_SLUGS,
    sanitize_member_html,
)

_logger = logging.getLogger(__name__)

SVC = "user_websites.user_websites_service_account"
MAX_TITLE_CHARS = 120
MAX_IMAGE_BYTES = 2 * 1024 * 1024
MAX_IMAGES_PER_SITE = 100
WRITES_PER_MINUTE = 30
UPLOADS_PER_MINUTE = 10
IMAGE_MAGIC = (
    (b"\xff\xd8\xff", "image/jpeg"),
    (b"\x89PNG\r\n\x1a\n", "image/png"),
    (b"GIF87a", "image/gif"),
    (b"GIF89a", "image/gif"),
)
IMAGE_DESCRIPTION = "user_websites site image"
WRAP_CLASS = "uw-member-page"


def _image_mimetype(data):
    # [@ANCHOR: user_websites:owner_image_mimetype]
    # Verified by [@ANCHOR: test_owner_image_upload_rules]
    for magic, mimetype in IMAGE_MAGIC:
        if data.startswith(magic):
            return mimetype
    if data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "image/webp"
    return None


def _page_arch(site_slug, page_key, title, body_html):
    """Page arch: the shared member-page frame around already sanitized body markup."""
    # [@ANCHOR: user_websites:owner_page_arch]
    # Verified by [@ANCHOR: test_owner_page_create_edit_publish]
    if body_html:
        fragment = lxml_html.fragment_fromstring(body_html, create_parent="div")
        body_xml = "".join(
            [fragment.text and _escape(fragment.text) or ""]
            + [etree.tostring(child, encoding="unicode", method="xml") for child in fragment]
        )
    else:
        body_xml = ""
    return (
        f'<t name="{_escape_attr(title)}" t-name="user_websites.page_{page_key}">'
        '<t t-call="user_websites.template_default_home">'
        f'<div id="wrap" class="oe_structure {WRAP_CLASS}">{body_xml}</div>'
        "</t></t>"
    )


def _escape(text):
    # [@ANCHOR: user_websites:fn_owner_escape]
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def _escape_attr(text):
    # [@ANCHOR: user_websites:fn_owner_escape_attr]
    return _escape(text).replace('"', "&quot;")


def _arch_body(arch_base):
    """(body_html, editable) from a stored page arch; editable False for builder-made layouts."""
    # [@ANCHOR: user_websites:owner_page_arch_body]
    # Verified by [@ANCHOR: test_owner_page_create_edit_publish]
    if not arch_base:
        return "", True
    try:
        root = etree.fromstring(arch_base.encode() if isinstance(arch_base, str) else arch_base)
    except etree.XMLSyntaxError:
        return "", False
    wraps = root.xpath('//div[@id="wrap"]')
    if not wraps:
        return "", False
    wrap = wraps[0]
    for el in wrap.iter():
        if not isinstance(el.tag, str):
            continue
        if el.tag == "t" or any(name.startswith("t-") for name in el.attrib):
            return "", False
    inner = (wrap.text or "") and _escape(wrap.text)
    inner += "".join(etree.tostring(child, encoding="unicode", method="html") for child in wrap)
    return inner, True


class SiteResponse(Exception):
    """Raised by _resolve_site() to end a request with a response of its own (a redirect from a
    retired slug, the "what is missing" page for an ineligible member).

    Not an HTTPException on purpose: Odoo's frontend error handler turns any raised HTTPException into
    its own error page (a 301 becomes a 418), throwing the response away. answers_site_response() turns
    this into the route's return value instead."""

    def __init__(self, response):
        super().__init__("owner site response")
        self.response = response


def answers_site_response(route):
    """Route decorator: a SiteResponse raised anywhere below becomes the route's response."""
    # [@ANCHOR: user_websites:fn_answers_site_response]
    @functools.wraps(route)
    def wrapper(*args, **kwargs):
        try:
            return route(*args, **kwargs)
        except SiteResponse as ended:
            return ended.response

    return wrapper


class UserWebsitesOwnerController(http.Controller):

    # ------------------------------------------------------------------ helpers

    def _svc(self):
        # [@ANCHOR: user_websites:fn_owner_svc]
        return request.env["zero_sudo.security.utils"]._get_service_env(SVC)

    def _resolve_site(self, site_slug):
        """(profile_user, profile_group, label) for a site the caller may manage, else 404."""
        # [@ANCHOR: user_websites:owner_resolve_site]
        # Verified by [@ANCHOR: test_owner_site_denied_for_other_member]
        svc = self._svc()
        user = request.env.user
        user_id = svc["res.users"].get_record_by_slug(site_slug)
        group_id = svc["user.websites.group"].get_record_by_slug(site_slug)
        profile_user = svc["res.users"].browse(user_id) if user_id else svc["res.users"]
        profile_group = (
            svc["user.websites.group"].browse(group_id) if group_id else svc["user.websites.group"]
        )
        if profile_user:
            if profile_user.id != user.id or profile_user.is_suspended_from_websites:
                raise request.not_found()
            gaps = profile_user._website_eligibility_gaps()
            if gaps:
                # [@ANCHOR: user_websites:owner_site_ineligible]
                # Verified by [@ANCHOR: test_site_eligibility_hook]
                # Say what is missing instead of a 404, and hide anything still public.
                request.env["res.users"].reconcile_website_eligibility([profile_user.id])
                raise SiteResponse(
                    request.render(
                        "user_websites.owner_site_ineligible",
                        {"gaps": gaps, "site_slug": profile_user.website_slug, "main_object": profile_user},
                        status=403,
                    )
                )
        elif profile_group:
            if user.id not in profile_group.member_ids.ids or profile_group.is_suspended_from_websites:
                raise request.not_found()
        else:
            self._redirect_legacy_slug(site_slug)
            raise request.not_found()
        return profile_user, profile_group

    def _redirect_legacy_slug(self, site_slug):
        """Raise a redirect to the same owner URL under the live slug when `site_slug` is retired."""
        # [@ANCHOR: user_websites:owner_redirect_legacy_slug]
        # Verified by [@ANCHOR: test_site_slug_one_site]
        target = legacy_slug_target(request.env, site_slug)
        if not target:
            return
        path = request.httprequest.path
        prefix = f"/my/site/{site_slug}"
        if not path.startswith(prefix):
            return
        new_url = f"/my/site/{quote(target, safe='')}" + path[len(prefix):]
        query = request.httprequest.query_string.decode("utf-8", "replace")
        if query:
            new_url += "?" + query
        # 301 for a GET; 308 for anything else keeps the method and body (a form left open across a
        # rename still lands on the live site, session and CSRF token unchanged).
        code = 301 if request.httprequest.method in ("GET", "HEAD") else 308
        raise SiteResponse(request.redirect(new_url, code=code, local=True))

    def _owner_domain(self, profile_user, profile_group):
        # [@ANCHOR: user_websites:fn_owner_owner_domain]
        if profile_user:
            return [("owner_user_id", "=", profile_user.id)]
        return [("user_websites_group_id", "=", profile_group.id)]

    def _rate_limit(self, bucket, limit):
        # [@ANCHOR: user_websites:owner_rate_limit]
        # Verified by [@ANCHOR: test_owner_write_rate_limited]
        key = f"uw:rl:{request.env.cr.dbname}:{bucket}:{request.env.user.id}"
        try:
            count = redis_client.incr(key)
            if count == 1:
                redis_client.expire(key, 60)
        except redis.exceptions.RedisError as e:
            _logger.error("Owner-site rate limiter unavailable: %s", e)
            raise ServiceUnavailable("The site tools are temporarily unavailable.")
        if count > limit:
            raise TooManyRequests("Too many changes in a minute. Please wait and try again.")

    def _qcontext(self, profile_user, profile_group, **extra):
        # [@ANCHOR: user_websites:fn_owner_qcontext]
        main = profile_user or profile_group
        values = {
            "profile_user": profile_user,
            "profile_group": profile_group,
            "main_object": main,
            "resolved_slug": main.website_slug,
            "member_site": True,
            "is_owner": True,
            "site_slug": main.website_slug,
            "error": request.params.get("error"),
            "saved": request.params.get("saved"),
        }
        values.update(extra)
        return values

    def _report_if_hostile(self, hostile, profile_user, profile_group, url):
        # [@ANCHOR: user_websites:fn_owner_report_if_hostile]
        if not hostile:
            return
        vals = {"url": url}
        if profile_user:
            vals["owner_user_id"] = profile_user.id
        elif profile_group:
            vals["user_websites_group_id"] = profile_group.id
        request.env["website.page"]._trigger_malicious_arch_violation(vals)

    def _clean_body(self, content):
        # [@ANCHOR: user_websites:fn_owner_clean_body]
        if len(content or "") > MAX_BODY_CHARS:
            return None, False
        return sanitize_member_html(content)

    # ---------------------------------------------------------------- dashboard

    @http.route("/my/site", type="http", auth="user", website=True, sitemap=False)
    # [@ANCHOR: user_websites:COMM_owner_site_entry]
    def owner_site_entry(self, **kwargs):
        slug = request.env.user.website_slug
        if not slug:
            return request.redirect("/my/home")
        return request.redirect(f"/my/site/{quote(slug, safe='')}", code=303)

    @http.route("/my/site/<string:site_slug>", type="http", auth="user", website=True, sitemap=False)
    # [@ANCHOR: user_websites:COMM_owner_site_dashboard]
    # Verified by [@ANCHOR: test_owner_site_dashboard_lists_content]
    @answers_site_response
    def owner_site_dashboard(self, site_slug, **kwargs):
        profile_user, profile_group = self._resolve_site(site_slug)
        svc = self._svc()
        domain = self._owner_domain(profile_user, profile_group)
        pages = svc["website.page"].search(domain, order="url", limit=200)
        posts = svc["blog.post"].search(domain, order="create_date desc, id desc", limit=200)
        res_model, res_id = self._attachment_owner(profile_user, profile_group)
        images = svc["ir.attachment"].search(
            [
                ("res_model", "=", res_model),
                ("res_id", "=", res_id),
                ("description", "=", IMAGE_DESCRIPTION),
            ],
            order="id desc",
            limit=MAX_IMAGES_PER_SITE,
        )
        home_url = f"/{(profile_user or profile_group).website_slug}"
        deleted_pages = svc["website.page"].with_context(active_test=False).search(
            domain + [("deleted_by_owner", "!=", False)], order="deleted_by_owner desc", limit=100
        )
        deleted_posts = svc["blog.post"].with_context(active_test=False).search(
            domain + [("deleted_by_owner", "!=", False)], order="deleted_by_owner desc", limit=100
        )
        return request.render(
            "user_websites.owner_site_dashboard",
            self._qcontext(
                profile_user,
                profile_group,
                pages=pages,
                posts=posts,
                deleted_pages=deleted_pages,
                deleted_posts=deleted_posts,
                images=images,
                home_url=home_url,
                max_images=MAX_IMAGES_PER_SITE,
                max_image_mb=MAX_IMAGE_BYTES // (1024 * 1024),
                has_home=any(p.url == home_url for p in pages),
            ),
        )

    # -------------------------------------------------------------- page editor

    def _own_page(self, profile_user, profile_group, page_id):
        # [@ANCHOR: user_websites:fn_owner_own_page]
        page = self._svc()["website.page"].browse(page_id)
        if not page.exists():
            raise request.not_found()
        owner = (profile_user or profile_group)
        if (profile_user and page.owner_user_id != profile_user) or (
            profile_group and page.user_websites_group_id != profile_group
        ):
            raise request.not_found()
        slug = owner.website_slug
        if page.url != f"/{slug}" and not page.url.startswith(f"/{slug}/"):
            raise request.not_found()
        return page

    @http.route(
        "/my/site/<string:site_slug>/page/new", type="http", auth="user", website=True, sitemap=False
    )
    # [@ANCHOR: user_websites:COMM_owner_page_new]
    @answers_site_response
    def owner_page_new(self, site_slug, **kwargs):
        profile_user, profile_group = self._resolve_site(site_slug)
        return request.render(
            "user_websites.owner_page_edit",
            self._qcontext(
                profile_user,
                profile_group,
                page=None,
                title=request.params.get("title", ""),
                page_slug=request.params.get("page_slug", ""),
                body=Markup(""),
                published=False,
                can_edit_body=True,
                is_home=False,
            ),
        )

    @http.route(
        "/my/site/<string:site_slug>/page/<int:page_id>",
        type="http",
        auth="user",
        website=True,
        sitemap=False,
    )
    # [@ANCHOR: user_websites:COMM_owner_page_edit]
    @answers_site_response
    def owner_page_edit(self, site_slug, page_id, **kwargs):
        profile_user, profile_group = self._resolve_site(site_slug)
        page = self._own_page(profile_user, profile_group, page_id)
        body, editable = _arch_body(page.arch_base)
        clean_body, _hostile = sanitize_member_html(body)
        slug = (profile_user or profile_group).website_slug
        return request.render(
            "user_websites.owner_page_edit",
            self._qcontext(
                profile_user,
                profile_group,
                page=page,
                title=page.name,
                page_slug=page.url.rsplit("/", 1)[-1] if page.url != f"/{slug}" else "",
                body=Markup(clean_body),
                published=page.website_published,
                can_edit_body=editable,
                is_home=page.url == f"/{slug}",
            ),
        )

    @http.route(
        "/my/site/<string:site_slug>/page/save",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_owner_page_save]
    # Verified by [@ANCHOR: test_owner_page_create_edit_publish]
    @answers_site_response
    def owner_page_save(
        self, site_slug, page_id="", title="", page_slug="", content="", is_published="", **kwargs
    ):
        profile_user, profile_group = self._resolve_site(site_slug)
        self._rate_limit("write", WRITES_PER_MINUTE)
        owner = profile_user or profile_group
        slug = owner.website_slug
        title = (title or "").strip()
        if not title or len(title) > MAX_TITLE_CHARS:
            return self._back_to_editor(slug, page_id, "title")
        body, hostile = self._clean_body(content)
        if body is None:
            return self._back_to_editor(slug, page_id, "too_long")

        if page_id:
            if not str(page_id).isdigit():
                raise request.not_found()
            page = self._own_page(profile_user, profile_group, int(page_id))
            _old_body, editable = _arch_body(page.arch_base)
            if not editable:
                return self._back_to_editor(slug, page_id, "not_editable")
            url = page.url
            key = url.strip("/").replace("/", "_") or slug
        else:
            page = None
            page_slug = (page_slug or slugify(title)).strip().lower()
            if not PAGE_SLUG_RE.match(page_slug) or page_slug in RESERVED_PAGE_SLUGS:
                return self._back_to_editor(slug, page_id, "address")
            url = f"/{slug}/{page_slug}"
            key = f"{slug}_{page_slug}"
            if self._svc()["website.page"].search_count([("url", "=", url)]):
                return self._back_to_editor(slug, page_id, "exists")

        arch = _page_arch(slug, key.replace("-", "_"), title, body)
        published = bool(is_published)
        self._report_if_hostile(hostile, profile_user, profile_group, url)
        if page:
            request.env["website.page"].browse(page.id).write(
                {"name": title, "arch_base": arch, "website_published": published}
            )
        else:
            vals = {
                "url": url,
                "name": title,
                "type": "qweb",
                "arch_base": arch,
                "website_id": request.website.id,
                "website_published": published,
            }
            if profile_user:
                vals["owner_user_id"] = profile_user.id
            elif profile_group:
                vals["user_websites_group_id"] = profile_group.id
            page = self._svc()["website.page"].create(vals)
        return request.redirect(f"/my/site/{quote(slug, safe='')}/page/{page.id}?saved=1")

    def _back_to_editor(self, slug, page_id, error):
        # [@ANCHOR: user_websites:fn_owner_back_to_editor]
        base = f"/my/site/{quote(slug, safe='')}/page/" + (page_id if str(page_id).isdigit() else "new")
        return request.redirect(f"{base}?error={error}")

    @http.route(
        "/my/site/<string:site_slug>/page/<int:page_id>/publish",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_owner_page_publish]
    # Verified by [@ANCHOR: test_owner_page_create_edit_publish]
    @answers_site_response
    def owner_page_publish(self, site_slug, page_id, publish="", **kwargs):
        profile_user, profile_group = self._resolve_site(site_slug)
        self._rate_limit("write", WRITES_PER_MINUTE)
        page = self._own_page(profile_user, profile_group, page_id)
        request.env["website.page"].browse(page.id).write({"website_published": publish == "1"})
        return request.redirect(
            request.params.get("next") if (request.params.get("next") or "").startswith("/my/site/")
            else f"/my/site/{quote(site_slug, safe='')}?saved=1",
            code=303,
        )

    @http.route(
        "/my/site/<string:site_slug>/post/<int:post_id>/publish",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_owner_post_publish]
    # Verified by [@ANCHOR: test_owner_post_publish_toggle]
    @answers_site_response
    def owner_post_publish(self, site_slug, post_id, publish="", **kwargs):
        profile_user, profile_group = self._resolve_site(site_slug)
        self._rate_limit("write", WRITES_PER_MINUTE)
        post = UserWebsitesController()._get_own_blog_post_for_edit(post_id)
        owner_ok = post and (
            (profile_user and post.owner_user_id.id == profile_user.id)
            or (profile_group and post.user_websites_group_id.id == profile_group.id)
        )
        if not owner_ok:
            raise request.not_found()
        post.write({"is_published": publish == "1"})
        return request.redirect(f"/my/site/{quote(site_slug, safe='')}?saved=1", code=303)


    # ------------------------------------------------------------ delete / restore

    def _own_post(self, profile_user, profile_group, post_id):
        # [@ANCHOR: user_websites:fn_owner_own_post]
        post = self._svc()["blog.post"].with_context(active_test=False).browse(post_id)
        if not post.exists():
            raise request.not_found()
        if (profile_user and post.owner_user_id != profile_user) or (
            profile_group and post.user_websites_group_id != profile_group
        ):
            raise request.not_found()
        return post

    def _own_page_any(self, profile_user, profile_group, page_id):
        """Like _own_page, but also finds a page the owner deleted (its address is parked)."""
        page = self._svc()["website.page"].with_context(active_test=False).browse(page_id)
        if not page.exists():
            raise request.not_found()
        if (profile_user and page.owner_user_id != profile_user) or (
            profile_group and page.user_websites_group_id != profile_group
        ):
            raise request.not_found()
        return page

    def _back(self, site_slug, flag):
        return request.redirect(f"/my/site/{quote(site_slug, safe='')}?{flag}", code=303)

    @http.route(
        "/my/site/<string:site_slug>/<any(page,post):kind>/<int:item_id>/delete",
        type="http",
        auth="user",
        methods=["GET"],
        website=True,
        sitemap=False,
    )
    # [@ANCHOR: user_websites:COMM_owner_delete_confirm]
    # Verified by [@ANCHOR: test_owner_delete_page_and_post]
    @answers_site_response
    def owner_delete_confirm(self, site_slug, kind, item_id, **kwargs):
        """The no-JavaScript confirmation step (the dashboard shows the same question in a dialog)."""
        profile_user, profile_group = self._resolve_site(site_slug)
        if kind == "page":
            item = self._own_page(profile_user, profile_group, item_id)
            title = item.name
            is_home = item.url == f"/{(profile_user or profile_group).website_slug}"
        else:
            item = self._own_post(profile_user, profile_group, item_id)
            title = item.name
            is_home = False
        return request.render(
            "user_websites.owner_delete_confirm",
            self._qcontext(profile_user, profile_group, kind=kind, item=item, item_title=title, is_home=is_home),
        )

    @http.route(
        "/my/site/<string:site_slug>/<any(page,post):kind>/<int:item_id>/delete",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_owner_delete]
    # Verified by [@ANCHOR: test_owner_delete_page_and_post]
    @answers_site_response
    def owner_delete(self, site_slug, kind, item_id, **kwargs):
        """Delete = removed from the site and from every public surface at once, kept archived.

        The record stays (active False, deleted_by_owner stamped): the platform retains data and this
        interface never promises erasure. A page is moved to a private address so its own can be reused.
        The home page cannot be deleted. Runs as the service account after the ownership checks."""
        profile_user, profile_group = self._resolve_site(site_slug)
        self._rate_limit("write", WRITES_PER_MINUTE)
        slug = (profile_user or profile_group).website_slug
        now = fields.Datetime.now()
        if kind == "page":
            page = self._own_page(profile_user, profile_group, item_id)
            if page.url == f"/{slug}":
                return self._back(site_slug, "error=home_delete")
            # no_cow: archive THIS page's view, not a site-specific copy of a generic one.
            page.with_context(no_cow=True).write(
                {
                    "deleted_original_url": page.url,
                    "url": f"/{slug}/deleted-{page.id}",
                    "website_published": False,
                    "active": False,
                    "deleted_by_owner": now,
                }
            )
        else:
            post = self._own_post(profile_user, profile_group, item_id)
            post.write({"is_published": False, "active": False, "deleted_by_owner": now})
        return self._back(site_slug, "saved=deleted")

    @http.route(
        "/my/site/<string:site_slug>/<any(page,post):kind>/<int:item_id>/restore",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_owner_restore]
    # Verified by [@ANCHOR: test_owner_delete_page_and_post]
    @answers_site_response
    def owner_restore(self, site_slug, kind, item_id, **kwargs):
        """Bring back something the owner deleted. It returns UNPUBLISHED: publishing is a separate,
        deliberate act. Only items the owner deleted can be restored here (a page archived by
        moderation, for instance, cannot)."""
        profile_user, profile_group = self._resolve_site(site_slug)
        self._rate_limit("write", WRITES_PER_MINUTE)
        slug = (profile_user or profile_group).website_slug
        if kind == "page":
            page = self._own_page_any(profile_user, profile_group, item_id)
            if not page.deleted_by_owner:
                raise request.not_found()
            url = page.deleted_original_url or f"/{slug}/restored-{page.id}"
            if not (url == f"/{slug}" or url.startswith(f"/{slug}/")) or self._svc()["website.page"].search_count(
                [("url", "=", url), ("website_id", "=", page.website_id.id)]
            ):
                url = f"/{slug}/restored-{page.id}"
            page.with_context(no_cow=True).write(
                {"url": url, "active": True, "deleted_by_owner": False, "deleted_original_url": False}
            )
        else:
            post = self._own_post(profile_user, profile_group, item_id)
            if not post.deleted_by_owner:
                raise request.not_found()
            post.write({"active": True, "deleted_by_owner": False})
        return self._back(site_slug, "saved=restored")

    # ------------------------------------------------------------------- images

    def _attachment_owner(self, profile_user, profile_group):
        # [@ANCHOR: user_websites:fn_owner_attachment_owner]
        if profile_user:
            return "res.users", profile_user.id
        return "user.websites.group", profile_group.id

    @http.route(
        "/my/site/<string:site_slug>/image",
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: user_websites:COMM_owner_image_upload]
    # Verified by [@ANCHOR: test_owner_image_upload_rules]
    @answers_site_response
    def owner_image_upload(self, site_slug, ufile=None, json_reply="", **kwargs):
        profile_user, profile_group = self._resolve_site(site_slug)
        self._rate_limit("upload", UPLOADS_PER_MINUTE)
        wants_json = bool(json_reply)

        def reply(ok, message="", url="", status=200):
            if wants_json:
                return request.make_response(
                    json.dumps({"ok": ok, "error": message, "url": url}),
                    headers=[("Content-Type", "application/json")],
                    status=status if not ok else 200,
                )
            target = f"/my/site/{quote(site_slug, safe='')}"
            return request.redirect(
                f"{target}?saved=1" if ok else f"{target}?error=image", code=303
            )

        if ufile is None:
            return reply(False, "Choose an image file.", status=400)
        data = ufile.read(MAX_IMAGE_BYTES + 1)
        if not data:
            return reply(False, "That file is empty.", status=400)
        if len(data) > MAX_IMAGE_BYTES:
            return reply(
                False,
                f"Images can be at most {MAX_IMAGE_BYTES // (1024 * 1024)} MB.",
                status=413,
            )
        mimetype = _image_mimetype(data)
        if not mimetype:
            return reply(False, "Use a JPEG, PNG, GIF or WebP image.", status=415)
        svc = self._svc()
        res_model, res_id = self._attachment_owner(profile_user, profile_group)
        if (
            svc["ir.attachment"].search_count(
                [
                    ("res_model", "=", res_model),
                    ("res_id", "=", res_id),
                    ("description", "=", IMAGE_DESCRIPTION),
                ]
            )
            >= MAX_IMAGES_PER_SITE
        ):
            return reply(False, "This site has reached its image limit.", status=409)
        name = (slugify((ufile.filename or "image").rsplit(".", 1)[0]) or "image")[:60]
        attachment = svc["ir.attachment"].create(
            {
                "name": name,
                "datas": base64.b64encode(data),
                "mimetype": mimetype,
                "public": True,
                "res_model": res_model,
                "res_id": res_id,
                "description": IMAGE_DESCRIPTION,
            }
        )
        return reply(True, url=f"/web/image/{attachment.id}")

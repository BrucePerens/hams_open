# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import urllib.parse

from odoo import api, models, tools
from odoo.addons.base.models.ir_http import SignedIntConverter
from odoo.http import request
from odoo.service import security
from werkzeug.exceptions import BadRequest

ROUTE_INT_LIMIT = 2**31


# [@ANCHOR: hams_base:bounded_int_converter]
class BoundedIntConverter(SignedIntConverter):
    """The ``<int:x>`` route converter, refusing a number a database id cannot be (a clean 404, not a SQL error)."""

    # [@ANCHOR: hams_base:bounded_int_converter_init]
    # Verified by [@ANCHOR: test_03_a_route_can_raise_the_limit]
    def __init__(self, map, fixed_digits=0, min=-ROUTE_INT_LIMIT + 1, max=ROUTE_INT_LIMIT - 1, signed=True):
        super().__init__(map, fixed_digits=fixed_digits, min=min, max=max, signed=signed)


# [@ANCHOR: hams_base_ir_http_ir_http]
class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _get_converters(cls):
        # [@ANCHOR: hams_base:bounded_int_converters]
        return {**super()._get_converters(), "int": BoundedIntConverter}

    @classmethod
    def _match(cls, path):
        # [@ANCHOR: hams_base_validate_session_before_website_match]
        """Validate the session before website's URL matching reads records with it.

        Odoo core's ``Request._serve_db`` builds ``request.env`` from the raw,
        unvalidated ``session.uid`` and then calls ``ir.http._match``; the
        session is only checked (and a dead one logged out) later, in
        ``_authenticate_explicit``. website/http_routing's ``_match`` reads
        ``website`` and ``res.lang`` records in that window, under the
        unvalidated uid, and http_routing's "temporarily grant the public
        user" step is skipped because ``session.uid`` is set. A session whose
        user has since been deleted therefore fails the ACL check there
        (``Access Denied by ACLs ... uid: <deleted id>, model: website``) and
        the visitor gets a bare 403 on every frontend page, instead of being
        logged out and served as the public user. Seen on hams.com production
        as repeated hits for a nonexistent ``uid: 99``.

        This runs the same check ``_authenticate_explicit`` runs, earlier.
        ``request`` is deliberately not passed to ``check_session``: the
        cursor is still read-only here, and ``_authenticate_explicit`` records
        the device log for a valid session afterwards as usual. The later
        call is an ormcache hit on ``_compute_session_token``.
        """
        if request and request.session.uid is not None:
            if not security.check_session(request.session, request.env):
                request.session.logout(keep_db=True)
                request.env = api.Environment(
                    request.env.cr, None, request.session.context
                )
        return super()._match(path)

    @classmethod
    def _dispatch(cls, endpoint):
        # [@ANCHOR: hams_base:refuse_nul_in_request_params]
        """Answer 400 to a request with a NUL character in any parameter.

        PostgreSQL cannot hold a NUL in a string, so a search box or form field that reaches the database with ``%00``
        in it (``/classifieds?search=a%00b``, a JSON string ``"\\u0000"``) fails with "A string literal cannot contain NUL
        (0x00) characters": a 500 and an exception ticket. No page of this site has a use for the character, so the
        request is refused once, here, for every route. Uploaded files are not text and are not looked at.
        """
        if _has_nul(request.params):
            raise BadRequest("A NUL character is not allowed in a request.")
        return super()._dispatch(endpoint)

    @classmethod
    def _url_localized(
        cls,
        url=None,
        lang_code=None,
        canonical_domain=None,
        prefetch_langs=False,
        force_default_lang=False,
    ):
        # [@ANCHOR: hams_base:canonical_url_survives_dot_segments]
        # Verified by [@ANCHOR: hams_base:test_canonical_url_survives_dot_segments]
        """Never let a hostile request path break the page that answers it.

        The website's 404 and error pages put a canonical ``<link>`` in their layout, built from the
        raw request path by this method. Odoo's ``tools.urls.urljoin`` refuses a path with a ``.`` or
        ``..`` segment (also percent-encoded, ``%2e%2e``) or bytes that are not UTF-8 by raising
        ``ValueError``. A scanner probing ``/foo/../x`` or ``/.//`` therefore turned a plain 404 into a
        server error, an unhandled-exception ticket, and a second traceback from the error page itself
        ("Couldn't render a template for http status 404"). Seen on hams.com production as hundreds of
        ``ValueError: Dot segments are not allowed`` a day.

        A canonical URL for such a path has no honest value, so the site root is used: the page is
        still a 404 and a crawler is told nothing wrong about any real page. Only the canonical
        (absolute) form can raise, so the relative form is left alone.
        """
        try:
            return super()._url_localized(
                url=url,
                lang_code=lang_code,
                canonical_domain=canonical_domain,
                prefetch_langs=prefetch_langs,
                force_default_lang=force_default_lang,
            )
        except ValueError:
            if not canonical_domain or not isinstance(canonical_domain, str):
                raise
            return tools.urls.urljoin(canonical_domain, "/")

    @classmethod
    def _url_for(cls, url_from, lang_code=None):
        # [@ANCHOR: hams_base:url_for_survives_dot_segments]
        # Verified by [@ANCHOR: hams_base:test_url_for_survives_dot_segments]
        """The site root for a local URL that still holds a dot segment after Odoo has tidied it.

        ``website.seo.metadata._default_website_meta`` (the ``og:url`` of every page that has a
        ``main_object``, including the 404 page) does ``url_join(domain, _url_for(request.httprequest.path))``.
        ``url_join`` raises ``ValueError("Dot segments are not allowed")``, also for a segment that only becomes
        ``..`` after percent-decoding. ``_url_for`` resolves a plain ``/a/../b`` itself but leaves a double-encoded
        ``/@fs/%2e%2e/x`` (the request path is decoded once, so ``%252e%252e`` arrives as ``%2e%2e``) alone, so a
        scanner's probe made the 404 page, and then the generic error page, raise: a 500 and an exception ticket
        (ticket 74, ``/@fs/.../self/environ``). ``_url_localized`` (above) covers the
        canonical ``<link>``; this covers the OpenGraph URL. A local URL with a dot segment has no honest value,
        so it becomes the site root; absolute URLs are left alone.
        """
        url = super()._url_for(url_from, lang_code=lang_code)
        if isinstance(url, str) and url.startswith("/") and not url.startswith("//") and _has_dot_segment(urllib.parse.urlsplit(url).path):
            return "/"
        return url


# [@ANCHOR: hams_base:has_dot_segment]
def _has_dot_segment(path):
    """True for a path ``tools.urls.urljoin`` would refuse: a ``.`` or ``..`` segment, also percent-encoded, or
    bytes that are not UTF-8."""
    try:
        decoded = urllib.parse.unquote(path, errors="strict")
    except UnicodeDecodeError:
        return True
    return any(segment in (".", "..") for segment in decoded.split("/"))


# [@ANCHOR: hams_base:has_nul]
def _has_nul(value):
    """True when a string anywhere in ``value`` (nested dicts, lists and tuples; dict keys too) holds a NUL character.
    Walks with a list, not recursion, so a deeply nested body cannot raise RecursionError."""
    pending = [value]
    while pending:
        item = pending.pop()
        if isinstance(item, str):
            if "\x00" in item:
                return True
        elif isinstance(item, dict):
            pending.extend(item.keys())
            pending.extend(item.values())
        elif isinstance(item, (list, tuple)):
            pending.extend(item)
    return False

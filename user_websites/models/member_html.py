# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Strict allow-list HTML sanitizer for what members write in the site editor.

The member-site editor (static/src/js/rich_editor.js) is a WYSIWYG: members never type markup.
What it sends is still untrusted, because anyone can POST anything to the save routes, so every
save goes through sanitize_member_html() in the controller, whoever the caller is. The model-level
sanitizers (blog.post._sanitize_member_content, website.page._sanitize_user_arch) skip
administrators and the service account; an internal administrator who owns a site is therefore
sanitized here, in the request path, before the model sees anything.

Allow-list, not block-list: only the tags and attributes the editor's toolbar can produce survive.
Everything else is unwrapped (its text kept) or dropped; <script>, <style>, <iframe>, <object>,
<embed>, forms and their relatives are dropped with their content. No style attribute, no class
except the three alignment classes, no data-* and no t-* (QWeb) attribute survives. Links keep only
http, https, mailto and same-site paths; images keep only same-site /web/image/ or /web/content/
paths, so a member cannot hot-link a tracking pixel from a visitor's browser.
"""
import re
from urllib.parse import urlsplit

from lxml import etree, html as lxml_html

from odoo.tools import html_sanitize

# [@ANCHOR: user_websites:member_html_sanitize]
# Verified by [@ANCHOR: test_member_html_sanitize_strips_hostile_markup]
ALLOWED_TAGS = frozenset(
    "p br hr h1 h2 h3 h4 strong b em i u s strike sub sup ul ol li blockquote pre code a img "
    "div span table thead tbody tr th td figure figcaption".split()
)
DROP_WITH_CONTENT = frozenset(
    "script style iframe frame frameset object embed applet form input button select textarea "
    "option link meta base head title noscript template math audio video source track canvas "
    "dialog".split()
)
ALIGN_CLASSES = {
    "left": "text-start",
    "start": "text-start",
    "center": "text-center",
    "right": "text-end",
    "end": "text-end",
    "justify": "text-justify",
}
ALLOWED_CLASSES = frozenset(ALIGN_CLASSES.values())
LINK_SCHEMES = frozenset({"http", "https", "mailto"})
IMAGE_PATH_RE = re.compile(r"^/web/(image|content)/[0-9]+(/[A-Za-z0-9._-]+)*(\?[A-Za-z0-9=&._-]*)?$")
TEXT_ALIGN_RE = re.compile(r"text-align\s*:\s*([a-z]+)", re.I)
MAX_BODY_CHARS = 200_000


def _clean_url(value, image=False):
    # [@ANCHOR: user_websites:fn_hmtl_clean_url]
    value = (value or "").strip()
    if not value or any(ord(c) < 32 for c in value):
        return None
    if image:
        return value if IMAGE_PATH_RE.match(value) else None
    if value.startswith("#"):
        return value
    parts = urlsplit(value)
    if not parts.scheme and not parts.netloc and value.startswith("/") and not value.startswith("//"):
        return value
    if parts.scheme.lower() in LINK_SCHEMES:
        return value
    return None


def _is_hostile(el):
    # [@ANCHOR: user_websites:fn_hmtl_is_hostile]
    """True when the element is something only a hand-made request could carry."""
    tag = el.tag if isinstance(el.tag, str) else ""
    if tag.lower() in DROP_WITH_CONTENT:
        return True
    for name, value in el.attrib.items():
        lname = name.lower()
        if lname.startswith("on") or lname.startswith("t-") or lname.startswith("xlink"):
            return True
        if lname in ("href", "src", "action", "formaction") and re.match(
            r"\s*(javascript|vbscript|data):", value or "", re.I
        ) and not (value or "").lower().startswith("data:image/"):
            return True
    return False


def sanitize_member_html(source):
    # [@ANCHOR: user_websites:fn_hmtl_sanitize_member_html]
    """Return (clean_html, hostile).

    clean_html is a well-formed HTML fragment containing only allow-listed markup. hostile is True
    when the input held something the toolbar cannot produce and an attacker would (script and
    frame tags, event-handler or QWeb attributes, javascript: URLs); the caller reports that
    through the content-violation pipeline.
    """
    source = (source or "")[:MAX_BODY_CHARS * 2]
    if not source.strip():
        return "", False
    try:
        root = lxml_html.fragment_fromstring(source, create_parent="div")
    except (etree.ParserError, ValueError):
        return "", False
    hostile = False
    for el in list(root.iter()):
        if el is root:
            continue
        if _is_hostile(el):
            hostile = True
    _clean_children(root)
    clean = etree.tostring(root, encoding="unicode", method="html")
    clean = clean[len("<div>"):-len("</div>")] if clean.startswith("<div>") else clean
    return clean.strip(), hostile


def _clean_children(parent):
    # [@ANCHOR: user_websites:fn_hmtl_clean_children]
    for child in list(parent):
        if not isinstance(child.tag, str):  # comment, processing instruction
            _drop(child)
            continue
        tag = child.tag.lower()
        if tag in DROP_WITH_CONTENT:
            _drop(child)
            continue
        if tag == "svg":
            _replace_with_sanitized_svg(child)
            continue
        _clean_children(child)
        if tag not in ALLOWED_TAGS:
            _unwrap(child)
            continue
        _clean_attributes(child, tag)
        if tag == "a" and not child.get("href"):
            _unwrap(child)
        elif tag == "img" and not child.get("src"):
            _drop(child)
        elif tag == "a":
            child.set("rel", "nofollow noopener ugc")


def _clean_attributes(el, tag):
    # [@ANCHOR: user_websites:fn_hmtl_clean_attributes]
    original = dict(el.attrib)
    style = original.get("style", "")
    cls = [c for c in (original.get("class") or "").split() if c in ALLOWED_CLASSES]
    match = TEXT_ALIGN_RE.search(style)
    if match and match.group(1).lower() in ALIGN_CLASSES:
        cls = [ALIGN_CLASSES[match.group(1).lower()]]
    for name in list(el.attrib):
        el.attrib.pop(name)
    if cls:
        el.set("class", " ".join(cls))
    if tag == "a":
        url = _clean_url(original.get("href"))
        if url:
            el.set("href", url)
        if original.get("title"):
            el.set("title", original["title"][:300])
    elif tag == "img":
        url = _clean_url(original.get("src"), image=True)
        if url:
            el.set("src", url)
        el.set("alt", (original.get("alt") or "")[:300])
        for name in ("width", "height"):
            if (original.get(name) or "").isdigit():
                el.set(name, original[name])
    elif tag in ("td", "th"):
        for name in ("colspan", "rowspan"):
            if (original.get(name) or "").isdigit():
                el.set(name, original[name])


def _drop(el):
    # [@ANCHOR: user_websites:fn_hmtl_drop]
    parent = el.getparent()
    if parent is None:
        return
    tail = el.tail or ""
    prev = el.getprevious()
    if prev is not None:
        prev.tail = (prev.tail or "") + tail
    else:
        parent.text = (parent.text or "") + tail
    parent.remove(el)


def _unwrap(el):
    # [@ANCHOR: user_websites:fn_hmtl_unwrap]
    parent = el.getparent()
    if parent is None:
        return
    index = parent.index(el)
    text = el.text or ""
    tail = el.tail or ""
    children = list(el)
    prev = el.getprevious()
    if text:
        if prev is not None:
            prev.tail = (prev.tail or "") + text
        else:
            parent.text = (parent.text or "") + text
    for offset, node in enumerate(children):
        parent.insert(index + offset, node)
    parent.remove(el)
    last = children[-1] if children else prev
    if last is not None:
        last.tail = (last.tail or "") + tail
    else:
        parent.text = (parent.text or "") + tail


# Second path segments a member page can never use: they are routes of the site itself.
# [@ANCHOR: user_websites:member_page_reserved_subpaths]
# Verified by [@ANCHOR: test_member_page_slug_reserved]
RESERVED_PAGE_SLUGS = frozenset(
    {
        "home", "blog", "manage", "create-site", "create_site", "create_blog", "create_blog_post",
        "subscribe", "logbook", "qso", "page", "post", "edit", "new", "admin", "my", "feed",
        "rss", "sitemap", "privacy", "terms", "report", "image", "images", "files",
    }
)
PAGE_SLUG_RE = re.compile(r"^[a-z0-9]([a-z0-9-]{0,58}[a-z0-9])?$")


def menu_pages(env, domain_field, record_id, slug):
    # [@ANCHOR: user_websites:fn_hmtl_menu_pages]
    """Published pages of one site for the site's own menu: [{'name', 'url'}], newest-edited last."""
    svc = env["zero_sudo.security.utils"]._get_service_env(
        "user_websites.user_websites_service_account"
    )
    pages = svc["website.page"].search(
        [
            (domain_field, "=", record_id),
            ("website_published", "=", True),
            ("url", "=like", f"/{slug}/%"),
            ("url", "not like", f"/{slug}/%/%"),
        ],
        order="name, id",
        limit=8,
    )
    return [
        {"name": page.name, "url": page.url}
        for page in pages
        if page.url.rsplit("/", 1)[-1] not in RESERVED_PAGE_SLUGS
    ]


def _replace_with_sanitized_svg(el):
    """Keep a schematic: the SVG goes through zero_sudo's SVG allow-list (the same html_sanitize()
    call blog.post._sanitize_member_content makes), which keeps shapes and text and removes every
    active part (script, handlers, links, <use>, data: URLs). The editor cannot draw SVG; this
    keeps what members pasted or imported before, as the old raw-markup form did."""
    # [@ANCHOR: user_websites:member_html_svg_allowlist]
    # Verified by [@ANCHOR: test_svg_allowlist_sanitizer_fields]
    tail = el.tail
    source = etree.tostring(el, encoding="unicode", method="html", with_tail=False)
    cleaned = html_sanitize(source)
    parent = el.getparent()
    index = parent.index(el)
    try:
        fragment = lxml_html.fragment_fromstring(cleaned, create_parent="div")
    except (etree.ParserError, ValueError):
        parent.remove(el)
        return
    parent.remove(el)
    nodes = list(fragment)
    for offset, node in enumerate(nodes):
        parent.insert(index + offset, node)
    last = nodes[-1] if nodes else None
    if last is not None:
        last.tail = tail
    elif tail:
        previous = parent[index - 1] if index else None
        if previous is not None:
            previous.tail = (previous.tail or "") + tail
        else:
            parent.text = (parent.text or "") + tail

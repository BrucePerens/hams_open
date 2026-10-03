# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Inline SVG in member-authored pages: `website.page._sanitize_user_arch` and its callers.

Tests [@ANCHOR: user_websites:page_arch_svg_allowlist]
[@ANCHOR: test_user_arch_svg_allowlist] and [@ANCHOR: test_blog_post_orm_content_sanitized]
live here.

`_sanitize_user_arch` is a separate XML sanitizer for pages members author. It never called
`html_sanitize`, so hostile SVG (`<set attributeName="href" to="javascript:...">`,
`<animate values="javascript:...">`, `<style>@import`, `<foreignObject>`, `<use href="data:...">`,
event handlers) came back unchanged: stored XSS. Every `<svg>` block now goes through zero_sudo's
allowlist (`sanitize_xml_svgs`), which accepts both the bare names an HTML parse gives and the
namespaced names the XML parse here gives.
"""
import json
import re
import uuid

from lxml import etree

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.addons.zero_sudo.tests.svg_corpus import (
    SAFE_ELEMENTS,
    SCHEMATIC_SVG,
    XSS_CORPUS,
)

SVG_NS = "http://www.w3.org/2000/svg"
XHTML_NS = "http://www.w3.org/1999/xhtml"
_SAFE_LOWER = {name.lower() for name in SAFE_ELEMENTS}
# Gone from every page, inside or outside an svg (the existing strip list, plus the svg-only
# active elements).
_NEVER = {
    "script", "iframe", "object", "embed", "base", "foreignobject", "use", "animate",
    "animatetransform", "animatemotion", "set", "image", "feimage", "filter", "mask", "switch",
}
_DANGEROUS_URL = re.compile(r"(?:javascript|vbscript|data):", re.IGNORECASE)
_NOISE = re.compile(r"[\x00-\x20\x7f]+")
_DANGEROUS_STYLE = re.compile(r"@import|expression\(|url\((?!#)", re.IGNORECASE)


def _local(tag):
    return tag.rpartition("}")[2].lower()


def assert_arch_is_safe(test, name, arch):
    """Every invariant a sanitized arch must satisfy, whatever went in."""
    if not arch.strip():
        return
    root = etree.fromstring(f"<root>{arch}</root>", etree.XMLParser(recover=True))
    test.assertIsNotNone(root, name)
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        tag = _local(element.tag)
        test.assertNotIn(":", element.tag, f"{name}: undeclared prefix survived: {element.tag}")
        test.assertNotIn(tag, _NEVER, f"{name}: <{tag}> survived: {arch[:300]}")
        in_svg = tag == "svg" or any(_local(a.tag) == "svg" for a in element.iterancestors())
        if in_svg:
            test.assertIn(tag, _SAFE_LOWER, f"{name}: <{tag}> inside svg: {arch[:300]}")
            test.assertTrue(
                not element.tag.startswith("{") or element.tag.startswith(f"{{{SVG_NS}}}"),
                f"{name}: foreign namespace inside svg: {element.tag}",
            )
        for attr, value in element.attrib.items():
            lowered = attr.lower()
            test.assertFalse(
                lowered.startswith("on"), f"{name}: handler {attr} kept on <{tag}>"
            )
            if lowered.startswith("data-blocked-"):
                continue  # the sanitizer's own inert quarantine of a removed attribute
            if in_svg:
                test.assertFalse(
                    lowered.startswith("t-") or lowered in {"href", "xlink:href", "src"},
                    f"{name}: {attr} kept inside svg",
                )
                test.assertIsNone(_DANGEROUS_STYLE.search(value), f"{name}: {attr}={value!r}")
            if lowered in {"href", "src", "action", "formaction", "content", "poster", "background"}:
                test.assertIsNone(
                    _DANGEROUS_URL.match(_NOISE.sub("", value)),
                    f"{name}: dangerous URL in {attr}={value!r}",
                )
            test.assertNotIn("evil.example", value if in_svg else "", name)


@tagged("post_install", "-at_install")
class TestUserArchSvgSanitizer(HamsHttpCase):
    """`_sanitize_user_arch` called directly, as the page `create`/`write` do for a member."""

    # Tests [@ANCHOR: user_websites:page_arch_svg_allowlist]
    # Tests [@ANCHOR: test_user_arch_svg_allowlist]

    def sanitize(self, arch):
        return self.env["website.page"]._sanitize_user_arch(arch)

    def test_01_every_corpus_vector_is_neutralised(self):
        for name, payload in XSS_CORPUS:
            with self.subTest(vector=name):
                cleaned, _modified = self.sanitize(f"<p>x</p>{payload}")
                assert_arch_is_safe(self, name, cleaned)
                again, _ = self.sanitize(cleaned)
                self.assertEqual(again, cleaned, f"{name}: second pass changed the arch")

    def test_02_the_reported_vectors_in_both_namespace_spellings(self):
        body = (
            '<set attributeName="href" to="javascript:window.__xss=1"/>'
            '<animate attributeName="href" values="javascript:window.__xss=1"/>'
            "<style>@import url(https://evil.example/x.css);</style>"
            '<foreignObject><script>window.__xss=1</script><iframe src="x"/></foreignObject>'
            '<use href="data:image/svg+xml,x#a"/><rect width="5" height="5" onclick="window.__xss=1"/>'
        )
        for label, svg in (
            ("bare", f"<svg viewBox='0 0 9 9'>{body}</svg>"),
            ("xmlns", f"<svg xmlns='{SVG_NS}' viewBox='0 0 9 9'>{body}</svg>"),
            ("prefixed", f"<s:svg xmlns:s='{SVG_NS}'><s:set attributeName='href' to='javascript:1'/><s:rect/></s:svg>"),
        ):
            with self.subTest(form=label):
                cleaned, modified = self.sanitize(f"<div>{svg}</div>")
                self.assertTrue(modified, "a hostile svg must count as an injection attempt")
                assert_arch_is_safe(self, label, cleaned)
                for word in ("javascript", "evil.example", "foreignObject", "<set", "<animate", "<style"):
                    self.assertNotIn(word, cleaned, label)

    def test_03_safe_schematic_survives_with_its_meaning_and_is_not_flagged(self):
        for label, source in (
            ("xmlns", SCHEMATIC_SVG),
            ("bare", SCHEMATIC_SVG.replace(f' xmlns="{SVG_NS}"', "")),
        ):
            with self.subTest(form=label):
                cleaned, modified = self.sanitize(f"<h2>Relay</h2>{source}<p>after</p>")
                self.assertFalse(modified, "a harmless schematic must not strike its author")
                self.assertIn("<h2>Relay</h2>", cleaned)
                self.assertIn("<p>after</p>", cleaned)
                before = etree.fromstring(
                    SCHEMATIC_SVG.replace(f' xmlns="{SVG_NS}"', "")
                )
                after = etree.fromstring(
                    re.search(r"<svg.*</svg>", cleaned, re.S).group(0).replace(f' xmlns="{SVG_NS}"', "")
                )

                def shape(tree):
                    return [
                        (
                            _local(e.tag),
                            sorted(k for k in e.attrib if k != "style"),
                            [e.attrib.get(k) for k in ("d", "points", "viewBox", "id", "x1", "cx", "fill")],
                            (e.text or "").strip(),
                        )
                        for e in tree.iter()
                        if isinstance(e.tag, str)
                    ]

                self.assertEqual(shape(after), shape(before))
                self.assertIn("margin-bottom", cleaned)
                self.assertEqual(self.sanitize(cleaned), (cleaned, False))

    def test_04_namespace_and_prefix_games_cannot_bypass_it(self):
        xss = "window.__xss=1"
        vectors = {
            "undeclared_prefix_script": f"<svg:script>{xss}</svg:script>",
            "undeclared_prefix_use": "<svg:use href='data:image/svg+xml,x'/>",
            "undeclared_prefix_svg": f"<svg:svg onload='{xss}'><svg:rect/></svg:svg>",
            "declared_svg_prefix": f"<s:svg xmlns:s='{SVG_NS}' s:onload='{xss}'><s:script>{xss}</s:script><s:rect/></s:svg>",
            "svg_prefix_bound_to_xhtml": f"<x:svg xmlns:x='{XHTML_NS}'><x:script>{xss}</x:script></x:svg>",
            "default_namespace_switch": f"<svg xmlns='{SVG_NS}'><a xmlns='{XHTML_NS}'><script>{xss}</script></a><rect/></svg>",
            "switch_to_xhtml_svg": f"<svg xmlns='{XHTML_NS}'><script>{xss}</script></svg>",
            "nested_xmlns_redeclaration": f"<svg xmlns='{SVG_NS}'><g xmlns='{XHTML_NS}'><iframe src='javascript:{xss}'/></g></svg>",
            "nested_svg_other_namespace": f"<svg xmlns='{SVG_NS}'><svg xmlns='{XHTML_NS}' onload='{xss}'><rect/></svg></svg>",
            "nested_prefix_redeclared": f"<svg xmlns:a='{SVG_NS}'><a:g xmlns:a='{XHTML_NS}'><a:script>{xss}</a:script></a:g></svg>",
            "xmlns_javascript": f"<svg xmlns='javascript:{xss}'><rect/></svg>",
            "uppercase_svg": f"<SVG xmlns='{SVG_NS}' ONLOAD='{xss}'><RECT/></SVG>",
            "xlink_prefixed_href": f"<svg xmlns:xlink='http://www.w3.org/1999/xlink'><a xlink:href='javascript:{xss}'><text>x</text></a></svg>",
            "svg_inside_foreign_switch": f"<div xmlns='{XHTML_NS}'><svg><rect onclick='{xss}'/></svg></div>",
        }
        for name, payload in vectors.items():
            with self.subTest(vector=name):
                cleaned, _ = self.sanitize(f"<p>a</p>{payload}<p>b</p>")
                assert_arch_is_safe(self, name, cleaned)
                self.assertIn("<p>a</p>", cleaned)
                self.assertIn("<p>b</p>", cleaned)
                self.assertNotIn(xss, cleaned)

    def test_05_qweb_directives_are_dropped_inside_svg(self):
        cleaned, modified = self.sanitize(
            "<svg t-if='True' t-att-fill='1'><t t-esc='1'/><t t-call='website.layout'/>"
            "<rect t-foreach='range(3)' t-as='i' t-att-x='i' t-attf-y='{{i}}' width='3' height='3'/>"
            "<text t-out='1'>keep</text></svg>"
        )
        self.assertNotIn("t-", cleaned)
        self.assertNotIn("<t ", cleaned)
        self.assertIn("keep", cleaned)
        self.assertIn("<rect", cleaned)

    def test_06_scheme_with_embedded_whitespace_is_blocked_outside_svg(self):
        for href in ("java\tscript:window.__xss=1", "java\nscript:1", "  javascript:1", "java&#9;script:1"):
            with self.subTest(href=href):
                cleaned, modified = self.sanitize(f'<a href="{href}">x</a>')
                self.assertTrue(modified)
                self.assertNotIn(' href="', cleaned)

    def test_07_text_after_a_removed_element_is_kept(self):
        cleaned, _ = self.sanitize("<p>one<script>1</script> two <svg onload='1'><script>x</script></svg> three</p>")
        self.assertIn("one two", cleaned)
        self.assertIn("three", cleaned)

    def test_09_review_findings_in_the_member_page_path(self):
        """The adversarial review's vectors (huge repeated blocks, style overlay)."""
        import time

        started = time.monotonic()
        cleaned, modified = self.sanitize("<div>" + "<svg><rect/></svg>" * 20000 + "</div>")
        self.assertLess(time.monotonic() - started, 5.0, "many svg blocks pinned the worker")
        self.assertTrue(modified)
        self.assertLessEqual(cleaned.count("<svg"), 1000)
        started = time.monotonic()
        self.sanitize("<p>" + "<svg></svg><b>x</b>" * 20000 + "</p>")
        self.assertLess(time.monotonic() - started, 5.0)
        cleaned, _ = self.sanitize(
            '<svg style="margin-left:-5000px;margin-top:-5000px;width:20000px;height:20000px">'
            '<rect width="100%" height="100%" fill="transparent"/></svg>'
        )
        style = (re.search(r'style="([^"]*)"', cleaned) or [None, ""])[1]
        self.assertNotRegex(style, r":\s*-", f"negative length survived: {style!r}")
        self.assertNotRegex(style, r"(?:width|height):\s*\d{4,}", f"huge size survived: {style!r}")

    def test_08_oversize_or_foreign_block_is_dropped_not_kept(self):
        huge = "<svg>" + "<g>" * 40 + "<rect/>" + "</g>" * 40 + "</svg>"
        cleaned, modified = self.sanitize(f"<p>ok</p>{huge}")
        self.assertNotIn("<svg", cleaned)
        self.assertIn("<p>ok</p>", cleaned)


@tagged("post_install", "-at_install")
class TestUserArchSvgMemberPaths(HamsHttpCase):
    """The real write paths a member reaches, as a member."""

    # Tests [@ANCHOR: test_user_arch_svg_allowlist]
    # Tests [@ANCHOR: test_blog_post_orm_content_sanitized]
    # Tests [@ANCHOR: user_websites:blog_post_content_sanitize]

    def setUp(self):
        super().setUp()
        unique = uuid.uuid4().hex[:8]
        self.member = self.env["res.users"].create(
            {
                "name": f"Svg Member {unique}",
                "login": f"svgmember_{unique}",
                "password": "svgmember",
                "email": f"svgmember_{unique}@example.com",
                "website_slug": f"svgmember-{unique}",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

    def _create_page(self, slug, arch, as_member=True):
        pages = self.env["website.page"]
        if as_member:
            pages = pages.with_user(self.member)
        return pages.create(
            {
                "url": f"/{self.member.website_slug}/{slug}",
                "name": slug,
                "type": "qweb",
                "owner_user_id": self.member.id,
                "website_published": True,
                "arch": arch,
            }
        )

    def test_01_create_and_write_sanitize_svg_and_strike_only_the_hostile(self):
        reports = self.env["content.violation.report"]
        before = reports.search_count([("content_owner_id", "=", self.member.id)])
        page = self._create_page("good", f"<t name='Good'><div>{SCHEMATIC_SVG}</div></t>")
        self.assertIn("viewBox", page.arch)
        self.assertIn("<linearGradient", page.arch)
        self.assertEqual(
            reports.search_count([("content_owner_id", "=", self.member.id)]),
            before,
            "a harmless schematic was reported as an injection",
        )
        hostile = (
            "<svg viewBox='0 0 9 9'><set attributeName='href' to='javascript:1'/>"
            "<animate values='javascript:1'/><style>@import url(https://evil.example/a.css)</style>"
            "<foreignObject><script>1</script></foreignObject><rect id='kept' width='3' height='3'/></svg>"
        )
        page2 = self._create_page("bad", f"<t name='Bad'><div>{hostile}</div></t>")
        assert_arch_is_safe(self, "create", page2.arch)
        self.assertIn("kept", page2.arch)
        self.assertGreater(
            reports.search_count([("content_owner_id", "=", self.member.id)]), before
        )
        page.with_user(self.member).write({"arch": f"<t name='Good'><div>{hostile}</div></t>"})
        assert_arch_is_safe(self, "write", page.arch)

    def test_02_blog_post_content_is_sanitized_on_the_orm_path(self):
        """blog.post.content is sanitize=False; create/write used to store it verbatim."""
        blog = self.env["blog.blog"].create(
            {"name": f"Svg blog {uuid.uuid4().hex[:6]}", "owner_user_id": self.member.id}
        )
        hostile = (
            "<p>hi</p><script>window.__xss=1</script><img src=x onerror=window.__xss=1>"
            "<svg viewBox='0 0 9 9'><set attributeName='href' to='javascript:1'/>"
            "<rect id='box' width='3' height='3'/></svg>"
        )
        post = self.env["blog.post"].with_user(self.member).create(
            {
                "name": "svg post",
                "blog_id": blog.id,
                "owner_user_id": self.member.id,
                "content": hostile,
            }
        )
        for stage in ("create", "write"):
            if stage == "write":
                post.with_user(self.member).write({"content": hostile})
            self.assertIn("hi", post.content)
            self.assertIn('id="box"', post.content)
            for word in ("<script", "onerror", "<set", "javascript"):
                self.assertNotIn(word, post.content, f"{stage}: {word}")

    def test_03_hostile_member_page_loaded_publicly_runs_nothing(self):
        """Save hostile pages as a member, load them as an anonymous visitor in headless Chrome."""
        def well_formed(fragment):
            try:
                etree.fromstring(f"<root>{fragment}</root>")
                return True
            except etree.XMLSyntaxError:
                return False

        # A member's save of markup that is not well-formed XML is repaired by the sanitizer's
        # recovering parser and can close the page's own wrapper element, which the view
        # validation then rejects; only well-formed vectors share the member's own page.
        combinable = [(n, p) for n, p in XSS_CORPUS if n in _COMBINABLE and well_formed(p)]
        self.assertGreater(len(combinable), 5)
        combined_names = {n for n, _ in combinable}
        member_page = self._create_page(
            "attack",
            "<t name='Attack'><div id='corpus-start'>x</div>"
            + SCHEMATIC_SVG
            + "".join(f"<p>{n}</p>{p}" for n, p in combinable)
            + "</t>",
        )
        assert_arch_is_safe(self, "member page", member_page.arch)
        extra = []
        for index, (name, payload) in enumerate(XSS_CORPUS):
            if name in combined_names:
                continue
            # The sanitizer's own output for this vector is exactly what a member's save would
            # store; one page per vector because some (plaintext, xmp, unclosed svg) change how
            # the browser reads everything after them. Created without the member sanitizer so
            # 100+ strikes do not suspend the member and 404 the pages.
            cleaned, _ = self.env["website.page"]._sanitize_user_arch(
                f"<div id='corpus-start'>{name}</div>{payload}"
            )
            page = self._create_page(f"v{index}", f"<t name='{name}'>{cleaned}</t>", as_member=False)
            extra.append(page.url)
        self.assertTrue(extra)
        self.browser_js(
            member_page.url,
            _XSS_JS.replace("__EXTRA_URLS__", json.dumps(extra)),
            ready="",
            timeout=180,
        )


# Vectors that read as ordinary markup for the browser and can share one page.
_COMBINABLE = {
    "script_element", "onload_on_svg", "onclick_on_rect", "onerror_on_image", "set_href_javascript",
    "animate_href_javascript", "foreignobject_script", "style_element_import", "use_data_uri",
    "namespace_prefix_script", "namespace_prefix_use", "namespace_xmlns_override",
    "a_href_javascript", "style_attr_url_external", "filter_feimage",
}

_XSS_JS = r"""
(async () => {
    if (document.readyState !== "complete") {
        await new Promise((resolve) => window.addEventListener("load", resolve, { once: true }));
    }
    const fail = (message) => { throw new Error(message); };
    // Positive control: the detector sees a raw payload run in this harness.
    const control = document.createElement("div");
    control.innerHTML = '<img src="x" onerror="window.__xss_control=1">';
    document.body.appendChild(control);
    for (let i = 0; i < 50 && window.__xss_control === undefined; i++) {
        await new Promise((resolve) => setTimeout(resolve, 100));
    }
    if (window.__xss_control !== 1) { fail("control payload did not run: the XSS detector is blind"); }
    control.remove();

    const allowed = new Set("svg g path rect circle ellipse line polyline polygon text tspan defs title desc linearGradient radialGradient stop clipPath marker symbol pattern".split(" "));
    const never = new Set("script iframe object embed link meta base foreignobject use image animate set a filter mask switch".split(" "));
    let svgBlocks = 0;
    function inspect(container, label) {
        for (const el of container.querySelectorAll("*")) {
            const name = el.localName.toLowerCase();
            for (const attr of Array.from(el.attributes)) {
                if (attr.name.toLowerCase().startsWith("on")) { fail(label + ": handler " + attr.name + " on <" + name + ">"); }
            }
            if (["script", "iframe", "object", "embed", "base"].includes(name)) { fail(label + ": forbidden <" + name + ">"); }
            if (el.localName === "svg") { svgBlocks++; }
            if (!el.closest("svg")) { continue; }
            if (never.has(name)) { fail(label + ": forbidden <" + name + "> inside svg"); }
            if (!allowed.has(el.localName)) { fail(label + ": <" + el.localName + "> outside the allowlist inside svg"); }
            if (el.namespaceURI !== "http://www.w3.org/2000/svg") { fail(label + ": svg child outside the SVG namespace: " + el.localName); }
            for (const attr of Array.from(el.attributes)) {
                if (/^(xlink:)?href$/i.test(attr.name)) { fail(label + ": href kept on <" + name + ">"); }
                if (attr.name.toLowerCase().startsWith("t-")) { fail(label + ": QWeb attribute " + attr.name + " inside svg"); }
                if (/javascript:|data:|vbscript:|@import|url\((?!#)/i.test(attr.value)) { fail(label + ": dangerous value " + attr.name + "=" + attr.value); }
            }
        }
    }
    for (let i = 0; i < 100 && !document.getElementById("corpus-start"); i++) {
        await new Promise((resolve) => setTimeout(resolve, 200));
    }
    const start = document.getElementById("corpus-start");
    if (!start) { fail("hostile member page is missing: " + location.href + " " + document.body.innerText.slice(0, 200)); }
    inspect(start.parentElement, "member page");
    if (!document.querySelector('svg[aria-label^="Relay"] rect')) { fail("the safe schematic did not survive"); }

    // Every other vector's page: fetch as the anonymous visitor, inspect what the browser
    // parsed, then put the markup into the live document where a handler would fire at once.
    const live = document.createElement("div");
    document.body.appendChild(live);
    for (const url of __EXTRA_URLS__) {
        const response = await fetch(url, { credentials: "same-origin" });
        if (response.status !== 200) { fail(url + " answered " + response.status); }
        const parsed = new DOMParser().parseFromString(await response.text(), "text/html");
        const marker = parsed.getElementById("corpus-start");
        if (!marker) { fail("page missing its marker: " + url); }
        inspect(marker.parentElement, url);
        live.innerHTML = marker.parentElement.innerHTML;
        await new Promise((resolve) => setTimeout(resolve, 150));
    }
    live.remove();
    for (const type of ["mouseover", "click", "focus"]) {
        for (const el of start.parentElement.querySelectorAll("svg, svg *")) { el.dispatchEvent(new Event(type, { bubbles: true })); }
    }
    await new Promise((resolve) => setTimeout(resolve, 1500));
    if (window.__xss !== undefined) { fail("an injected script ran: window.__xss=" + window.__xss); }
    const requested = performance.getEntriesByType("resource").map((e) => e.name).filter((n) => n.includes("evil.example"));
    if (requested.length) { fail("a request was made to an injected URL: " + requested.join(",")); }
    if (svgBlocks < 1) { fail("no svg block at all was left to check"); }
    console.log("USER_ARCH_SVG_REPORT " + JSON.stringify({ svgBlocks, pages: __EXTRA_URLS__.length }));
})().then(
    () => console.log("test successful"),
    (error) => console.error("User page SVG XSS check failed: " + error.message),
);
"""

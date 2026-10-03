# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
[@ANCHOR: zero_sudo:svg_allowlist_sanitizer]
Verified by [@ANCHOR: test_svg_allowlist_sanitizer_unit]
and [@ANCHOR: test_svg_allowlist_sanitizer_fields]

Odoo's HTML sanitizer (`odoo.tools.mail.html_sanitize`, run by every
`fields.Html` write) strips every SVG shape element, because
`SANITIZE_TAGS['allow_tags']` is the HTML tag list. A schematic written as
inline SVG therefore arrives as an empty box. Turning the sanitizer off, or
adding `svg`/`path`/... to the global tag list, would open the whole SVG
attack surface (script, foreignObject, use, animate/set href, style with
url(), feImage, event handlers). Instead this module makes the sanitizer
SVG-aware with a strict allowlist and leaves everything outside `<svg>`
exactly as Odoo handled it before.

Where it hooks: `odoo.tools.mail._Cleaner.__call__`, the one place every
`html_sanitize()` run (whatever module imported the function by name, and
whatever `output_method`/`sanitize_*` options it passed) filters the parsed
document. Patching `html_sanitize` itself would miss the dozen modules that
bound it with `from odoo.tools import html_sanitize` before this module was
imported; the class is looked up through the module at call time, so one
patch covers them all. See `install()`.

How it works, per `_Cleaner` run on an already-parsed lxml tree:

1. Every `<svg>` element (outermost first) is validated and rebuilt as a
   brand-new subtree containing only allowlisted elements, attributes and
   values, then replaced in the tree by a random-nonce text token.
2. The unmodified Odoo cleaner runs on the rest of the document.
3. Each token that survived the cleaner in ordinary text content (not in an
   attribute, comment, textarea, title, style, math... ) is replaced by its
   rebuilt SVG subtree. A token that did not survive means the block is
   dropped.

Because the SVG is rebuilt rather than filtered in place, nothing the author
wrote reaches the output except values that matched an allowlist, and the
camel-case SVG names (`viewBox`, `linearGradient`, `clipPath`...) that
libxml2's HTML parser lowercased are restored from the allowlist's canonical
spelling. The same parser Odoo's sanitizer already uses reads the SVG, so
there is no second parser whose view of the markup could differ, and no XML
entity/DTD processing exists to abuse (libxml2's HTML parser does not
declare or expand entities; `&Omega;` style named character references are
decoded to plain text before this module sees them).

The design, threat model, residual risks and tested vectors are written up
in `zero_sudo/docs/stories/svg_allowlist_sanitizer.md`.
"""
import logging
import re
import secrets

from lxml import etree

_logger = logging.getLogger(__name__)

SVG_NS = "http://www.w3.org/2000/svg"
XML_NS = "http://www.w3.org/XML/1998/namespace"

# One SVG block: at most this much text+attribute data, this many elements, this
# deep. Blocks over a cap are dropped whole, not truncated.
MAX_SVG_BYTES = 256 * 1024
MAX_SVG_ELEMENTS = 5000
MAX_SVG_DEPTH = 32
MAX_ATTR_VALUE = 20000

ALLOWED_ELEMENTS = {
    name.lower(): name
    for name in (
        "svg g path rect circle ellipse line polyline polygon text tspan "
        "defs title desc linearGradient radialGradient stop clipPath "
        "marker symbol pattern"
    ).split()
}
# Elements whose character data is rendered; everywhere else only whitespace
# text is kept.
TEXT_ELEMENTS = {"text", "tspan", "title", "desc"}

_NUM = r"[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?"
_LEN = rf"{_NUM}(?:px|em|rem|ex|pt|pc|cm|mm|in|%)?"
_ID = r"[A-Za-z][A-Za-z0-9_\-.:]{0,127}"
_COLOR = (
    r"(?:none|currentColor|inherit|transparent|[A-Za-z]{3,25}"
    r"|\#[0-9A-Fa-f]{3,8}"
    rf"|(?:rgb|rgba|hsl|hsla)\(\s*{_NUM}%?(?:\s*[, ]\s*{_NUM}%?){{2}}"
    rf"(?:\s*[,/]\s*{_NUM}%?)?\s*\))"
)
_URL_REF = rf"url\(\s*\#({_ID})\s*\)"
_PAINT = rf"(?:{_COLOR}|{_URL_REF}(?:\s+{_COLOR})?)"

_ENUMS = {
    "fill-rule": {"nonzero", "evenodd"},
    "clip-rule": {"nonzero", "evenodd"},
    "stroke-linecap": {"butt", "round", "square"},
    "stroke-linejoin": {"miter", "round", "bevel", "arcs", "miter-clip"},
    "text-anchor": {"start", "middle", "end"},
    "dominant-baseline": {
        "auto", "text-bottom", "alphabetic", "ideographic", "middle",
        "central", "mathematical", "hanging", "text-top",
    },
    "font-style": {"normal", "italic", "oblique"},
    "gradientUnits": {"userSpaceOnUse", "objectBoundingBox"},
    "clipPathUnits": {"userSpaceOnUse", "objectBoundingBox"},
    "patternUnits": {"userSpaceOnUse", "objectBoundingBox"},
    "patternContentUnits": {"userSpaceOnUse", "objectBoundingBox"},
    "markerUnits": {"userSpaceOnUse", "strokeWidth"},
    "spreadMethod": {"pad", "reflect", "repeat"},
    "vector-effect": {"none", "non-scaling-stroke"},
    "xml:space": {"default", "preserve"},
}

_RE_LEN = re.compile(_LEN)
_RE_LEN_AUTO = re.compile(rf"(?:auto|{_LEN})")
_RE_MARGIN = re.compile(rf"(?:auto|{_LEN})(?:\s+(?:auto|{_LEN})){{0,3}}")
_RE_DISPLAY = re.compile(r"(?:block|inline|inline-block|none)")
_RE_NUMBER = re.compile(_NUM)
_RE_OPACITY = re.compile(rf"{_NUM}%?")
_RE_NUMLIST = re.compile(r"[0-9eE+\-.,\s%]*")
_RE_LENLIST = re.compile(r"[0-9eE+\-.,\s%a-z]*")
_RE_PATH = re.compile(r"[MmLlHhVvCcSsQqTtAaZz0-9eE+\-.,\s]*")
# One leading \s*, and separators consumed in exactly one place, so a long run of
# spaces cannot be split between iterations (catastrophic backtracking).
_RE_TRANSFORM = re.compile(
    r"\s*(?:(?:translate|rotate|scale|skewX|skewY|matrix)"
    r"\s*\([0-9eE+\-.,\s]*\)[\s,]*)*"
)
_RE_PAINT = re.compile(_PAINT)
_RE_URL_ONLY = re.compile(rf"\s*(?:none|{_URL_REF})\s*")
_RE_ID = re.compile(_ID)
_RE_CLASS = re.compile(r"[A-Za-z0-9_\- ]{0,200}")
_RE_ROLE = re.compile(r"[a-z]{2,30}")
_RE_ARIA_NAME = re.compile(r"aria-[a-z]{3,30}")
_RE_ARIA_VALUE = re.compile(r"[^\x00-\x1f<>\"'&\\`]{0,200}")
_RE_FONT_FAMILY = re.compile(r"[A-Za-z0-9 ,\-_']{1,200}")
_RE_FONT_WEIGHT = re.compile(r"(?:normal|bold|bolder|lighter|[1-9]00)")
_RE_ORIENT = re.compile(rf"(?:auto|auto-start-reverse|{_NUM}(?:deg|rad|grad|turn)?)")
_RE_ASPECT = re.compile(
    r"(?:none|x(?:Min|Mid|Max)Y(?:Min|Mid|Max))(?:\s+(?:meet|slice))?"
)
_RE_OFFSET = re.compile(rf"{_NUM}%?")
_RE_STYLE_PROP = re.compile(r"[a-z\-]{3,40}")
# Defence in depth on every value after it passed its own allowlist.
_RE_BAD_VALUE = re.compile(
    r"javascript|vbscript|data\s*:|expression|@import|&#|\\|<|>|"
    r"[\x00-\x08\x0b\x0c\x0e-\x1f]",
    re.IGNORECASE,
)
_RE_CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f￾￿]")

# Contexts in which a text token must never be expanded into markup: the
# browser reads their content as raw text or as another language.
_BLOCKED_CONTEXT = frozenset(
    "textarea title style script xmp plaintext noscript noembed noframes "
    "iframe template select option math svg head object embed applet "
    "frameset frame".split()
)


def _check_enum(name):
    allowed = _ENUMS[name]

    def check(value):
        return value if value in allowed else None

    return check


# A single number, colour, id or keyword is never long. The cap keeps the quadratic
# worst case of patterns like `\d+\.?\d*` (a 20000-digit value ending in a letter)
# off the table; lists, paths and transforms use linear patterns and a larger cap.
SCALAR_LIMIT = 120


def _check_regex(regex, limit=SCALAR_LIMIT):
    def check(value):
        stripped = value.strip()
        if len(stripped) > limit or not regex.fullmatch(stripped):
            return None
        return stripped

    return check


def _paint(value):
    stripped = value.strip()
    if len(stripped) > 2 * SCALAR_LIMIT or not _RE_PAINT.fullmatch(stripped):
        return None
    ref = re.search(_URL_REF, stripped)
    if ref:
        # Only a same-document fragment survives, rewritten in canonical form.
        fallback = stripped[ref.end():].strip()
        return f"url(#{ref.group(1)})" + (f" {fallback}" if fallback else "")
    return stripped


def _url_ref(value):
    if len(value) > 2 * SCALAR_LIMIT:
        return None
    match = _RE_URL_ONLY.fullmatch(value)
    if not match:
        return None
    ref = re.search(_URL_REF, value)
    return f"url(#{ref.group(1)})" if ref else "none"


def _viewbox(value):
    if len(value) > 2 * SCALAR_LIMIT:
        return None
    parts = re.split(r"[\s,]+", value.strip())
    if len(parts) != 4:
        return None
    if not all(_RE_NUMBER.fullmatch(part) for part in parts):
        return None
    return " ".join(parts)


def _aria(value):
    return value if _RE_ARIA_VALUE.fullmatch(value) else None


_PAINT_ATTRS = ("fill", "stroke", "stop-color")
_NUM_ATTRS = (
    "fill-opacity", "stroke-opacity", "opacity", "stop-opacity",
)
_LEN_ATTRS = (
    "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r",
    "rx", "ry", "fx", "fy", "fr", "stroke-width", "font-size",
    "markerWidth", "markerHeight", "refX", "refY", "stroke-miterlimit",
    "stroke-dashoffset",
)
_LIST_ATTRS = ("points", "dx", "dy", "rotate")

# canonical attribute name -> validator(value) -> cleaned value or None
_VALIDATORS = {
    "viewBox": _viewbox,
    "preserveAspectRatio": _check_regex(_RE_ASPECT),
    "d": _check_regex(_RE_PATH, MAX_ATTR_VALUE),
    "transform": _check_regex(_RE_TRANSFORM, 2000),
    "gradientTransform": _check_regex(_RE_TRANSFORM, 2000),
    "patternTransform": _check_regex(_RE_TRANSFORM, 2000),
    "stroke-dasharray": _check_regex(_RE_LENLIST, 500),
    "font-family": _check_regex(_RE_FONT_FAMILY, 200),
    "font-weight": _check_regex(_RE_FONT_WEIGHT),
    "offset": _check_regex(_RE_OFFSET),
    "orient": _check_regex(_RE_ORIENT),
    "id": _check_regex(_RE_ID),
    "class": _check_regex(_RE_CLASS, 200),
    "role": _check_regex(_RE_ROLE),
    "clip-path": _url_ref,
    "marker-start": _url_ref,
    "marker-mid": _url_ref,
    "marker-end": _url_ref,
    "xmlns": lambda value: SVG_NS if value.strip() == SVG_NS else None,
}
for _name in _PAINT_ATTRS + ("color",):
    _VALIDATORS[_name] = _paint
for _name in _NUM_ATTRS:
    _VALIDATORS[_name] = _check_regex(_RE_OPACITY)
for _name in _LEN_ATTRS:
    _VALIDATORS[_name] = _check_regex(_RE_LEN)
for _name in ("width", "height"):
    _VALIDATORS[_name] = _check_regex(_RE_LEN_AUTO)
for _name in _LIST_ATTRS:
    _VALIDATORS[_name] = _check_regex(_RE_NUMLIST, MAX_ATTR_VALUE)
for _name in _ENUMS:
    _VALIDATORS[_name] = _check_enum(_name)
del _name

ALLOWED_ATTRIBUTES = {name.lower(): name for name in _VALIDATORS}

# `style` is accepted only as a list of declarations whose property is one of
# these presentation properties and whose value passes that property's own
# validator; it is re-serialised from the parsed declarations.
_STYLE_PROPS = {
    "fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin",
    "stroke-dasharray", "stroke-dashoffset", "stroke-opacity",
    "stroke-miterlimit", "fill-opacity", "fill-rule", "opacity",
    "font-family", "font-size", "font-weight", "font-style", "text-anchor",
    "dominant-baseline", "stop-color", "stop-opacity", "clip-path",
    "marker-start", "marker-mid", "marker-end", "vector-effect",
    # Layout niceties real schematics carry; none can position content outside its box.
    "color", "background-color", "margin", "margin-top", "margin-bottom",
    "margin-left", "margin-right", "max-width", "width", "height", "display",
}
_STYLE_ONLY_VALIDATORS = {
    "background-color": _paint,
    "margin": _check_regex(_RE_MARGIN),
    "margin-top": _check_regex(_RE_LEN_AUTO),
    "margin-bottom": _check_regex(_RE_LEN_AUTO),
    "margin-left": _check_regex(_RE_LEN_AUTO),
    "margin-right": _check_regex(_RE_LEN_AUTO),
    "max-width": _check_regex(_RE_LEN_AUTO),
    "display": _check_regex(_RE_DISPLAY),
}


class _Rejected(Exception):
    """Raised inside one block when it exceeds a cap; the block is dropped."""


class _Budget:
    def __init__(self):
        self.elements = 0
        self.size = 0

    def element(self):
        self.elements += 1
        if self.elements > MAX_SVG_ELEMENTS:
            raise _Rejected("too many elements")

    def spend(self, amount):
        self.size += amount
        if self.size > MAX_SVG_BYTES:
            raise _Rejected("block too large")


def _clean_style(value):
    if len(value) > 2000 or "/*" in value or "@" in value or "\\" in value:
        return None
    declarations = []
    for declaration in value.split(";"):
        if ":" not in declaration:
            continue
        prop, _sep, raw = declaration.partition(":")
        prop = prop.strip().lower()
        if prop not in _STYLE_PROPS or not _RE_STYLE_PROP.fullmatch(prop):
            continue
        validator = _STYLE_ONLY_VALIDATORS.get(prop) or _VALIDATORS.get(prop)
        if validator is None:
            continue
        cleaned = validator(raw.strip())
        if cleaned is None or _RE_BAD_VALUE.search(cleaned):
            continue
        declarations.append(f"{prop}:{cleaned}")
    return ";".join(declarations) or None


def _clean_attribute(name, value):
    """Return (canonical name, cleaned value) or None when not allowed."""
    if not isinstance(name, str) or not isinstance(value, str):
        return None
    lowered = name.lower()
    if lowered.startswith("aria-"):
        if _RE_ARIA_NAME.fullmatch(lowered) and _aria(value) is not None:
            return lowered, value
        return None
    if lowered == "style":
        cleaned = _clean_style(value)
        return ("style", cleaned) if cleaned else None
    canonical = ALLOWED_ATTRIBUTES.get(lowered)
    if canonical is None or len(value) > MAX_ATTR_VALUE:
        return None
    cleaned = _VALIDATORS[canonical](value)
    if cleaned is None or _RE_BAD_VALUE.search(cleaned):
        return None
    return canonical, cleaned


def _build(source, depth, budget, dropped_attributes=()):
    """Rebuild one source element as a clean element, or None to drop it."""
    if depth > MAX_SVG_DEPTH:
        raise _Rejected("nested too deep")
    tag = source.tag
    if not isinstance(tag, str):
        return None
    canonical = ALLOWED_ELEMENTS.get(tag.lower())
    if canonical is None:
        return None
    budget.element()
    budget.spend(len(canonical))
    clean = etree.Element(canonical)
    for name, value in source.attrib.items():
        result = _clean_attribute(name, value)
        if result is None or result[0] in dropped_attributes:
            continue
        budget.spend(len(result[0]) + len(result[1]))
        attr_name, attr_value = result
        if attr_name == "xml:space":
            attr_name = f"{{{XML_NS}}}space"
        clean.set(attr_name, attr_value)
    keeps_text = canonical in TEXT_ELEMENTS
    clean.text = _clean_text(source.text, keeps_text, budget)
    previous = None
    for child in source:
        built = _build(child, depth + 1, budget, dropped_attributes)
        if built is None:
            # A dropped child's own tail text still belongs to this element.
            tail = _clean_text(child.tail, keeps_text, budget)
            if tail and previous is None:
                clean.text = _join_text(clean.text, tail, keeps_text)
            elif tail:
                previous.tail = _join_text(previous.tail, tail, keeps_text)
            continue
        built.tail = _clean_text(child.tail, keeps_text, budget)
        clean.append(built)
        previous = built
    return clean


def _join_text(first, second, keeps_text):
    joined = (first or "") + second
    # Layout whitespace is normalised so a second pass changes nothing.
    if not keeps_text and not joined.strip():
        return "\n"
    return joined


def _clean_text(text, keeps_text, budget):
    if not text:
        return None
    if keeps_text:
        text = _RE_CONTROL.sub("", text)
        budget.spend(len(text))
        return text or None
    return "\n" if not text.strip() else None


def sanitize_svg_element(source, dropped_attributes=()):
    """Return a rebuilt, allowlisted copy of an `<svg>` element, or None.

    None means the block is dropped: not an svg element, over a size,
    element-count or depth cap. `dropped_attributes` removes allowlisted
    attributes the calling field refuses (`style` for strip_style fields,
    `class` for strip_classes fields).
    """
    if not isinstance(source.tag, str) or source.tag.lower() != "svg":
        return None
    try:
        clean = _build(source, 1, _Budget(), dropped_attributes)
    except _Rejected as rejection:
        _logger.warning("SVG block dropped: %s", rejection)
        return None
    if clean is None:
        return None
    clean.set("xmlns", SVG_NS)
    return clean


def _top_level_svgs(doc):
    found = []
    for element in doc.iter("svg"):
        if any(True for _ in element.iterancestors("svg")):
            continue
        found.append(element)
    return found


def _replace_with_text(element, token):
    """Swap `element` for `token` in the tree, keeping the tail text."""
    parent = element.getparent()
    tail = element.tail or ""
    if parent is None:
        return
    previous = element.getprevious()
    if previous is None:
        parent.text = (parent.text or "") + token + tail
    else:
        previous.tail = (previous.tail or "") + token + tail
    parent.remove(element)


def _detach_svgs(doc, nonce, dropped_attributes):
    """Validate every svg block, replace each with a token; return the stash."""
    stash = {}
    svgs = _top_level_svgs(doc)
    for index, element in enumerate(svgs):
        token = f"hamssvg{nonce}x{index}x"
        stash[index] = sanitize_svg_element(element, dropped_attributes)
        if element is doc:
            # The whole value was a single <svg>: the root cannot be swapped
            # out, so it becomes a plain <div> holding the token.
            for child in list(doc):
                doc.remove(child)
            doc.attrib.clear()
            doc.tag = "div"
            doc.text = token
            doc.tail = None
            continue
        _replace_with_text(element, token)
    return stash


def _in_blocked_context(element):
    node = element
    while node is not None:
        if isinstance(node.tag, str) and node.tag.lower() in _BLOCKED_CONTEXT:
            return True
        node = node.getparent()
    return False


def _split_text(text, pattern, stash, blocked):
    """Split text on tokens -> (leading text, [(svg or None, trailing text)])."""
    parts = pattern.split(text)
    lead = parts[0]
    items = []
    for position in range(1, len(parts), 2):
        svg = None if blocked else stash.pop(int(parts[position]), None)
        trailing = parts[position + 1]
        if svg is None:
            if items:
                items[-1] = (items[-1][0], items[-1][1] + trailing)
            else:
                lead += trailing
        else:
            items.append((svg, trailing))
    return lead, items


def _reattach_svgs(doc, stash, nonce):
    pattern = re.compile(rf"hamssvg{nonce}x(\d+)x")
    for owner in list(doc.iter()):
        if not isinstance(owner.tag, str):
            continue
        blocked = _in_blocked_context(owner)
        for child in list(owner):
            tail = child.tail
            if not tail or not pattern.search(tail):
                continue
            lead, items = _split_text(tail, pattern, stash, blocked)
            child.tail = lead or None
            position = owner.index(child) + 1
            for svg, trailing in items:
                svg.tail = trailing or None
                owner.insert(position, svg)
                position += 1
        if owner.text and pattern.search(owner.text):
            lead, items = _split_text(owner.text, pattern, stash, blocked)
            owner.text = lead or None
            for position, (svg, trailing) in enumerate(items):
                svg.tail = trailing or None
                owner.insert(position, svg)


def clean_document(doc, run_cleaner, dropped_attributes=()):
    """Run `run_cleaner(doc)` with every svg block validated around it."""
    if next(doc.iter("svg"), None) is None:
        return run_cleaner(doc)
    nonce = secrets.token_hex(12)
    stash = _detach_svgs(doc, nonce, dropped_attributes)
    result = run_cleaner(doc)
    _reattach_svgs(doc, stash, nonce)
    return result


def install():
    """Make Odoo's HTML sanitizer SVG-aware. Idempotent."""
    from odoo.tools import mail

    cleaner = mail._Cleaner
    current = cleaner.__call__
    if getattr(current, "_hams_svg_allowlist", False):
        return

    def call_with_svg_allowlist(self, doc):
        dropped = []
        if getattr(self, "style", False):
            dropped.append("style")
        if getattr(self, "strip_classes", False):
            dropped.append("class")
        return clean_document(
            doc, lambda tree: current(self, tree), tuple(dropped)
        )

    call_with_svg_allowlist._hams_svg_allowlist = True
    call_with_svg_allowlist.__wrapped__ = current
    cleaner.__call__ = call_with_svg_allowlist

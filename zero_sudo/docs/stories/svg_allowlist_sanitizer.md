<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Inline SVG Diagrams Survive the HTML Sanitizer, Safely

As an **Author** of a manual article, forum post or blog post, I want a circuit schematic written as
inline SVG to render, so that a diagram is not an empty box. As a **Security Reviewer**, I want that
to cost nothing in XSS protection: every SVG that reaches a stored `fields.Html` value has been
rebuilt from a strict allowlist `[@ANCHOR: zero_sudo:svg_allowlist_sanitizer]`.

## The Problem

`odoo.tools.mail.html_sanitize` (run by every `fields.Html` write) allows only HTML tags, so it
strips `svg`, `rect`, `path`, `circle`, `line`, `polygon` and the rest. A schematic arrived as an
empty box (seen on the live site in `ham_schematics`). Switching the sanitizer off for a field, or
adding the SVG tag names to the global list, would also admit `script`, `foreignObject`, `use`,
`animate`/`set` (which can set `href` to `javascript:`), `style` with `url()`, `feImage` and event
handlers.

## Design

`zero_sudo` (loaded before every Html-bearing module, and already the module that patches core for
cross-cutting reasons) replaces `odoo.tools.mail._Cleaner.__call__` at import time
`[@ANCHOR: zero_sudo:svg_allowlist_install]`. Every `html_sanitize()` call, from any module and
whatever options it passes, filters its parsed document through that one method. Patching
`html_sanitize` itself would miss the dozen modules that bound it by name
(`from odoo.tools import html_sanitize`) before the patch ran.

Per document:

1. Each outermost `<svg>` element is validated and **rebuilt** as a new element tree holding only
   allowlisted elements, attributes and values, then replaced by a random-nonce text token.
2. Odoo's cleaner runs unchanged on everything else. Content outside `<svg>` behaves exactly as
   before (a test compares against the stock cleaner).
3. Each token that survived in ordinary text content is swapped for its rebuilt SVG. A token that
   did not survive (the block sat in a comment, an attribute, a `textarea`, `title`, `math`...) is
   dropped, so SVG is never expanded where a browser would read raw text.

The SVG is read by the parser Odoo's sanitizer already uses (libxml2's HTML parser), so there is no
second parser whose reading could differ from what was validated, and XML entity and DTD processing
does not exist to abuse: `<!ENTITY>` declarations are ignored and `&xxe;` stays literal text.
libxml2 lowercases names, so the camel-case SVG spellings (`viewBox`, `linearGradient`, `clipPath`,
`gradientUnits`, `markerWidth`, `refX`...) are restored from the allowlist; a value an older
sanitize pass already mangled is canonicalised the same way.

**Elements kept:** `svg g path rect circle ellipse line polyline polygon text tspan defs title desc
linearGradient radialGradient stop clipPath marker symbol pattern`. Any other element is dropped
with its whole subtree.

**Attributes kept** (each value checked against its own pattern, rewritten in canonical form):
geometry and presentation attributes (`viewBox preserveAspectRatio width height x y x1 y1 x2 y2 cx
cy r rx ry d points transform fill fill-opacity fill-rule stroke stroke-width stroke-linecap
stroke-linejoin stroke-dasharray opacity font-family font-size font-weight text-anchor
dominant-baseline offset stop-color stop-opacity gradientUnits gradientTransform ...`), `id` and
`class` from a safe character set, `aria-*`, `role`, `xmlns` (forced to the SVG namespace),
`markerWidth markerHeight refX refY orient`, `xml:space`. `fill`, `stroke`, `clip-path` and
`marker-*` accept a colour or `url(#same-document-id)` only. `style` is accepted only as a list of
presentation declarations that pass the same validators and is re-serialised from the parsed
declarations; a field that sets `strip_style` or `strip_classes` loses `style` or `class` from the
SVG too. Every other attribute is dropped: all `on*` handlers, `href`, `xlink:href`, `src`, any
namespace-prefixed name, `data-*`, and any value that contains `javascript:`, `vbscript:`, `data:`,
`expression`, `@import`, a backslash or markup characters.

**Caps:** one block may hold at most 5000 elements, 256 KB of attribute and text data and 32 levels
of nesting. A block over a cap is dropped whole, never truncated.

## Threat Model

Attacker: any user who can write a `fields.Html` value (forum posts, portal users' articles and
posts, mail, website content) and wants script execution, request forgery, tracking or a redirect for
whoever views it. Out of scope: an administrator who can already write unsanitized fields.

| Goal | Mechanism blocked |
| --- | --- |
| Script in the page | `script`, `on*` handlers, `javascript:`/`data:` in any URL-like value, `animate`/`set` targeting `href`, `foreignObject` (HTML inside SVG), `use` of a `data:` or external document, `style` expressions |
| Outbound request (tracking, SSRF from a viewer's browser) | `image`, `use`, `feImage`, `filter`, `mask`, `pattern` with a URL, `url()` to anything but `#fragment`, `@import`, `link`, `meta refresh`, `base` |
| Parser differential / mutation XSS | The SVG is rebuilt, not filtered in place; only allowlisted names and attribute values are emitted, text is escaped, no `style`/`script`/`title`-as-RCDATA element can appear, and no HTML-breakout element (`p`, `br`, `img`...) can appear inside the foreign content |
| Entity expansion, XXE | No XML parser is used; `<!DOCTYPE>`/`<!ENTITY>` are ignored by the HTML parser |
| Placeholder forgery | Per-call random 96-bit nonce; a literal look-alike in the input is plain text |
| Resource exhaustion | Element, byte and depth caps |

## Residual Risks

- Odoo's own sanitizer limits still apply outside the SVG. `fields.Html(sanitize=False)` fields
  (stock `blog.post.content`, edited through the website builder) are not sanitized by the ORM at
  all, so this patch neither helps nor hurts them; `user_websites`' blog edit route calls
  `html_sanitize` itself and therefore does keep a safe SVG now.
- `fields.Html(sanitize_overridable=True)` compares `html_normalize(stored)` with
  `html_sanitize(stored)`. A stored SVG makes the two differ (the plain parser lowercases `viewBox`;
  the sanitized value restores it), so a user without the sanitize-override group who edits such a
  field is told the content is restricted. This already happened for any SVG before this change,
  because the stripped output never matched; it is not a new regression.
- A bare `<svg>` that is the whole value is wrapped in a `<div>`, because the cleaner cannot drop a
  root element. A value that stock Odoo reduces to a top-level `<svg>` (an odd `<!DOCTYPE>` prefix)
  gains that wrapper on the next pass; after that it no longer changes.
- `id` values are kept (needed for `url(#id)`), so two diagrams on one page can collide, and an `id`
  can shadow a `window` property (DOM clobbering). Odoo allows `id` on ordinary HTML too, so this is
  no new class of risk.
- A `title` or `desc` element inside the SVG survives (stock Odoo kills `title`). Both hold plain
  escaped text only.
- Mail clients ignore or strip inline SVG; nothing relies on it there.
- The patch is process-wide and applies once `zero_sudo` is imported by the server, for every
  database that server hosts.
- `user_websites`' page-arch sanitizer (`website_page._sanitize_user_arch`) is a separate XML
  sanitizer for QWeb arch, not `html_sanitize`; it is not covered by this module.

## Vectors Tested

`zero_sudo/tests/svg_corpus.py` holds a corpus of more than 100 vectors; each is cleaned on its own
and between two good schematics, must be idempotent, and is checked structurally (no forbidden
element anywhere, nothing but allowlisted elements inside an SVG, no `on*` attribute, no href-like
attribute, no dangerous URL scheme, no leaked placeholder, no injected host). The same corpus is
written through real `fields.Html` fields (partner comment, mail message, manual article, blog
post route, XML data load) and loaded in headless Chrome, where `window.__xss` must stay unset.
Families: script elements and CDATA; `on*` handlers on every element kind; `a`/`xlink:href`
`javascript:`; `animate`/`set`/`animateTransform`/`animateMotion` changing `href` or an event
handler; `use` pointing at `data:`, external and local documents; `image` with `javascript:` or
external `href`; `foreignObject` holding `script`, `iframe`, `body onload`, `img onerror`, `srcdoc`;
`style` elements and attributes with `url()`, `@import`, `expression`, CSS escapes and comment
smuggling; internal, external, billion-laughs and parameter entities and external DTDs; mutation
XSS through nested `svg`/`math`/`style`/`noscript`/`title`/`textarea`/`xmp`/`plaintext`/`iframe`/`desc`
CDATA and `br`/`p`/`pre` breakouts; case, whitespace, tab, newline, entity and hex-entity
obfuscation of tags and schemes; NUL bytes in tags, attributes and schemes; quote, backtick and
entity attribute injection; SVG inside HTML comments, conditional comments, attribute values and
`data-*` attributes; CDATA breakouts; namespace-prefix games (`svg:script`, `svg:use`, `svg:svg`,
declared XHTML prefixes, forged `xmlns`); `filter`/`feImage`/`mask`/`switch`/`symbol`+`use`;
`iframe`/`object`/`embed`/`link`/`meta`/`base` inside SVG; `url()` paint values pointing at
external, `javascript:`, `data:` and protocol-relative targets; text that looks like markup;
stray closing tags; a forged placeholder token; unclosed and mis-nested SVG; `textPath href`;
`data-*` payloads. Cap tests cover element count, depth and size on both sides of each limit.

## Verification

`zero_sudo/tests/test_svg_sanitizer.py` (unit and `fields.Html` coverage, including every module
that binds `html_sanitize` by name, the data-file load path and the documentation installer),
`knowledge/tests/test_svg_article.py` (manual article), `user_websites/tests/
test_user_websites_blog_post.py` (blog route). The real-browser tests (shapes have non-zero
rendered boxes, `currentColor` follows the page and the theme, the diagram scales at 375 px, no
script ran, no console error, no CSP violation) live in hams_com's `theme_hams` tests, which need the
site's own theme and Content-Security-Policy.

Modules to rehearse the upgrade on: `zero_sudo`, `knowledge`, `user_websites` (hams_open); the
patch changes the stored result of every `fields.Html` write that contains an `<svg>`, so any
module whose data files ship inline SVG (`ham_schematics` in hams_com) also changes on `-u`.

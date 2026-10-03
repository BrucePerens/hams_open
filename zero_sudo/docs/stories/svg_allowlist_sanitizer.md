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
dominant-baseline offset stop-color stop-opacity gradientUnits gradientTransform ...`), `xmlns`
(forced to the SVG namespace), `markerWidth markerHeight refX refY orient`, `xml:space`.
`fill`, `stroke`, `clip-path` and `marker-*` accept a colour or `url(#same-document-id)` only.
Because an `<svg>` is a replaced element that paints above neighbouring text, the outer and nested
`svg` elements get tighter rules: `width`/`height` must be `auto`, at most 100%, 4000px or 250em,
and `transform` is dropped. `class` is dropped on every SVG element (utility classes such as
`position-fixed w-100 h-100` would pin a transparent shape over the page; the shipped schematics
use none). `role` is limited to `img`, `graphics-document`, `presentation`, `none`. Of the `aria-*`
attributes only `aria-label`, `aria-hidden` and `aria-roledescription` survive; the ones that point
at other ids in the page (`owns controls labelledby describedby flowto activedescendant`) do not.
`id` is kept (local `url(#id)` references need it) unless it begins `__` or names a window/document
property (`cookie location name top parent self body forms ...`, case-insensitive), which would
clobber it. `style` is accepted only as a list of presentation declarations (`fill stroke
stroke-* opacity font-* text-anchor color stop-* background-color` and a non-negative
`margin-top`/`margin-bottom` of at most 99px or 9em) re-serialised from the parsed declarations; it
can never size, position, stack or offset the drawing (no width, height, display, position, inset,
transform, z-index or left/right/negative margin). A field that sets `strip_style` or
`strip_classes` loses `style` from the SVG too. Every other attribute is dropped: all `on*`
handlers, `href`, `xlink:href`, `src`, any namespace-prefixed name, `data-*`, and any value that
contains `javascript:`, `vbscript:`, `data:`, `expression`, `@import`, a backslash or markup
characters.

**Caps:** one block may hold at most 5000 elements, 256 KB of attribute and text data and 32 levels
of nesting. Per `html_sanitize` call, across all blocks, at most 1000 blocks (Bruce's decision) and 1 MB of SVG data
are kept (document order); anything past a cap is dropped whole, never truncated. The per-call
totals bound both the time and the output size (an empty `<svg></svg>` grows about 4x when rebuilt
with its namespace). Placeholders are comments swapped in O(1) and removed in one pass per parent,
so 20000 adjacent blocks cost about 0.1-0.3 s (a text-token design was quadratic: 11 s).

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
| Resource exhaustion | Element, byte and depth caps per block; block and byte totals per call; linear-time placeholder swap; length-capped scalar values |
| UI redress (a large or offset SVG with a transparent shape covering links) | No `class`, no size/position/stack/offset in `style`, bounded root `width`/`height`, no root `transform` |
| DOM clobbering, ARIA relationships into the page | Clobbering ids refused; `aria-owns`/`controls`/`labelledby`/... and arbitrary `role` dropped |

## Residual Risks

- Odoo's own sanitizer limits still apply outside the SVG. `fields.Html(sanitize=False)` fields
  (stock `blog.post.content`, edited through the website builder) are not sanitized by the ORM at
  all, so this patch neither helps nor hurts them; `user_websites`' blog edit route calls
  `html_sanitize` itself and therefore does keep a safe SVG now.
- **New behaviour for `fields.Html(sanitize_overridable=True)`** (stock: `survey.question` and
  `survey.survey` description, `gamification.karma.rank` motivational text). On write, a user
  without the sanitize-override group has the stored value compared as
  `html_normalize(stored)` against `html_sanitize(stored)`, and a difference raises "includes
  content that is restricted for security reasons". Before this change an SVG was stripped at write
  time, so the stored value contained none and the two always agreed. Now the stored value keeps
  the SVG with its camel-case names, the plain `html_normalize` parser lowercases `viewBox` and
  friends, the two differ, and such a user gets that error on their *next* edit of a field that
  holds an SVG. Users in the override group are unaffected. Mail templates and the other
  `sanitize_overridable` fields that never hold SVG behave as before. Accepting it is a decision
  for the owner of those fields; the alternative is to make `html_normalize` canonicalise the SVG
  too (not done: it would patch a second core function).
- A bare `<svg>` that is the whole value is wrapped in a `<div>`, because the cleaner cannot drop a
  root element. A value that stock Odoo reduces to a top-level `<svg>` (an odd `<!DOCTYPE>` prefix)
  gains that wrapper on the next pass; after that it no longer changes.
- `id` values are kept (needed for `url(#id)`), so two diagrams on one page can still collide with
  each other (a gradient or marker id reused by two diagrams resolves to the first). Names that
  would shadow `window`/`document` properties are refused, but the list is a denylist; Odoo allows
  `id` on ordinary HTML too.
- A large `width`/`height` on shapes *inside* an SVG is not capped: the SVG viewport clips them,
  and the root's own size is capped.
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

## Where Diagrams Can Live

Inline SVG works in any sanitized `fields.Html` value and in `.html` documentation files
(`data/documentation.html`, installed through an article write). A **`.md` documentation file
cannot carry an inline diagram**: python-markdown splits inline SVG, hoisting the shapes out of the
`<svg>` into separate paragraphs, before the sanitizer ever sees it (found while testing the
installer's markdown path; the unit test was dropped for that reason). Static `.svg` files served as
images remain a valid choice and need no sanitizer.

## Verification

`zero_sudo/tests/test_svg_sanitizer.py` (unit and `fields.Html` coverage, including every module
that binds `html_sanitize` by name and the data-file load path),
`knowledge/tests/test_svg_article.py` (manual article), `user_websites/tests/
test_user_websites_blog_post.py` (blog route). The real-browser tests (shapes have non-zero
rendered boxes, `currentColor` follows the page and the theme, the diagram scales at 375 px, no
script ran, no console error, no CSP violation) live in hams_com's `theme_hams` tests, which need the
site's own theme and Content-Security-Policy.

Modules to rehearse the upgrade on: `zero_sudo`, `knowledge`, `user_websites` (hams_open); the
patch changes the stored result of every `fields.Html` write that contains an `<svg>`, so any
module whose data files ship inline SVG (`ham_schematics` in hams_com) also changes on `-u`.

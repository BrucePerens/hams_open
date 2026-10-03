# Story: Service-Worker Update Toast [@ANCHOR: caching:COMM_story_sw_update_toast]

As a **Site Visitor**,
I want to be told when a new version of the site has loaded in the background,
so that I can reload and get it without losing anything I'm doing on the current page.

## The Process

1. When the service worker signals `NEW_VERSION_INSTALLED`, the toast would normally show
   immediately -- but this toast is a real `main_components` element, so it renders on every page,
   including a brand-new visitor's very first page load, at the exact moment Odoo's own
   cookie-consent bar is up. Both are fixed-position and the toast's z-index sits on top of the
   consent bar, silently eating clicks meant for its buttons.
2. Rather than hardcode a different screen position (which would only trade one breakpoint-fragile
   collision for another), the toast defers its own reveal until no Bootstrap modal is currently
   open, re-checking on `hidden.bs.modal`, which is correct regardless of viewport size and
   generalizes to any other modal the site might show at the same moment
   `[@ANCHOR: caching:toast_deferred_while_modal_open]`.

**Status:** Verified by `[@ANCHOR: test_toast_deferred_while_modal_open]`.

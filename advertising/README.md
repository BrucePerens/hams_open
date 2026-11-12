# advertising

Optional, consent-gated Google AdSense placement for the shared website layout (`docs/proposals/ADVERTISING.md`). It renders
nothing until an administrator sets a real AdSense client id (and a slot id for each placement) on the website, and ad
personalisation waits for the same cookies-bar consent signal that website analytics uses.

## Browser scripts

The pages carry no inline scripts, so a strict Content-Security-Policy without `'unsafe-inline'` can serve them from
`'self'` and pages stay cacheable. The one per-request fact, whether every optional cookie is already accepted, is passed in
a non-executed `<script type="application/json" id="advertising_adsense_consent">` block.

- `static/src/js/adsense_consent.js` is loaded synchronously before the AdSense loader. It defines the global `gtag()` that
  appends to `window.dataLayer` and sets the Google Consent Mode defaults to denied for advertising storage, user data,
  personalisation and analytics storage, so nothing personalised is allowed before the visitor agrees.
  `advertising:adsense_consent_js`
- The same file defines `adsenseConsentsGranted()`, which sends the Consent Mode update that grants advertising storage, user
  data and personalisation. It runs at once when the page was rendered with every consent already granted, otherwise on the
  first `optionalCookiesAccepted` event (once).
  `advertising:adsense_consents_granted`
- `static/src/js/adsense_slot_push.js` is included once right after each `<ins class="adsbygoogle">` slot and does the one
  `(adsbygoogle = window.adsbygoogle || []).push({})` that asks AdSense to fill that slot. A browser runs every `<script src>`
  element even when two share a URL, so each slot gets exactly one push.
  `advertising:adsense_slot_push_js`

## Anchor reference

Full entries for the three browser scripts named above.

### `advertising:adsense_consent_js` [@ANCHOR: advertising:adsense_consent_js]

**Purpose.** Defines `gtag()` and sets Google Consent Mode to denied by default, so nothing personalised is allowed before the visitor agrees. It must load before the AdSense loader.

**Where.** `static/src/js/adsense_consent.js`, included by `views/website_layout.xml` as a synchronous `<script id="adsense_consent_config" src=...>` in `<head>`, only when `website.google_adsense_client_id` is set and the page is not in the website editor. It is a plain script with no module wrapper, so `gtag` stays a global.

**What it does.** Reads the JSON block `<script type="application/json" id="advertising_adsense_consent">` (never executed; it carries `all_consents_granted`, from `website._allConsentsGranted()`), makes `window.dataLayer` an array, defines `gtag()` (pushes its `arguments` object onto `dataLayer`, as Google's tag expects) and calls `gtag("consent", "default", ...)` with `ad_storage`, `ad_user_data`, `ad_personalization` and `analytics_storage` all `denied`.

**Concerns.** It throws if the JSON block is missing (the template always emits it first). No inline script is needed, so a strict Content-Security-Policy without `unsafe-inline` allows it from `'self'`, and pages stay cacheable.

**See also.** Test `tests/test_advertising_layout.py` (`test_05_consent_mode_default_denies_until_accepted`); `advertising:adsense_consents_granted`.

### `advertising:adsense_consents_granted` [@ANCHOR: advertising:adsense_consents_granted]

**Purpose.** Tells Google that the visitor has accepted optional cookies, so ad personalisation may begin.

**Call interface.** `adsenseConsentsGranted()` in `static/src/js/adsense_consent.js`: no arguments, no return value, global function. It calls `gtag("consent", "update", {ad_storage: "granted", ad_user_data: "granted", ad_personalization: "granted"})`. It does not grant `analytics_storage`, which stays denied.

**When it runs.** Immediately at load when the server rendered the page with `all_consents_granted` true in the JSON block (every optional cookie already accepted); otherwise it is registered with `{once: true}` as the listener of the document event `optionalCookiesAccepted`, which the cookies bar fires after the visitor accepts. It therefore runs at most once per page view and never grants consent on its own.

**Concerns.** Idempotent (repeating it changes nothing). It sends no network request itself; Google's loader reads `dataLayer`. If the event never fires, consent stays denied, which is the safe default.

**See also.** Test `tests/test_advertising_layout.py`; `advertising:adsense_consent_js`.

### `advertising:adsense_slot_push_js` [@ANCHOR: advertising:adsense_slot_push_js]

**Purpose.** Asks AdSense to fill one ad slot. Google's documented way is the inline call `(adsbygoogle = window.adsbygoogle || []).push({})`; it lives in a static file so pages need no inline script under a strict Content-Security-Policy.

**Where.** `static/src/js/adsense_slot_push.js`, included by `views/website_layout.xml` right after each `<ins class="adsbygoogle">` element (the footer slot `advertising_footer_slot` and the sidebar slot `advertising_sidebar_slot`). The file has one statement and defines no function.

**Behaviour.** It creates `window.adsbygoogle` if the loader has not yet, and pushes an empty object. The browser runs every `<script src>` element even when two share a URL, so each slot gets exactly one push and each `<ins>` is filled by the loader (`adsense_loader`, loaded async in `<head>`) when it is ready. No arguments, no return, no state of its own.

**Concerns.** Pushing more times than there are `<ins>` elements makes AdSense log an error; adding a slot in the template requires one include. It is only rendered when the publisher id and that slot's id are both configured.

**See also.** Test `tests/test_advertising_layout.py`; `advertising:adsense_consent_js`.

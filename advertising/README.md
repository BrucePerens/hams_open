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
  [@ANCHOR: advertising:adsense_consent_js]
- The same file defines `adsenseConsentsGranted()`, which sends the Consent Mode update that grants advertising storage, user
  data and personalisation. It runs at once when the page was rendered with every consent already granted, otherwise on the
  first `optionalCookiesAccepted` event (once).
  [@ANCHOR: advertising:adsense_consents_granted]
- `static/src/js/adsense_slot_push.js` is included once right after each `<ins class="adsbygoogle">` slot and does the one
  `(adsbygoogle = window.adsbygoogle || []).push({})` that asks AdSense to fill that slot. A browser runs every `<script src>`
  element even when two share a URL, so each slot gets exactly one push.
  [@ANCHOR: advertising:adsense_slot_push_js]

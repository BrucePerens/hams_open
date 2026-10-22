/* SPDX-License-Identifier: AGPL-3.0-or-later */
/** @odoo-module **/
/* The pragma above only tells the asset-bundler lint this file was considered: this file is NOT in any asset bundle. It is served as-is
 * and loaded by a plain <script src> (classic script, globals on purpose), so Odoo never transpiles it and the pragma is inert. */

/*
 * AdSense Consent Mode defaults, loaded with a synchronous <script id="adsense_consent_config" src>
 * from views/website_layout.xml, before the AdSense loader. Was an inline script until 2026-10-03;
 * moved so a strict Content-Security-Policy without 'unsafe-inline' (and without per-response
 * nonces, so pages stay cacheable) allows it through 'self'. The one per-request value, whether
 * every optional cookie is already accepted, arrives in
 * <script type="application/json" id="advertising_adsense_consent">, which is never executed.
 * Not wrapped in a function: `gtag` and `adsenseConsentsGranted` were globals and stay globals.
 */
/* eslint-disable no-unused-vars */
var advertisingAdsenseConsent = JSON.parse(
    document.getElementById("advertising_adsense_consent").textContent
);
window.dataLayer = window.dataLayer || [];
// [@ANCHOR: advertising:adsense_consent_js]
function gtag() {
    window.dataLayer.push(arguments);
}
gtag("consent", "default", {
    ad_storage: "denied",
    ad_user_data: "denied",
    ad_personalization: "denied",
    analytics_storage: "denied",
});
// [@ANCHOR: advertising:adsense_consents_granted]
function adsenseConsentsGranted() {
    gtag("consent", "update", {
        ad_storage: "granted",
        ad_user_data: "granted",
        ad_personalization: "granted",
    });
}
if (advertisingAdsenseConsent.all_consents_granted) {
    adsenseConsentsGranted();
} else {
    document.addEventListener("optionalCookiesAccepted", adsenseConsentsGranted, { once: true });
}

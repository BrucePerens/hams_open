/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

// FormShieldErrorNotice.start(): tells the visitor why a shielded form was refused, then cleans the address. It reads
// document.location and calls window.history, so the method is compiled with stand-ins for those two (the same device
// hams_com's event_interactions.test.js uses); the notification service is a recorder.
import { afterEach, beforeEach, describe, expect, test } from "@odoo/hoot";
import { translatedTerms, translationLoaded } from "@web/core/l10n/translation";
import { _t } from "@web/core/l10n/translation";
import { FormShieldErrorNotice, noticeText } from "@form_shield/js/error_notice";

/** Runs start() on a page whose address is `search` under `/contact`; returns what was shown and how the address was cleaned. */
function runStart(search) {
    const seen = { notices: [], replaced: [] };
    const scope = {
        document: { location: { search, pathname: "/contact" } },
        window: { history: { replaceState: (...args) => seen.replaced.push(args) } },
        noticeText,
        _t,
    };
    const start = new Function(...Object.keys(scope), `return (${"function " + String(FormShieldErrorNotice.prototype.start)});`)(...Object.values(scope));
    start.call({ services: { notification: { add: (message, options) => seen.notices.push([String(message), options]) } } });
    return seen;
}

describe("form_shield_error_notice_start", () => {
    describe.current.tags("form_shield_error_notice_start");

    // The words shown are lazily translated strings; mark translations as loaded so they can be read.
    beforeEach(() => { translatedTerms[translationLoaded] = true; });
    afterEach(() => { translatedTerms[translationLoaded] = false; });

    test("a refused form's reason is shown as a danger notification and removed from the address, keeping other parameters", () => {
        // Tests [@ANCHOR: form_shield_js_error_notice_form_shield_error_notice]
        // Tests [@ANCHOR: form_shield:js_error_notice_start]
        const seen = runStart("?form_shield_error=too_fast&tab=2");
        expect(seen.notices.length).toBe(1);
        expect(seen.notices[0][0]).toBe(String(noticeText("too_fast")));
        expect(seen.notices[0][1].type).toBe("danger");
        expect(seen.notices[0][1].sticky).toBe(false);
        expect(String(seen.notices[0][1].title)).toBe("Not sent");
        expect(seen.replaced).toEqual([[{}, "", "/contact?tab=2"]]);
    });

    test("a reason on its own leaves a bare path, and an unknown reason gets the default words", () => {
        // Tests [@ANCHOR: form_shield:js_error_notice_start]
        const seen = runStart("?form_shield_error=made_up");
        expect(seen.notices[0][0]).toBe(String(noticeText("made_up")));
        expect(seen.replaced).toEqual([[{}, "", "/contact"]]);
    });

    test("a page without the parameter shows nothing and leaves the address alone", () => {
        // Tests [@ANCHOR: form_shield:js_error_notice_start]
        const seen = runStart("?tab=2");
        expect(seen.notices.length).toBe(0);
        expect(seen.replaced.length).toBe(0);
    });
});

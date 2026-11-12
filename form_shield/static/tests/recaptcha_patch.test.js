/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

// The patch that makes Odoo's own forms ask the shield for their token instead of Google (recaptcha_patch.js), on the real
// ReCaptcha class. The shield session is replaced by a recorder where a test needs to control its answer; the challenge
// request every new session makes is stubbed.
import { afterEach, beforeEach, describe, expect, mockFetch, test } from "@odoo/hoot";
import { translatedTerms, translationLoaded } from "@web/core/l10n/translation";
import { patchWithCleanup } from "@web/../tests/web_test_helpers";
import { session } from "@web/session";
import { ReCaptcha } from "@google_recaptcha/js/recaptcha";
import { ShieldSession } from "@form_shield/js/shield_client";
import { ENFORCED_ACTIONS } from "@form_shield/js/recaptcha_patch";

describe("form_shield_recaptcha_patch", () => {
    describe.current.tags("form_shield_recaptcha_patch");

    // The words shown are lazily translated strings; mark translations as loaded so they can be read.
    beforeEach(() => { translatedTerms[translationLoaded] = true; });
    afterEach(() => { translatedTerms[translationLoaded] = false; });

    test("loadLibs loads no Google script, and gives a visitor a shield session to start solving", async () => {
        // Tests [@ANCHOR: form_shield:js_recaptcha_load_libs]
        mockFetch(() => ({ challenge: "c", difficulty: 0 }));
        patchWithCleanup(session, { is_website_user: true });
        const visitor = new ReCaptcha();
        expect(await visitor.loadLibs()).toBe(true);
        expect(visitor._shield).toBeInstanceOf(ShieldSession);
        patchWithCleanup(session, { is_website_user: false });
        const member = new ReCaptcha();
        expect(await member.loadLibs()).toBe(true);
        expect(member._shield).toBe(undefined);
    });

    test("getToken answers with a message, and no token, for a form the server does not enforce or for a signed-in member", async () => {
        // Tests [@ANCHOR: form_shield:js_recaptcha_get_token]
        mockFetch(() => ({}));
        patchWithCleanup(session, { is_website_user: true });
        const askedFor = [];
        const captcha = new ReCaptcha();
        captcha._shield = { token: async (action) => { askedFor.push(action); return "x"; } };
        const login = await captcha.getToken("login");
        expect(login.token).toBe(undefined);
        expect(String(login.message)).toBe("No automatic check is asked for on this form.");
        patchWithCleanup(session, { is_website_user: false });
        const member = await captcha.getToken("website_form");
        expect(member.token).toBe(undefined);
        expect(askedFor.length).toBe(0);
        expect([...ENFORCED_ACTIONS].sort()).toEqual(["website_event_registration", "website_form", "website_mail_follow", "website_mass_mailing_subscribe"]);
    });

    test("getToken solves for an enforced form's action and uses a new session for the next submit; a failed solve returns an error", async () => {
        // Tests [@ANCHOR: form_shield:js_recaptcha_get_token]
        mockFetch(() => ({ challenge: "c", difficulty: 0 }));
        patchWithCleanup(session, { is_website_user: true });
        const askedFor = [];
        const captcha = new ReCaptcha();
        const first = { token: async (action) => { askedFor.push(action); return "chal~7"; } };
        captcha._shield = first;
        expect(await captcha.getToken("website_form")).toEqual({ token: "chal~7" });
        expect(askedFor).toEqual(["website_form"]);
        expect(captcha._shield).not.toBe(first);
        expect(captcha._shield).toBeInstanceOf(ShieldSession);
        const failing = new ReCaptcha();
        failing._shield = { token: async () => { throw new Error("no worker"); } };
        const result = await failing.getToken("website_mail_follow");
        expect(String(result.error)).toBe("The automatic check could not run in this browser. Please try again.");
        expect(failing._shield).toBeInstanceOf(ShieldSession);
    });
});

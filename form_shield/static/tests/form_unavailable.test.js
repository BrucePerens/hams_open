/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

// FormShieldForm.unavailable(): what a shielded form does when the automatic check cannot run in the browser. Real form DOM,
// the challenge request stubbed.
import { afterEach, beforeEach, describe, expect, mockFetch, test } from "@odoo/hoot";
import { translatedTerms, translationLoaded } from "@web/core/l10n/translation";
import { FormShieldForm } from "@form_shield/js/form_shield_form";

/** A form with the optional fallback link and question box; `parts` names which of them it has. */
function makeForm(...parts) {
    const form = document.createElement("form");
    const markup = [
        '<div data-form-shield-status></div>',
        parts.includes("link") ? '<a data-form-shield-fallback hidden href="/simple">simple</a>' : "",
        parts.includes("question")
            ? '<div data-form-shield-question hidden><input name="shield_challenge"/><span data-form-shield-question-text></span></div>'
            : "",
    ].join("");
    form.innerHTML = markup;
    const said = [];
    const fake = { el: form, released: false, say: (text) => said.push(String(text)) };
    return { form, fake, said };
}

describe("form_shield_form_unavailable", () => {
    describe.current.tags("form_shield_form_unavailable");

    // The words shown are lazily translated strings; mark translations as loaded so they can be read.
    beforeEach(() => { translatedTerms[translationLoaded] = true; });
    afterEach(() => { translatedTerms[translationLoaded] = false; });

    test("with a simple-question page, it shows the link and says to use it", async () => {
        // Tests [@ANCHOR: form_shield:js_form_unavailable]
        const { form, fake, said } = makeForm("link", "question");
        await FormShieldForm.prototype.unavailable.call(fake);
        expect(form.querySelector("[data-form-shield-fallback]").hidden).toBe(false);
        expect(said).toEqual(["Your browser could not run the automatic check. Please use the simple question form."]);
        expect(fake.released).toBe(false);
    });

    test("without a page of its own, it asks the question inside the form and lets the form be sent", async () => {
        // Tests [@ANCHOR: form_shield:js_form_unavailable]
        const requests = [];
        mockFetch((url, init) => {
            requests.push([url, String(init.body)]);
            return { challenge: "c1", question: "What is 2 plus 3?" };
        });
        const { form, fake, said } = makeForm("question");
        await FormShieldForm.prototype.unavailable.call(fake);
        expect(requests).toEqual([["/form_shield/challenge", "kind=q"]]);
        expect(form.querySelector('input[name="shield_challenge"]').value).toBe("c1");
        expect(form.querySelector("[data-form-shield-question-text]").textContent).toBe("What is 2 plus 3?");
        expect(form.querySelector("[data-form-shield-question]").hidden).toBe(false);
        expect(fake.released).toBe(true);
        expect(said[0]).toInclude("answer the question below");
    });

    test("with neither, it says the check could not run, and when the question cannot be fetched it says to try later", async () => {
        // Tests [@ANCHOR: form_shield:js_form_unavailable]
        const bare = makeForm();
        await FormShieldForm.prototype.unavailable.call(bare.fake);
        expect(bare.said).toEqual(["Your browser could not run the automatic check."]);
        mockFetch(() => new Response("no", { status: 503 }));
        const failing = makeForm("question");
        await FormShieldForm.prototype.unavailable.call(failing.fake);
        expect(failing.said).toEqual(["The check is unavailable right now. Please try again later."]);
        expect(failing.fake.released).toBe(false);
        expect(failing.form.querySelector("[data-form-shield-question]").hidden).toBe(true);
    });
});

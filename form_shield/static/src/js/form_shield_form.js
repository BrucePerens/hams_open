/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { Interaction } from "@web/public/interaction";
import { registry } from "@web/core/registry";
import { _t } from "@web/core/l10n/translation";
import { ShieldSession, fetchChallenge } from "@form_shield/js/shield_client";

// [@ANCHOR: form_shield:form_interaction]
// Verified by [@ANCHOR: form_shield:test_report_form_happy_path_tour]
//
// Wires an ordinary HTML <form data-form-shield="<form id>"> to the shield. The form carries:
//   <input type="hidden" name="shield_response"/>                 receives the answer
//   <div data-form-shield-status role="status" aria-live="polite"/> where progress is told
//   <a data-form-shield-fallback hidden href="...">               shown if the check cannot run
// Solving starts when the form is shown (when it sits in a Bootstrap modal, when the modal
// opens) and never blocks typing. Pressing submit before it finishes holds the submit, says so
// in the status line, and sends the form the moment the answer is ready.
export class FormShieldForm extends Interaction {
    static selector = "form[data-form-shield]";
    dynamicContent = {
        _root: { "t-on-submit": this.onSubmit },
    };

    setup() {
        this.formId = this.el.dataset.formShield;
        this.session = null;
        this.released = false;
    }

    start() {
        const modal = this.el.closest(".modal");
        if (modal) {
            this.addListener(modal, "show.bs.modal", () => this.begin());
        } else {
            this.begin();
        }
    }

    get statusEl() {
        return this.el.querySelector("[data-form-shield-status]");
    }

    say(text) {
        if (this.statusEl) {
            this.statusEl.textContent = text;
        }
    }

    begin() {
        this.released = false;
        const field = this.el.querySelector('input[name="shield_response"]');
        if (field) {
            field.value = "";
        }
        this.say(_t("Verifying you are human…"));
        this.session = new ShieldSession(this.formId);
        this.pending = this.session
            .token()
            .then((answer) => {
                if (field) {
                    field.value = answer;
                }
                this.say(_t("Verified. You can send the form."));
                return answer;
            })
            .catch(async () => {
                await this.unavailable();
                return null;
            });
    }

    async unavailable() {
        const link = this.el.querySelector("[data-form-shield-fallback]");
        if (link) {
            link.hidden = false;
            this.say(
                _t("Your browser could not run the automatic check. Please use the simple question form.")
            );
            return;
        }
        // No page of its own for the question: ask it here, inside the form.
        const box = this.el.querySelector("[data-form-shield-question]");
        if (!box) {
            this.say(_t("Your browser could not run the automatic check."));
            return;
        }
        try {
            const issued = await fetchChallenge("q");
            box.querySelector('input[name="shield_challenge"]').value = issued.challenge;
            box.querySelector("[data-form-shield-question-text]").textContent = issued.question;
            box.hidden = false;
            this.released = true;
            this.say(_t("Your browser could not run the automatic check. Please answer the question below, then send the form."));
        } catch {
            this.say(_t("The check is unavailable right now. Please try again later."));
        }
    }

    async onSubmit(ev) {
        if (this.released || !this.pending) {
            return;
        }
        ev.preventDefault();
        const submitter = ev.submitter;
        this.say(_t("Verifying you are human… the form will be sent as soon as this finishes."));
        this.el.setAttribute("aria-busy", "true");
        const answer = await this.waitFor(this.pending);
        this.el.removeAttribute("aria-busy");
        if (!answer) {
            return;
        }
        this.released = true;
        this.el.requestSubmit(submitter);
    }
}

registry.category("public.interactions").add("form_shield.FormShieldForm", FormShieldForm);

/** @odoo-module **/

// _onModalShow() is real, pure DOM logic (read the triggering button's
// data-url, inject it into the modal's url field, clear the description/
// email fields) -- exercised here against real DOM nodes (hoot tests run
// in a real browser), not a faked `this`/`querySelector`, since building
// the actual elements is just as easy and tests the real querySelector
// calls too. Fixtures are built via explicit createElement/append calls,
// not innerHTML + a template literal, to stay clear of this codebase's
// own DOM-XSS linter pattern (it flags that shape regardless of whether
// the literal has any interpolation, and createElement is no less clear
// here).
import { describe, expect, test } from "@odoo/hoot";
import { ViolationReportModal } from "@user_websites/js/violation_report";

function makeInput(name, value) {
    const input = document.createElement("input");
    input.name = name;
    input.value = value;
    return input;
}

function makeModal({ withDescriptionAndEmail = true } = {}) {
    const modal = document.createElement("div");
    modal.appendChild(makeInput("url", "stale-value"));
    if (withDescriptionAndEmail) {
        const description = document.createElement("textarea");
        description.name = "description";
        description.value = "stale description";
        modal.appendChild(description);
        modal.appendChild(makeInput("email", "stale@example.com"));
    }
    return modal;
}

describe("violation_report", () => {
    describe.current.tags("user_websites_violation_report");

    test("_onModalShow injects the triggering button's data-url and clears prior input", () => {
        const instance = Object.create(ViolationReportModal.prototype);
        const modal = makeModal();
        const button = document.createElement("button");
        button.setAttribute("data-url", "/blog/some-offending-post");

        instance._onModalShow({ relatedTarget: button, currentTarget: modal });

        expect(modal.querySelector('input[name="url"]').value).toBe("/blog/some-offending-post");
        expect(modal.querySelector('textarea[name="description"]').value).toBe("");
        expect(modal.querySelector('input[name="email"]').value).toBe("");
    });

    test("_onModalShow does nothing when the event carries no relatedTarget", () => {
        const instance = Object.create(ViolationReportModal.prototype);
        const modal = makeModal();

        instance._onModalShow({ relatedTarget: null, currentTarget: modal });

        // Unchanged from makeModal()'s own stale values -- confirms the
        // early return, not just "didn't throw".
        expect(modal.querySelector('input[name="url"]').value).toBe("stale-value");
        expect(modal.querySelector('textarea[name="description"]').value).toBe("stale description");
    });

    test("_onModalShow tolerates a modal missing the optional description/email fields", () => {
        const instance = Object.create(ViolationReportModal.prototype);
        const modal = makeModal({ withDescriptionAndEmail: false });
        modal.querySelector('input[name="url"]').value = "";
        const button = document.createElement("button");
        button.setAttribute("data-url", "/forum/post/42");

        // Must not throw even though description/email inputs don't exist.
        instance._onModalShow({ relatedTarget: button, currentTarget: modal });

        expect(modal.querySelector('input[name="url"]').value).toBe("/forum/post/42");
    });
});

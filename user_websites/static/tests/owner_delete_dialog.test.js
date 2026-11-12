/** @odoo-module **/

// The owner dashboard's delete confirmation (OwnerDeleteDialog), made by hand on real DOM the way the Interaction framework
// would make it. Fixtures are built with createElement, not innerHTML and a template literal, to stay clear of the DOM-XSS linter.
import { describe, expect, test } from "@odoo/hoot";
import { OwnerDeleteDialog } from "@user_websites/js/owner_delete_dialog";

function element(tag, id, parent) {
    const node = document.createElement(tag);
    if (id) {
        node.id = id;
    }
    parent.appendChild(node);
    return node;
}

/** The dialog, its form, its two text spans and Cancel, plus one delete link; showModal and close are recorders. */
function makePage({ withShowModal = true } = {}) {
    const page = document.createElement("div");
    const dialog = element("dialog", "uw_delete_dialog", page);
    const calls = [];
    if (withShowModal) {
        dialog.showModal = () => { dialog.setAttribute("open", ""); calls.push("showModal"); };
    } else {
        dialog.showModal = undefined;
    }
    dialog.close = () => calls.push("close");
    element("form", "uw_delete_form", dialog);
    element("span", "uw_delete_what", dialog);
    element("span", "uw_delete_name", dialog);
    const cancel = element("button", "uw_delete_cancel", dialog);
    const link = element("a", null, page);
    link.className = "uw-delete-link";
    link.dataset.uwAction = "/my/site/delete/5";
    link.dataset.uwWhat = "post";
    link.dataset.uwTitle = "Hello";
    document.body.appendChild(page);
    return { page, dialog, calls, cancel, link };
}

describe("owner_delete_dialog", () => {
    describe.current.tags("user_websites_owner_delete_dialog");

    test("start makes a click on a delete link fill in and open the dialog instead of following the link, and Cancel closes it", () => {
        // Tests [@ANCHOR: user_websites_js_owner_delete_dialog_owner_delete_dialog]
        // Tests [@ANCHOR: user_websites:js_owner_delete_dialog_start]
        const { page, dialog, calls, cancel, link } = makePage();
        try {
            const instance = Object.create(OwnerDeleteDialog.prototype);
            instance.el = dialog;
            instance.start();
            const click = new MouseEvent("click", { cancelable: true });
            link.dispatchEvent(click);
            expect(click.defaultPrevented).toBe(true);
            expect(document.getElementById("uw_delete_form").getAttribute("action")).toBe("/my/site/delete/5");
            expect(document.getElementById("uw_delete_what").textContent).toBe("post");
            expect(document.getElementById("uw_delete_name").textContent).toBe("Hello");
            expect(calls).toEqual(["showModal"]);
            expect(document.activeElement).toBe(cancel);
            cancel.click();
            expect(calls).toEqual(["showModal", "close"]);
        } finally {
            page.remove();
        }
    });

    test("start does nothing in a browser that cannot show a modal dialog", () => {
        // Tests [@ANCHOR: user_websites:js_owner_delete_dialog_start]
        const { page, dialog, calls, link } = makePage({ withShowModal: false });
        try {
            const instance = Object.create(OwnerDeleteDialog.prototype);
            instance.el = dialog;
            instance.start();
            const click = new MouseEvent("click", { cancelable: true });
            link.dispatchEvent(click);
            expect(click.defaultPrevented).toBe(false);
            expect(calls.length).toBe(0);
        } finally {
            page.remove();
        }
    });
});

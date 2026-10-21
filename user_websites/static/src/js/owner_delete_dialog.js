/** @odoo-module **/

import { Interaction } from "@web/public/interaction";
import { registry } from "@web/core/registry";

/**
 * The owner dashboard's "Delete this page/post?" confirmation. Each `.uw-delete-link` carries the action URL, the kind
 * ("page" or "post") and the title in data attributes; clicking it fills in and opens the dialog instead of following the link.
 * This was an inline script until the strict Content-Security-Policy (no executable inline scripts) blocked it.
 */
export class OwnerDeleteDialog extends Interaction {
    static selector = "#uw_delete_dialog";

    start() {
        const dialog = this.el;
        if (typeof dialog.showModal !== "function") {
            return;
        }
        document.querySelectorAll(".uw-delete-link").forEach((link) => {
            link.addEventListener("click", (ev) => {
                ev.preventDefault();
                document.getElementById("uw_delete_form").setAttribute("action", link.dataset.uwAction);
                document.getElementById("uw_delete_what").textContent = link.dataset.uwWhat;
                document.getElementById("uw_delete_name").textContent = link.dataset.uwTitle;
                dialog.showModal();
                document.getElementById("uw_delete_cancel").focus();
            });
        });
        document.getElementById("uw_delete_cancel").addEventListener("click", () => dialog.close());
    }
}

registry.category("public.interactions").add("user_websites.owner_delete_dialog", OwnerDeleteDialog);

/** @odoo-module **/

import { Interaction } from "@web/public/interaction";
import { registry } from "@web/core/registry";

export class ViolationReportModal extends Interaction {
    static selector = '.user-websites-report-container';

    start() {
        // Find the modal element in the DOM
        const modalElement = document.getElementById('reportViolationModal');
        if (modalElement) {
            // Listen for the Bootstrap 5 'show' event, which fires right before the modal becomes visible
            modalElement.addEventListener('show.bs.modal', this._onModalShow.bind(this));
        }
    }

    /**
     * Handles the modal opening event.
     * Injects the URL and resets the form to clear any previous inputs.
     * @param {Event} ev
     */
    // [@ANCHOR: violation_report_logic]

    // Verified by [@ANCHOR: test_tour_violation_report]
    _onModalShow(ev) {
        // The button that triggered the modal is available via ev.relatedTarget in Bootstrap 5
        const button = ev.relatedTarget;
        if (!button) {
            return;
        }

        const url = button.getAttribute('data-url');
        const modal = ev.currentTarget;

        // 1. Inject the target URL
        const modalUrlInput = modal.querySelector('input[name="url"]');
        if (modalUrlInput) {
            modalUrlInput.value = url;
        }

        // 2. Clear previous user input (Description & Email)
        const descriptionInput = modal.querySelector('textarea[name="description"]');
        if (descriptionInput) {
            descriptionInput.value = '';
        }

        const emailInput = modal.querySelector('input[name="email"]');
        if (emailInput) {
            emailInput.value = '';
        }
    }
}
registry.category("public.interactions").add("user_websites.ViolationReportModal", ViolationReportModal);

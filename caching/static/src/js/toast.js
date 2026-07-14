/** Copyright © HAMS project. AGPL-3.0. **/
/** @odoo-module **/

import { Component, useState, xml } from "@odoo/owl";
import { registry } from "@web/core/registry";

export class SWToast extends Component {
    static template = xml`
        <div t-if="state.show" class="position-fixed bottom-0 end-0 p-3" style="z-index: 9999;">
            <div class="toast show text-bg-primary" role="alert" aria-live="assertive" aria-atomic="true">
                <div class="toast-header">
                    <strong class="me-auto">Update Available</strong>
                    <button type="button" class="btn-close" aria-label="Close" t-on-click="close"></button>
                </div>
                <div class="toast-body">
                    A new version of this app is available.
                    <button class="btn btn-light btn-sm ms-2" t-on-click="reload">Reload</button>
                </div>
            </div>
        </div>
    `;

    setup() {
        this.state = useState({ show: false });

        if ('serviceWorker' in navigator) {
            navigator.serviceWorker.addEventListener('message', (event) => {
                if (event.data && event.data.type === 'NEW_VERSION_INSTALLED') {
                    this.state.show = true;
                }
            });
        }
    }

    close() {
        this.state.show = false;
    }

    reload() {
        document.location.reload();
    }
}

registry.category("main_components").add("caching.SWToast", {
    Component: SWToast,
});
